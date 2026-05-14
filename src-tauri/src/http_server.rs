use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use crate::http_api;
use crate::services::workspaces;

const DEFAULT_MAX_BODY_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub allow_non_local: bool,
    pub operator_token: Option<String>,
    pub static_root: PathBuf,
    pub registry_path: PathBuf,
    pub max_body_bytes: usize,
    pub allowed_origins: Vec<String>,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, String> {
        let host = std::env::var("BEEHIVE_SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("BEEHIVE_SERVER_PORT")
            .ok()
            .map(|value| {
                value.parse::<u16>().map_err(|error| {
                    format!("BEEHIVE_SERVER_PORT must be a valid u16 port: {error}")
                })
            })
            .transpose()?
            .unwrap_or(8787);
        let allow_non_local = std::env::var("BEEHIVE_SERVER_ALLOW_NON_LOCAL")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let operator_token = std::env::var("BEEHIVE_OPERATOR_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        validate_bind_security(&host, allow_non_local, operator_token.as_deref())?;
        let max_body_bytes = std::env::var("BEEHIVE_SERVER_MAX_BODY_BYTES")
            .ok()
            .map(|value| {
                value.parse::<usize>().map_err(|error| {
                    format!("BEEHIVE_SERVER_MAX_BODY_BYTES must be a valid usize: {error}")
                })
            })
            .transpose()?
            .unwrap_or(DEFAULT_MAX_BODY_BYTES);
        let allowed_origins = parse_allowed_origins(
            std::env::var("BEEHIVE_ALLOWED_ORIGIN").ok(),
            !is_local_host(&host) || allow_non_local,
        )?;

        Ok(Self {
            host,
            port,
            allow_non_local,
            operator_token,
            static_root: default_static_root(),
            registry_path: workspaces::registry_path(),
            max_body_bytes,
            allowed_origins,
        })
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub fn run_server(config: ServerConfig) -> Result<(), String> {
    let listener = TcpListener::bind(config.bind_addr())
        .map_err(|error| format!("Failed to bind Beehive server: {error}"))?;
    log_json(
        "server_start",
        serde_json::json!({
            "url": format!("http://{}", config.bind_addr()),
            "registry_path": config.registry_path.display().to_string(),
            "static_root": config.static_root.display().to_string(),
            "max_body_bytes": config.max_body_bytes,
            "allowed_origins": &config.allowed_origins,
            "token_configured": config.operator_token.is_some(),
            "allow_non_local": config.allow_non_local,
        }),
    );
    if config.static_root.exists() {
        println!("Serving frontend from {}", config.static_root.display());
    } else {
        println!(
            "Frontend dist not found at {}; API server only.",
            config.static_root.display()
        );
    }

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let request_config = config.clone();
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, &request_config) {
                        log_json(
                            "request_failed",
                            serde_json::json!({
                                "message": error,
                            }),
                        );
                    }
                });
            }
            Err(error) => log_json(
                "request_failed",
                serde_json::json!({
                    "message": format!("connection failed: {error}"),
                }),
            ),
        }
    }

    Ok(())
}

pub fn validate_bind_security(
    host: &str,
    allow_non_local: bool,
    token: Option<&str>,
) -> Result<(), String> {
    if is_local_host(host) {
        return Ok(());
    }
    if !allow_non_local {
        return Err(
            "Non-local bind requires BEEHIVE_SERVER_ALLOW_NON_LOCAL=1 and BEEHIVE_OPERATOR_TOKEN."
                .to_string(),
        );
    }
    if token.is_none_or(|value| value.trim().is_empty()) {
        return Err("Non-local bind requires BEEHIVE_OPERATOR_TOKEN.".to_string());
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, config: &ServerConfig) -> Result<(), String> {
    let started = Instant::now();
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("Failed to clone TCP stream: {error}"))?,
    );
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| format!("Failed to read request line: {error}"))?;
    let request_line = request_line.trim_end();
    if request_line.is_empty() {
        return Ok(());
    }
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| "HTTP method is missing.".to_string())?;
    let target = request_parts
        .next()
        .ok_or_else(|| "HTTP path is missing.".to_string())?;
    let path = target.split('?').next().unwrap_or(target);

    let headers = read_headers(&mut reader)?;
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if request_body_too_large(content_length, config.max_body_bytes) {
        log_request_completed(method, path, 413, started.elapsed().as_millis());
        return write_json_response(
            &mut stream,
            413,
            r#"{"errors":[{"code":"payload_too_large","message":"Request body exceeds BEEHIVE_SERVER_MAX_BODY_BYTES.","path":null}]}"#,
            config,
            &headers,
        );
    }
    if method == "OPTIONS" {
        log_request_completed(method, path, 204, started.elapsed().as_millis());
        return write_empty_response(&mut stream, 204, config, &headers);
    }
    let mut body_bytes = vec![0_u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body_bytes)
            .map_err(|error| format!("Failed to read request body: {error}"))?;
    }
    let body = if body_bytes.is_empty() {
        None
    } else {
        Some(
            std::str::from_utf8(&body_bytes)
                .map_err(|error| format!("Request body is not valid UTF-8: {error}"))?,
        )
    };

    if path.starts_with("/api/") {
        if let Some(token) = config.operator_token.as_deref() {
            if !authorization_matches(&headers, token) {
                log_request_completed(method, path, 401, started.elapsed().as_millis());
                return write_json_response(
                    &mut stream,
                    401,
                    r#"{"errors":[{"code":"unauthorized","message":"Authorization bearer token is required.","path":null}]}"#,
                    config,
                    &headers,
                );
            }
        }
        let response = http_api::handle_json_request(method, path, body);
        let body = serde_json::to_string(&response.body)
            .map_err(|error| format!("Failed to serialize API response: {error}"))?;
        log_workspace_action(method, path, response.status_code);
        log_request_completed(
            method,
            path,
            response.status_code,
            started.elapsed().as_millis(),
        );
        return write_json_response(&mut stream, response.status_code, &body, config, &headers);
    }

    let status_code = serve_static(&mut stream, path, config, &headers)?;
    log_request_completed(method, path, status_code, started.elapsed().as_millis());
    Ok(())
}

fn read_headers(reader: &mut BufReader<TcpStream>) -> Result<HashMap<String, String>, String> {
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| format!("Failed to read HTTP header: {error}"))?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Ok(headers)
}

fn serve_static(
    stream: &mut TcpStream,
    path: &str,
    config: &ServerConfig,
    request_headers: &HashMap<String, String>,
) -> Result<u16, String> {
    let static_root = &config.static_root;
    if !static_root.exists() {
        write_plain_response(
            stream,
            404,
            "Frontend dist directory was not found.",
            config,
            request_headers,
        )?;
        return Ok(404);
    }
    let relative = match path {
        "/" | "" => "index.html",
        value => value.trim_start_matches('/'),
    };
    if relative
        .split('/')
        .any(|component| component == ".." || component == "." || component.is_empty())
        && relative != "index.html"
    {
        write_plain_response(stream, 400, "Invalid static path.", config, request_headers)?;
        return Ok(400);
    }
    let candidate = static_root.join(relative);
    let file_path = if candidate.exists() && candidate.is_file() {
        candidate
    } else {
        static_root.join("index.html")
    };
    if !file_path.exists() {
        write_plain_response(
            stream,
            404,
            "Frontend entrypoint was not found.",
            config,
            request_headers,
        )?;
        return Ok(404);
    }
    let bytes = fs::read(&file_path).map_err(|error| {
        format!(
            "Failed to read static file '{}': {error}",
            file_path.display()
        )
    })?;
    write_response(
        stream,
        200,
        content_type(&file_path),
        &bytes,
        extra_headers(config, request_headers),
    )?;
    Ok(200)
}

fn write_json_response(
    stream: &mut TcpStream,
    status_code: u16,
    body: &str,
    config: &ServerConfig,
    request_headers: &HashMap<String, String>,
) -> Result<(), String> {
    write_response(
        stream,
        status_code,
        "application/json; charset=utf-8",
        body.as_bytes(),
        extra_headers(config, request_headers),
    )
}

fn write_plain_response(
    stream: &mut TcpStream,
    status_code: u16,
    body: &str,
    config: &ServerConfig,
    request_headers: &HashMap<String, String>,
) -> Result<(), String> {
    write_response(
        stream,
        status_code,
        "text/plain; charset=utf-8",
        body.as_bytes(),
        extra_headers(config, request_headers),
    )
}

fn write_empty_response(
    stream: &mut TcpStream,
    status_code: u16,
    config: &ServerConfig,
    request_headers: &HashMap<String, String>,
) -> Result<(), String> {
    write_response(
        stream,
        status_code,
        "text/plain; charset=utf-8",
        &[],
        extra_headers(config, request_headers),
    )
}

fn write_response(
    stream: &mut TcpStream,
    status_code: u16,
    content_type: &str,
    body: &[u8],
    headers: Vec<(String, String)>,
) -> Result<(), String> {
    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        status_code,
        reason_phrase(status_code),
        content_type,
        body.len()
    );
    for (name, value) in headers {
        response.push_str(&name);
        response.push_str(": ");
        response.push_str(&value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    stream
        .write_all(response.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|error| format!("Failed to write HTTP response: {error}"))
}

fn authorization_matches(headers: &HashMap<String, String>, token: &str) -> bool {
    headers
        .get("authorization")
        .map(|value| value == &format!("Bearer {token}"))
        .unwrap_or(false)
}

fn extra_headers(
    config: &ServerConfig,
    request_headers: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut headers = vec![
        (
            "Access-Control-Allow-Headers".to_string(),
            "Authorization, Content-Type, Accept".to_string(),
        ),
        (
            "Access-Control-Allow-Methods".to_string(),
            "GET, POST, OPTIONS".to_string(),
        ),
        ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
    ];
    if let Some(origin) = request_headers.get("origin") {
        if origin_allowed(origin, &config.allowed_origins) {
            headers.push(("Access-Control-Allow-Origin".to_string(), origin.clone()));
            headers.push(("Vary".to_string(), "Origin".to_string()));
        }
    }
    headers
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

fn reason_phrase(status_code: u16) -> &'static str {
    match status_code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn parse_allowed_origins(
    raw: Option<String>,
    non_local_enabled: bool,
) -> Result<Vec<String>, String> {
    let origins = raw
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(default_allowed_origins);
    if non_local_enabled && origins.iter().any(|origin| origin == "*") {
        return Err(
            "BEEHIVE_ALLOWED_ORIGIN='*' is not allowed when non-local bind is enabled.".to_string(),
        );
    }
    Ok(origins)
}

fn default_allowed_origins() -> Vec<String> {
    vec![
        "http://127.0.0.1:8787".to_string(),
        "http://localhost:8787".to_string(),
        "http://127.0.0.1:5173".to_string(),
        "http://localhost:5173".to_string(),
    ]
}

fn origin_allowed(origin: &str, allowed_origins: &[String]) -> bool {
    allowed_origins
        .iter()
        .any(|allowed| allowed == "*" || allowed == origin)
}

fn request_body_too_large(content_length: usize, max_body_bytes: usize) -> bool {
    content_length > max_body_bytes
}

fn is_local_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn default_static_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|path| path.join("dist"))
        .unwrap_or_else(|| PathBuf::from("dist"))
}

fn log_json(event: &str, payload: serde_json::Value) {
    println!(
        "{}",
        serde_json::json!({
            "event": event,
            "ts": chrono::Utc::now().to_rfc3339(),
            "payload": payload,
        })
    );
}

fn log_request_completed(method: &str, path: &str, status_code: u16, duration_ms: u128) {
    log_json(
        "request_completed",
        serde_json::json!({
            "method": method,
            "path": path,
            "status_code": status_code,
            "duration_ms": duration_ms,
        }),
    );
}

fn log_workspace_action(method: &str, path: &str, status_code: u16) {
    let parts = path
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() >= 3 && parts[0] == "api" && parts[1] == "workspaces" {
        log_json(
            "workspace_action",
            serde_json::json!({
                "method": method,
                "workspace_id": parts[2],
                "route": parts.get(3).copied().unwrap_or("workspace"),
                "status_code": status_code,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_bind_does_not_require_token() {
        assert!(validate_bind_security("127.0.0.1", false, None).is_ok());
        assert!(validate_bind_security("localhost", false, None).is_ok());
    }

    #[test]
    fn non_local_bind_requires_opt_in_and_token() {
        assert!(validate_bind_security("0.0.0.0", false, None).is_err());
        assert!(validate_bind_security("0.0.0.0", true, None).is_err());
        assert!(validate_bind_security("0.0.0.0", true, Some("token")).is_ok());
    }

    #[test]
    fn request_body_limit_rejects_oversized_content_length() {
        assert!(!request_body_too_large(1024, 1024));
        assert!(request_body_too_large(1025, 1024));
    }

    #[test]
    fn cors_defaults_allow_local_dev_origins_without_wildcard() {
        let origins = parse_allowed_origins(None, false).expect("origins");

        assert!(origin_allowed("http://127.0.0.1:5173", &origins));
        assert!(origin_allowed("http://localhost:8787", &origins));
        assert!(!origins.iter().any(|origin| origin == "*"));
    }

    #[test]
    fn non_local_cors_rejects_wildcard_origin() {
        let result = parse_allowed_origins(Some("*".to_string()), true);

        assert!(result.is_err());
    }
}

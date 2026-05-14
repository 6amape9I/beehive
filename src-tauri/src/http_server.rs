use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

use crate::http_api;
use crate::services::workspaces;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub allow_non_local: bool,
    pub operator_token: Option<String>,
    pub static_root: PathBuf,
    pub registry_path: PathBuf,
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

        Ok(Self {
            host,
            port,
            allow_non_local,
            operator_token,
            static_root: default_static_root(),
            registry_path: workspaces::registry_path(),
        })
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub fn run_server(config: ServerConfig) -> Result<(), String> {
    let listener = TcpListener::bind(config.bind_addr())
        .map_err(|error| format!("Failed to bind Beehive server: {error}"))?;
    println!("Beehive server listening on http://{}", config.bind_addr());
    println!("Workspace registry: {}", config.registry_path.display());
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
                        eprintln!("Beehive server request failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("Beehive server connection failed: {error}"),
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

    if method == "OPTIONS" {
        return write_empty_response(&mut stream, 204);
    }

    if path.starts_with("/api/") {
        if let Some(token) = config.operator_token.as_deref() {
            if !authorization_matches(&headers, token) {
                return write_json_response(
                    &mut stream,
                    401,
                    r#"{"errors":[{"code":"unauthorized","message":"Authorization bearer token is required.","path":null}]}"#,
                );
            }
        }
        let response = http_api::handle_json_request(method, path, body);
        let body = serde_json::to_string(&response.body)
            .map_err(|error| format!("Failed to serialize API response: {error}"))?;
        return write_json_response(&mut stream, response.status_code, &body);
    }

    serve_static(&mut stream, path, &config.static_root)
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

fn serve_static(stream: &mut TcpStream, path: &str, static_root: &Path) -> Result<(), String> {
    if !static_root.exists() {
        return write_plain_response(stream, 404, "Frontend dist directory was not found.");
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
        return write_plain_response(stream, 400, "Invalid static path.");
    }
    let candidate = static_root.join(relative);
    let file_path = if candidate.exists() && candidate.is_file() {
        candidate
    } else {
        static_root.join("index.html")
    };
    if !file_path.exists() {
        return write_plain_response(stream, 404, "Frontend entrypoint was not found.");
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
        extra_headers(),
    )
}

fn write_json_response(stream: &mut TcpStream, status_code: u16, body: &str) -> Result<(), String> {
    write_response(
        stream,
        status_code,
        "application/json; charset=utf-8",
        body.as_bytes(),
        extra_headers(),
    )
}

fn write_plain_response(
    stream: &mut TcpStream,
    status_code: u16,
    body: &str,
) -> Result<(), String> {
    write_response(
        stream,
        status_code,
        "text/plain; charset=utf-8",
        body.as_bytes(),
        extra_headers(),
    )
}

fn write_empty_response(stream: &mut TcpStream, status_code: u16) -> Result<(), String> {
    write_response(
        stream,
        status_code,
        "text/plain; charset=utf-8",
        &[],
        extra_headers(),
    )
}

fn write_response(
    stream: &mut TcpStream,
    status_code: u16,
    content_type: &str,
    body: &[u8],
    headers: Vec<(&'static str, &'static str)>,
) -> Result<(), String> {
    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        status_code,
        reason_phrase(status_code),
        content_type,
        body.len()
    );
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
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

fn extra_headers() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Access-Control-Allow-Origin", "*"),
        (
            "Access-Control-Allow-Headers",
            "Authorization, Content-Type, Accept",
        ),
        ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        ("X-Content-Type-Options", "nosniff"),
    ]
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
        500 => "Internal Server Error",
        _ => "OK",
    }
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
}

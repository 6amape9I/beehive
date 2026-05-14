use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

use aws_config::BehaviorVersion;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use tokio::runtime::Runtime;

use crate::domain::S3StorageConfig;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct S3ObjectMetadata {
    pub bucket: String,
    pub key: String,
    pub version_id: Option<String>,
    pub etag: Option<String>,
    pub checksum_sha256: Option<String>,
    pub size: Option<u64>,
    pub last_modified: Option<String>,
    pub metadata: HashMap<String, String>,
}

pub(crate) trait S3MetadataClient {
    fn list_objects(&self, bucket: &str, prefix: &str) -> Result<Vec<S3ObjectMetadata>, String>;
    fn head_object(&self, bucket: &str, key: &str) -> Result<Option<S3ObjectMetadata>, String>;
}

pub(crate) struct AwsS3MetadataClient {
    runtime: Runtime,
    client: Client,
}

impl AwsS3MetadataClient {
    pub(crate) fn from_storage_config(storage: &S3StorageConfig) -> Result<Self, String> {
        let env_values = EnvValues::load();
        let region = first_config_or_env(
            storage.region.as_deref(),
            &env_values,
            &["BEEHIVE_S3_REGION", "AWS_REGION", "S3_REGION"],
        );
        let endpoint = first_config_or_env(
            storage.endpoint.as_deref(),
            &env_values,
            &["BEEHIVE_S3_ENDPOINT", "S3_HOST"],
        );
        let access_key = env_values.first(&["AWS_ACCESS_KEY_ID", "S3_KEY"]);
        let secret_key = env_values.first(&["AWS_SECRET_ACCESS_KEY", "S3_SEC_KEY"]);
        let session_token = env_values.first(&["AWS_SESSION_TOKEN"]);

        let runtime = Runtime::new()
            .map_err(|error| format!("Failed to create AWS S3 async runtime: {error}"))?;
        let mut loader = aws_config::defaults(BehaviorVersion::latest());
        if let Some(region) = region {
            loader = loader.region(Region::new(region));
        }
        if let Some(endpoint) = endpoint {
            loader = loader.endpoint_url(endpoint);
        }
        if let (Some(access_key), Some(secret_key)) = (access_key, secret_key) {
            let credentials =
                Credentials::new(access_key, secret_key, session_token, None, "beehive-env");
            loader = loader.credentials_provider(SharedCredentialsProvider::new(credentials));
        }

        let shared_config = runtime.block_on(loader.load());
        let config = aws_sdk_s3::config::Builder::from(&shared_config)
            .force_path_style(true)
            .build();
        Ok(Self {
            runtime,
            client: Client::from_conf(config),
        })
    }

    pub(crate) fn put_json_object(
        &self,
        bucket: &str,
        key: &str,
        bytes: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> Result<S3ObjectMetadata, String> {
        let size = bytes.len() as u64;
        let output = self
            .runtime
            .block_on(
                self.client
                    .put_object()
                    .bucket(bucket)
                    .key(key)
                    .content_type("application/json")
                    .set_metadata(Some(metadata.clone()))
                    .body(ByteStream::from(bytes))
                    .send(),
            )
            .map_err(|error| format!("Failed to upload S3 object s3://{bucket}/{key}: {error}"))?;

        Ok(S3ObjectMetadata {
            bucket: bucket.to_string(),
            key: key.to_string(),
            version_id: output.version_id().map(ToOwned::to_owned),
            etag: output.e_tag().map(ToOwned::to_owned),
            checksum_sha256: output.checksum_sha256().map(ToOwned::to_owned),
            size: Some(size),
            last_modified: None,
            metadata,
        })
    }
}

impl S3MetadataClient for AwsS3MetadataClient {
    fn list_objects(&self, bucket: &str, prefix: &str) -> Result<Vec<S3ObjectMetadata>, String> {
        let mut objects = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut request = self.client.list_objects_v2().bucket(bucket).prefix(prefix);
            if let Some(token) = continuation_token.as_deref() {
                request = request.continuation_token(token);
            }

            let output = self.runtime.block_on(request.send()).map_err(|error| {
                format!("Failed to list S3 objects under s3://{bucket}/{prefix}: {error}")
            })?;
            for object in output.contents() {
                let Some(key) = object.key() else {
                    continue;
                };
                objects.push(S3ObjectMetadata {
                    bucket: bucket.to_string(),
                    key: key.to_string(),
                    version_id: None,
                    etag: object.e_tag().map(ToOwned::to_owned),
                    checksum_sha256: None,
                    size: object.size().and_then(non_negative_i64_to_u64),
                    last_modified: object.last_modified().map(ToString::to_string),
                    metadata: HashMap::new(),
                });
            }

            if output.is_truncated().unwrap_or(false) {
                continuation_token = output.next_continuation_token().map(ToOwned::to_owned);
                if continuation_token.is_none() {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(objects)
    }

    fn head_object(&self, bucket: &str, key: &str) -> Result<Option<S3ObjectMetadata>, String> {
        let output = match self
            .runtime
            .block_on(self.client.head_object().bucket(bucket).key(key).send())
        {
            Ok(output) => output,
            Err(error) => {
                let display_message = error.to_string();
                let debug_message = format!("{error:?}");
                if is_not_found_error(&display_message) || is_not_found_error(&debug_message) {
                    return Ok(None);
                }
                return Err(format!(
                    "Failed to head S3 object s3://{bucket}/{key}: {display_message}; {debug_message}"
                ));
            }
        };

        Ok(Some(S3ObjectMetadata {
            bucket: bucket.to_string(),
            key: key.to_string(),
            version_id: output.version_id().map(ToOwned::to_owned),
            etag: output.e_tag().map(ToOwned::to_owned),
            checksum_sha256: output.checksum_sha256().map(ToOwned::to_owned),
            size: output.content_length().and_then(non_negative_i64_to_u64),
            last_modified: output.last_modified().map(ToString::to_string),
            metadata: output
                .metadata()
                .map(|metadata| {
                    metadata
                        .iter()
                        .map(|(key, value)| (key.to_string(), value.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        }))
    }
}

fn non_negative_i64_to_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

fn is_not_found_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("notfound")
        || normalized.contains("not found")
        || normalized.contains("no such key")
        || normalized.contains("nosuchkey")
        || normalized.contains("status: 404")
        || normalized.contains("404")
}

fn first_config_or_env(
    config_value: Option<&str>,
    env_values: &EnvValues,
    keys: &[&str],
) -> Option<String> {
    config_value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| env_values.first(keys))
}

#[derive(Debug, Default)]
struct EnvValues {
    values: HashMap<String, String>,
}

impl EnvValues {
    fn load() -> Self {
        let mut values = HashMap::new();
        for (key, value) in env::vars() {
            if !value.trim().is_empty() {
                values.insert(key, value);
            }
        }
        load_dotenv_values(Path::new(".env"), &mut values);
        Self { values }
    }

    fn first(&self, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|key| {
            self.values
                .get(*key)
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
    }
}

fn load_dotenv_values(path: &Path, values: &mut HashMap<String, String>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        if key.is_empty() || values.contains_key(key) {
            continue;
        }
        let value = trim_dotenv_value(raw_value.trim());
        if !value.is_empty() {
            values.insert(key.to_string(), value);
        }
    }
}

fn trim_dotenv_value(value: &str) -> String {
    let mut trimmed = value.trim().to_string();
    if let Some(comment_start) = trimmed.find(" #") {
        trimmed.truncate(comment_start);
        trimmed = trimmed.trim().to_string();
    }
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"')
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_values_preserve_process_env_precedence() {
        let mut values = HashMap::new();
        values.insert("S3_REGION".to_string(), "from-process".to_string());
        load_dotenv_values(Path::new("does-not-exist.env"), &mut values);

        let env_values = EnvValues { values };
        assert_eq!(
            env_values
                .first(&["BEEHIVE_S3_REGION", "S3_REGION"])
                .as_deref(),
            Some("from-process")
        );
    }

    #[test]
    fn dotenv_value_trimming_handles_quotes_and_comments() {
        assert_eq!(
            trim_dotenv_value("\"https://example.test\""),
            "https://example.test"
        );
        assert_eq!(trim_dotenv_value("'region-1'"), "region-1");
        assert_eq!(trim_dotenv_value("value # comment"), "value");
    }

    #[test]
    fn not_found_detection_handles_sdk_debug_status() {
        assert!(is_not_found_error(
            "ServiceError { raw: Response { status: 404, body: SdkBody } }"
        ));
        assert!(is_not_found_error("NoSuchKey"));
    }
}

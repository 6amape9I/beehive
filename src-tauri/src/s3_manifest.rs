use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{ArtifactLocation, S3StorageConfig, StageRecord, StorageProvider};
use crate::save_path::{resolve_s3_save_path_route, SavePathRouteErrorKind};

pub(crate) const S3_ARTIFACT_MANIFEST_SCHEMA: &str = "beehive.s3_artifact_manifest.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct S3ArtifactManifest {
    pub schema: String,
    pub workspace_id: String,
    pub run_id: String,
    pub source: S3ManifestSource,
    pub status: S3ManifestStatus,
    #[serde(default)]
    pub outputs: Vec<S3ManifestOutput>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct S3ManifestSource {
    pub bucket: String,
    pub key: String,
    pub version_id: Option<String>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum S3ManifestStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct S3ManifestOutput {
    pub artifact_id: String,
    pub bucket: String,
    pub key: String,
    pub save_path: String,
    pub content_type: Option<String>,
    pub checksum_sha256: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum S3ManifestValidationErrorKind {
    Invalid,
    BlockedRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct S3ManifestValidationError {
    pub kind: S3ManifestValidationErrorKind,
    pub message: String,
}

#[derive(Debug, Clone)]
pub(crate) struct S3ManifestValidationContext {
    pub workspace_id: String,
    pub run_id: String,
    pub source: ArtifactLocation,
    pub storage: S3StorageConfig,
    pub source_stage: StageRecord,
    pub active_stages: Vec<StageRecord>,
}

#[derive(Debug, Clone)]
pub(crate) struct ValidatedS3Manifest {
    pub manifest: S3ArtifactManifest,
    pub outputs: Vec<ResolvedS3ManifestOutput>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedS3ManifestOutput {
    pub output: S3ManifestOutput,
    pub target_stage: StageRecord,
    pub location: ArtifactLocation,
}

pub(crate) fn parse_and_validate_s3_manifest(
    body: &str,
    context: &S3ManifestValidationContext,
) -> Result<ValidatedS3Manifest, S3ManifestValidationError> {
    let root = serde_json::from_str::<Value>(body).map_err(|error| invalid(format!("{error}")))?;
    reject_business_payload_fields(&root)?;
    let manifest = serde_json::from_value::<S3ArtifactManifest>(root)
        .map_err(|error| invalid(format!("Manifest JSON does not match schema: {error}")))?;

    if manifest.schema != S3_ARTIFACT_MANIFEST_SCHEMA {
        return Err(invalid(format!(
            "Manifest schema must be '{}'.",
            S3_ARTIFACT_MANIFEST_SCHEMA
        )));
    }
    if manifest.workspace_id.trim().is_empty() || manifest.workspace_id != context.workspace_id {
        return Err(invalid(
            "Manifest workspace_id does not match active workspace.",
        ));
    }
    if manifest.run_id.trim().is_empty() || manifest.run_id != context.run_id {
        return Err(invalid("Manifest run_id does not match active stage run."));
    }
    let expected_bucket = context
        .source
        .bucket
        .as_deref()
        .ok_or_else(|| invalid("Claimed source artifact does not have an S3 bucket."))?;
    let expected_key = context
        .source
        .key
        .as_deref()
        .ok_or_else(|| invalid("Claimed source artifact does not have an S3 key."))?;
    if manifest.source.bucket != expected_bucket || manifest.source.key != expected_key {
        return Err(invalid(
            "Manifest source bucket/key does not match claimed artifact.",
        ));
    }

    match manifest.status {
        S3ManifestStatus::Success => validate_success_manifest(manifest, context),
        S3ManifestStatus::Error => validate_error_manifest(manifest),
    }
}

fn validate_success_manifest(
    manifest: S3ArtifactManifest,
    context: &S3ManifestValidationContext,
) -> Result<ValidatedS3Manifest, S3ManifestValidationError> {
    if manifest.outputs.is_empty() && context.source_stage.next_stage.is_some() {
        return Err(invalid(
            "Success manifest may omit outputs only for terminal/no-output stages.",
        ));
    }
    let mut outputs = Vec::new();
    for output in &manifest.outputs {
        if output.artifact_id.trim().is_empty() {
            return Err(invalid("Manifest output artifact_id is required."));
        }
        if output.bucket != context.storage.bucket {
            return Err(invalid(format!(
                "Manifest output bucket '{}' is not configured storage bucket '{}'.",
                output.bucket, context.storage.bucket
            )));
        }
        let route =
            resolve_s3_save_path_route(&output.save_path, &context.storage, &context.active_stages)
                .map_err(|error| match error.kind {
                    SavePathRouteErrorKind::Unsafe
                    | SavePathRouteErrorKind::Unknown
                    | SavePathRouteErrorKind::Ambiguous => S3ManifestValidationError {
                        kind: S3ManifestValidationErrorKind::BlockedRoute,
                        message: error.message,
                    },
                })?;
        let Some(prefix) = route.location.key.as_deref() else {
            return Err(invalid("Resolved S3 route did not produce a key prefix."));
        };
        if output.key != prefix && !output.key.starts_with(&format!("{prefix}/")) {
            return Err(S3ManifestValidationError {
                kind: S3ManifestValidationErrorKind::BlockedRoute,
                message: format!(
                    "Manifest output key '{}' is outside resolved target prefix '{}'.",
                    output.key, prefix
                ),
            });
        }
        outputs.push(ResolvedS3ManifestOutput {
            output: output.clone(),
            target_stage: route.stage,
            location: ArtifactLocation {
                provider: StorageProvider::S3,
                local_path: None,
                bucket: Some(output.bucket.clone()),
                key: Some(output.key.clone()),
                version_id: None,
                etag: None,
                checksum_sha256: output.checksum_sha256.clone(),
                size: output.size,
            },
        });
    }
    Ok(ValidatedS3Manifest { manifest, outputs })
}

fn validate_error_manifest(
    manifest: S3ArtifactManifest,
) -> Result<ValidatedS3Manifest, S3ManifestValidationError> {
    if !manifest.outputs.is_empty() {
        return Err(invalid("Error manifest must not contain outputs."));
    }
    if manifest
        .error_type
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Err(invalid("Error manifest must include error_type."));
    }
    if manifest
        .error_message
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Err(invalid("Error manifest must include error_message."));
    }
    Ok(ValidatedS3Manifest {
        manifest,
        outputs: Vec::new(),
    })
}

fn reject_business_payload_fields(value: &Value) -> Result<(), S3ManifestValidationError> {
    let Some(root) = value.as_object() else {
        return Err(invalid("Manifest root must be a JSON object."));
    };
    for forbidden in ["payload", "business_payload", "business_json", "data"] {
        if root.contains_key(forbidden) {
            return Err(invalid(format!(
                "Manifest must not contain business payload field '{}'.",
                forbidden
            )));
        }
    }
    if let Some(outputs) = root.get("outputs").and_then(Value::as_array) {
        for output in outputs {
            if let Some(output) = output.as_object() {
                for forbidden in ["payload", "business_payload", "business_json", "data"] {
                    if output.contains_key(forbidden) {
                        return Err(invalid(format!(
                            "Manifest output must not contain business payload field '{}'.",
                            forbidden
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> S3ManifestValidationError {
    S3ManifestValidationError {
        kind: S3ManifestValidationErrorKind::Invalid,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(id: &str, input_uri: &str, next_stage: Option<&str>) -> StageRecord {
        StageRecord {
            id: id.to_string(),
            input_folder: String::new(),
            input_uri: Some(input_uri.to_string()),
            output_folder: String::new(),
            workflow_url: "http://localhost:5678/webhook/test".to_string(),
            max_attempts: 3,
            retry_delay_sec: 0,
            next_stage: next_stage.map(ToOwned::to_owned),
            save_path_aliases: Vec::new(),
            is_active: true,
            archived_at: None,
            last_seen_in_config_at: None,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
            entity_count: 0,
        }
    }

    fn context() -> S3ManifestValidationContext {
        let source_stage = stage(
            "raw",
            "s3://steos-s3-data/main_dir/raw",
            Some("raw_entities"),
        );
        let target_stage = stage(
            "raw_entities",
            "s3://steos-s3-data/main_dir/processed/raw_entities",
            None,
        );
        S3ManifestValidationContext {
            workspace_id: "beehive-s3-dev".to_string(),
            run_id: "run_123".to_string(),
            source: ArtifactLocation {
                provider: StorageProvider::S3,
                local_path: None,
                bucket: Some("steos-s3-data".to_string()),
                key: Some("main_dir/raw/input_001.json".to_string()),
                version_id: None,
                etag: None,
                checksum_sha256: None,
                size: None,
            },
            storage: S3StorageConfig {
                bucket: "steos-s3-data".to_string(),
                workspace_prefix: "main_dir".to_string(),
                region: None,
                endpoint: None,
            },
            source_stage,
            active_stages: vec![target_stage],
        }
    }

    fn success_manifest(save_path: &str) -> String {
        format!(
            r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-s3-dev",
  "run_id":"run_123",
  "source":{{"bucket":"steos-s3-data","key":"main_dir/raw/input_001.json","version_id":null,"etag":null}},
  "status":"success",
  "outputs":[{{"artifact_id":"art_001","bucket":"steos-s3-data","key":"main_dir/processed/raw_entities/art_001.json","save_path":"{save_path}","content_type":"application/json","checksum_sha256":null,"size":123}}],
  "created_at":"2026-05-12T00:00:00Z"
}}"#
        )
    }

    #[test]
    fn valid_success_manifest_parses_and_resolves_output_route() {
        let validated = parse_and_validate_s3_manifest(
            &success_manifest("main_dir/processed/raw_entities"),
            &context(),
        )
        .expect("valid manifest");

        assert_eq!(validated.manifest.run_id, "run_123");
        assert_eq!(validated.outputs.len(), 1);
        assert_eq!(validated.outputs[0].target_stage.id, "raw_entities");
        assert_eq!(
            validated.outputs[0].location.key.as_deref(),
            Some("main_dir/processed/raw_entities/art_001.json")
        );
    }

    #[test]
    fn valid_error_manifest_parses() {
        let manifest = r#"{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-s3-dev",
  "run_id":"run_123",
  "source":{"bucket":"steos-s3-data","key":"main_dir/raw/input_001.json"},
  "status":"error",
  "error_type":"llm_invalid_json",
  "error_message":"Model returned invalid JSON",
  "outputs":[],
  "created_at":"2026-05-12T00:00:00Z"
}"#;
        let validated =
            parse_and_validate_s3_manifest(manifest, &context()).expect("error manifest");

        assert_eq!(validated.manifest.status, S3ManifestStatus::Error);
        assert!(validated.outputs.is_empty());
    }

    #[test]
    fn invalid_manifest_contracts_reject() {
        for manifest in [
            success_manifest("main_dir/processed/unknown"),
            success_manifest("s3://unknown-bucket/main_dir/processed/raw_entities"),
            success_manifest("main_dir/processed/raw_entities")
                .replace("beehive.s3_artifact_manifest.v1", "wrong"),
            success_manifest("main_dir/processed/raw_entities").replace("run_123", "run_999"),
            success_manifest("main_dir/processed/raw_entities")
                .replace("main_dir/raw/input_001.json", "main_dir/raw/other.json"),
            success_manifest("main_dir/processed/raw_entities").replace(
                "\"bucket\":\"steos-s3-data\"",
                "\"bucket\":\"other-bucket\"",
            ),
            success_manifest("main_dir/processed/raw_entities")
                .replace("\"outputs\":[", "\"payload\":{\"x\":1},\"outputs\":["),
        ] {
            assert!(
                parse_and_validate_s3_manifest(&manifest, &context()).is_err(),
                "manifest should reject: {manifest}"
            );
        }
    }
}

use serde::{Deserialize, Serialize};

pub(crate) const S3_CONTROL_ENVELOPE_SCHEMA: &str = "beehive.s3_control_envelope.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct S3ControlEnvelopeParts {
    pub(crate) workspace_id: String,
    pub(crate) run_id: String,
    pub(crate) stage_id: String,
    pub(crate) source_bucket: String,
    pub(crate) source_key: String,
    pub(crate) source_version_id: Option<String>,
    pub(crate) source_etag: Option<String>,
    pub(crate) source_entity_id: String,
    pub(crate) source_artifact_id: String,
    pub(crate) manifest_prefix: String,
    pub(crate) workspace_prefix: String,
    pub(crate) target_prefix: String,
    pub(crate) save_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct S3ControlEnvelope {
    pub(crate) schema: String,
    pub(crate) workspace_id: String,
    pub(crate) run_id: String,
    pub(crate) stage_id: String,
    pub(crate) source_bucket: String,
    pub(crate) source_key: String,
    pub(crate) source_version_id: Option<String>,
    pub(crate) source_etag: Option<String>,
    pub(crate) source_entity_id: String,
    pub(crate) source_artifact_id: String,
    pub(crate) manifest_prefix: String,
    pub(crate) workspace_prefix: String,
    pub(crate) target_prefix: String,
    pub(crate) save_path: String,
}

impl S3ControlEnvelope {
    pub(crate) fn from_parts(parts: S3ControlEnvelopeParts) -> Self {
        Self {
            schema: S3_CONTROL_ENVELOPE_SCHEMA.to_string(),
            workspace_id: parts.workspace_id,
            run_id: parts.run_id,
            stage_id: parts.stage_id,
            source_bucket: parts.source_bucket,
            source_key: parts.source_key,
            source_version_id: parts.source_version_id,
            source_etag: parts.source_etag,
            source_entity_id: parts.source_entity_id,
            source_artifact_id: parts.source_artifact_id,
            manifest_prefix: parts.manifest_prefix,
            workspace_prefix: parts.workspace_prefix,
            target_prefix: parts.target_prefix,
            save_path: parts.save_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s3_control_envelope_uses_canonical_schema_and_preserves_cyrillic_key() {
        let envelope = S3ControlEnvelope::from_parts(S3ControlEnvelopeParts {
            workspace_id: "beehive-s3-dev".to_string(),
            run_id: "run-001".to_string(),
            stage_id: "smoke_source".to_string(),
            source_bucket: "steos-s3-data".to_string(),
            source_key: "beehive-smoke/raw/порфирия.json".to_string(),
            source_version_id: None,
            source_etag: Some("etag-source".to_string()),
            source_entity_id: "entity-001".to_string(),
            source_artifact_id: "artifact-001".to_string(),
            manifest_prefix: "beehive-smoke/runs/run-001/".to_string(),
            workspace_prefix: "beehive-smoke".to_string(),
            target_prefix: "beehive-smoke/processed".to_string(),
            save_path: "beehive-smoke/processed".to_string(),
        });

        let body = serde_json::to_string(&envelope).expect("serialize envelope");
        let value = serde_json::to_value(&envelope).expect("envelope value");

        assert_eq!(value["schema"].as_str(), Some(S3_CONTROL_ENVELOPE_SCHEMA));
        assert_eq!(
            value["source_key"].as_str(),
            Some("beehive-smoke/raw/порфирия.json")
        );
        assert!(body.contains("порфирия"));
        assert!(value.get("payload_json").is_none());
        assert!(value.get("raw_article").is_none());
        assert!(value.get("business_payload").is_none());
    }
}

use std::path::Path;

use crate::database;
use crate::domain::{
    EntityFileRecord, StageRunOutputArtifact, StageRunOutputsPayload, StorageProvider,
};
use crate::services::runtime::load_workspace_context;

pub(crate) fn list_stage_run_outputs_for_workspace(
    workspace_id: &str,
    run_id: &str,
) -> Result<StageRunOutputsPayload, String> {
    let context = load_workspace_context(workspace_id)?;
    list_stage_run_outputs(&context.database_path, run_id)
}

pub(crate) fn list_stage_run_outputs(
    database_path: &Path,
    run_id: &str,
) -> Result<StageRunOutputsPayload, String> {
    let normalized_run_id = run_id.trim();
    if normalized_run_id.is_empty() {
        return Err("run_id is required.".to_string());
    }

    let files = database::list_entity_files(database_path, None)?
        .into_iter()
        .filter(|file| file.producer_run_id.as_deref() == Some(normalized_run_id))
        .collect::<Vec<_>>();
    let mut outputs = Vec::new();
    for file in files {
        outputs.push(output_artifact(database_path, normalized_run_id, file)?);
    }
    outputs.sort_by(|left, right| {
        left.target_stage_id
            .cmp(&right.target_stage_id)
            .then_with(|| left.entity_id.cmp(&right.entity_id))
            .then_with(|| left.entity_file_id.cmp(&right.entity_file_id))
    });
    Ok(StageRunOutputsPayload {
        run_id: normalized_run_id.to_string(),
        output_count: outputs.len() as u64,
        outputs,
    })
}

fn output_artifact(
    database_path: &Path,
    run_id: &str,
    file: EntityFileRecord,
) -> Result<StageRunOutputArtifact, String> {
    let runtime_status =
        database::get_stage_state_status(database_path, &file.entity_id, &file.stage_id)?;
    let s3_uri = match (
        &file.storage_provider,
        file.bucket.as_deref(),
        file.key.as_deref(),
    ) {
        (StorageProvider::S3, Some(bucket), Some(key)) => Some(format!("s3://{bucket}/{key}")),
        _ => None,
    };

    Ok(StageRunOutputArtifact {
        entity_file_id: file.id,
        entity_id: file.entity_id,
        artifact_id: file.artifact_id,
        target_stage_id: file.stage_id,
        relation_to_source: file.relation_to_source,
        storage_provider: file.storage_provider,
        bucket: file.bucket,
        key: file.key,
        s3_uri,
        version_id: file.version_id,
        etag: file.etag,
        checksum_sha256: file.checksum_sha256,
        size: file.artifact_size,
        runtime_status,
        producer_run_id: run_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{
        bootstrap_database, register_s3_artifact_pointers, RegisterS3ArtifactPointerInput,
    };
    use crate::domain::{
        PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition, StageStatus,
    };

    fn test_config() -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            storage: None,
            runtime: RuntimeConfig::default(),
            stages: vec![StageDefinition {
                id: "processed".to_string(),
                input_folder: "stages/processed".to_string(),
                input_uri: Some("s3://bucket/prefix/processed".to_string()),
                output_folder: String::new(),
                workflow_url: "https://n8n.example/webhook/processed".to_string(),
                max_attempts: 3,
                retry_delay_sec: 30,
                next_stage: None,
                save_path_aliases: vec!["prefix/processed".to_string()],
                allow_empty_outputs: false,
            }],
        }
    }

    #[test]
    fn stage_run_outputs_returns_all_children_for_producer_run() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        bootstrap_database(&database_path, &test_config()).expect("bootstrap");
        let pointers = vec![
            RegisterS3ArtifactPointerInput {
                entity_id: "child-1".to_string(),
                artifact_id: "artifact-1".to_string(),
                relation_to_source: Some("child_entity".to_string()),
                stage_id: "processed".to_string(),
                bucket: "bucket".to_string(),
                key: "prefix/processed/child-1.json".to_string(),
                version_id: None,
                etag: None,
                checksum_sha256: None,
                size: Some(10),
                last_modified: None,
                source_file_id: None,
                producer_run_id: Some("run-123".to_string()),
                status: StageStatus::Pending,
            },
            RegisterS3ArtifactPointerInput {
                entity_id: "child-2".to_string(),
                artifact_id: "artifact-2".to_string(),
                relation_to_source: Some("child_entity".to_string()),
                stage_id: "processed".to_string(),
                bucket: "bucket".to_string(),
                key: "prefix/processed/child-2.json".to_string(),
                version_id: None,
                etag: None,
                checksum_sha256: None,
                size: Some(11),
                last_modified: None,
                source_file_id: None,
                producer_run_id: Some("run-123".to_string()),
                status: StageStatus::Pending,
            },
        ];
        register_s3_artifact_pointers(&database_path, &pointers).expect("register outputs");

        let payload = list_stage_run_outputs(&database_path, "run-123").expect("outputs");
        assert_eq!(payload.output_count, 2);
        assert_eq!(
            payload
                .outputs
                .iter()
                .map(|output| output.s3_uri.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("s3://bucket/prefix/processed/child-1.json"),
                Some("s3://bucket/prefix/processed/child-2.json"),
            ]
        );
        assert!(payload
            .outputs
            .iter()
            .all(|output| output.runtime_status.as_deref() == Some("pending")));
    }
}

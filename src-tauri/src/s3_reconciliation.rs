use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::database::{self, RegisterS3ArtifactPointerInput};
use crate::domain::{
    AppEventLevel, EntityFileRecord, PipelineConfig, RegisterS3SourceArtifactRequest,
    S3ReconciliationSummary, StageRecord, StageStatus, StorageProvider,
};
use crate::s3_client::{AwsS3MetadataClient, S3MetadataClient, S3ObjectMetadata};
use crate::save_path::parse_s3_uri;

pub(crate) fn reconcile_s3_workspace(
    database_path: &Path,
    config: &PipelineConfig,
) -> Result<S3ReconciliationSummary, String> {
    let storage = config
        .storage
        .as_ref()
        .and_then(|storage| storage.s3_config())
        .ok_or_else(|| {
            "S3 reconciliation requires pipeline storage.provider=s3 with bucket and workspace_prefix."
                .to_string()
        })?;
    let client = AwsS3MetadataClient::from_storage_config(&storage)?;
    reconcile_s3_workspace_with_client(database_path, config, &client)
}

pub(crate) fn reconcile_s3_workspace_with_client(
    database_path: &Path,
    config: &PipelineConfig,
    client: &dyn S3MetadataClient,
) -> Result<S3ReconciliationSummary, String> {
    let storage = config
        .storage
        .as_ref()
        .and_then(|storage| storage.s3_config())
        .ok_or_else(|| {
            "S3 reconciliation requires pipeline storage.provider=s3 with bucket and workspace_prefix."
                .to_string()
        })?;
    let started = Instant::now();
    let scan_id = Uuid::new_v4().to_string();
    let reconciled_at = Utc::now().to_rfc3339();
    let connection = database::open_connection(database_path)?;
    let active_stages = database::load_active_stages_from_connection(&connection)?;
    drop(connection);

    let mut targets = Vec::new();
    for stage in active_stages.into_iter().filter(|stage| {
        stage
            .input_uri
            .as_deref()
            .is_some_and(|uri| uri.starts_with("s3://"))
    }) {
        let input_uri = stage
            .input_uri
            .as_deref()
            .ok_or_else(|| format!("S3 stage '{}' has no input_uri.", stage.id))?;
        let (bucket, prefix) = parse_s3_uri(input_uri)
            .map_err(|error| format!("Invalid S3 input_uri: {}", error.message))?;
        if bucket != storage.bucket {
            return Err(format!(
                "S3 stage '{}' uses bucket '{}', but storage.bucket is '{}'.",
                stage.id, bucket, storage.bucket
            ));
        }
        targets.push(S3StageTarget {
            stage,
            bucket,
            prefix,
        });
    }

    let mut summary = S3ReconciliationSummary {
        scan_id: scan_id.clone(),
        stage_count: targets.len() as u64,
        listed_object_count: 0,
        metadata_tagged_count: 0,
        registered_file_count: 0,
        updated_file_count: 0,
        unchanged_file_count: 0,
        missing_file_count: 0,
        restored_file_count: 0,
        unmapped_object_count: 0,
        elapsed_ms: 0,
        latest_reconciliation_at: reconciled_at.clone(),
    };
    let mut seen_paths = HashSet::<String>::new();
    let active_stage_ids = targets
        .iter()
        .map(|target| target.stage.id.clone())
        .collect::<HashSet<_>>();
    let mut artifact_seen_this_scan = HashMap::<(String, String), String>::new();

    for target in &targets {
        let listed = client.list_objects(&target.bucket, &target.prefix)?;
        summary.listed_object_count += listed.len() as u64;
        for listed_object in listed {
            if !key_is_inside_prefix(&listed_object.key, &target.prefix) {
                continue;
            }
            let metadata_object = client
                .head_object(&target.bucket, &listed_object.key)?
                .unwrap_or(listed_object);
            let file_path = s3_file_path(&target.bucket, &metadata_object.key);
            seen_paths.insert(file_path.clone());

            let Some(entity_id) = metadata_value(&metadata_object.metadata, "beehive-entity-id")
            else {
                summary.unmapped_object_count += 1;
                record_unmapped_event(
                    database_path,
                    &scan_id,
                    target,
                    &metadata_object,
                    "missing_beehive_entity_id",
                    "S3 object has no beehive entity metadata.",
                    &reconciled_at,
                )?;
                continue;
            };
            let Some(artifact_id) =
                metadata_value(&metadata_object.metadata, "beehive-artifact-id")
            else {
                summary.unmapped_object_count += 1;
                record_unmapped_event(
                    database_path,
                    &scan_id,
                    target,
                    &metadata_object,
                    "missing_beehive_artifact_id",
                    "S3 object has no beehive artifact metadata.",
                    &reconciled_at,
                )?;
                continue;
            };
            if let Some(metadata_stage_id) =
                metadata_value(&metadata_object.metadata, "beehive-stage-id")
            {
                if metadata_stage_id != target.stage.id {
                    summary.unmapped_object_count += 1;
                    record_unmapped_event(
                        database_path,
                        &scan_id,
                        target,
                        &metadata_object,
                        "metadata_stage_mismatch",
                        "S3 object stage metadata does not match the scanned stage.",
                        &reconciled_at,
                    )?;
                    continue;
                }
            }

            let artifact_key = (target.stage.id.clone(), artifact_id.clone());
            if let Some(previous_path) = artifact_seen_this_scan.get(&artifact_key) {
                if previous_path != &file_path {
                    summary.unmapped_object_count += 1;
                    record_unmapped_event(
                        database_path,
                        &scan_id,
                        target,
                        &metadata_object,
                        "duplicate_artifact_id_in_scan",
                        "S3 artifact_id appears on more than one key in this scan.",
                        &reconciled_at,
                    )?;
                    continue;
                }
            } else {
                artifact_seen_this_scan.insert(artifact_key, file_path.clone());
            }

            if let Some(conflict) = find_manual_artifact_conflict(
                database_path,
                &target.stage.id,
                &entity_id,
                &artifact_id,
                &target.bucket,
                &metadata_object.key,
            )? {
                summary.unmapped_object_count += 1;
                record_unmapped_event(
                    database_path,
                    &scan_id,
                    target,
                    &metadata_object,
                    "artifact_id_location_conflict",
                    &conflict,
                    &reconciled_at,
                )?;
                continue;
            }

            summary.metadata_tagged_count += 1;
            let registration = RegisterS3ArtifactPointerInput {
                entity_id,
                artifact_id,
                relation_to_source: None,
                stage_id: target.stage.id.clone(),
                bucket: target.bucket.clone(),
                key: metadata_object.key.clone(),
                version_id: metadata_object.version_id.clone(),
                etag: metadata_object.etag.clone(),
                checksum_sha256: metadata_object.checksum_sha256.clone(),
                size: metadata_object.size,
                last_modified: metadata_object.last_modified.clone(),
                source_file_id: None,
                producer_run_id: None,
                status: StageStatus::Pending,
            };
            let outcome = classify_registration_outcome(database_path, &registration)?;
            database::register_s3_artifact_pointers(
                database_path,
                std::slice::from_ref(&registration),
            )?;
            match outcome {
                ReconciledFileOutcome::Inserted => {
                    summary.registered_file_count += 1;
                    record_registration_event(
                        database_path,
                        AppEventLevel::Info,
                        "s3_artifact_discovered",
                        "S3 artifact was discovered and registered.",
                        &scan_id,
                        &registration,
                        &reconciled_at,
                    )?;
                }
                ReconciledFileOutcome::Updated => {
                    summary.updated_file_count += 1;
                    record_registration_event(
                        database_path,
                        AppEventLevel::Info,
                        "s3_artifact_updated",
                        "S3 artifact pointer metadata was updated.",
                        &scan_id,
                        &registration,
                        &reconciled_at,
                    )?;
                }
                ReconciledFileOutcome::Unchanged => summary.unchanged_file_count += 1,
                ReconciledFileOutcome::Restored => {
                    summary.restored_file_count += 1;
                    record_registration_event(
                        database_path,
                        AppEventLevel::Info,
                        "s3_artifact_restored",
                        "Previously missing S3 artifact was found again.",
                        &scan_id,
                        &registration,
                        &reconciled_at,
                    )?;
                }
            }
        }
    }

    summary.missing_file_count = database::mark_missing_s3_files_for_active_stages(
        database_path,
        &active_stage_ids,
        &seen_paths,
        &scan_id,
        &reconciled_at,
    )?;
    summary.elapsed_ms = started.elapsed().as_millis();
    write_reconciliation_settings(database_path, &summary)?;
    record_reconciliation_completed(database_path, &summary)?;

    Ok(summary)
}

pub(crate) fn register_s3_source_artifact(
    database_path: &Path,
    input: &RegisterS3SourceArtifactRequest,
) -> Result<EntityFileRecord, String> {
    let connection = database::open_connection(database_path)?;
    let stage = database::find_stage_by_id(&connection, &input.stage_id)?
        .ok_or_else(|| format!("Target stage '{}' was not found.", input.stage_id))?;
    drop(connection);
    validate_stage_accepts_s3_source(&stage, input)?;

    if let Some(conflict) = find_manual_artifact_conflict(
        database_path,
        &input.stage_id,
        &input.entity_id,
        &input.artifact_id,
        &input.bucket,
        &input.key,
    )? {
        return Err(conflict);
    }

    let registration = RegisterS3ArtifactPointerInput {
        entity_id: input.entity_id.clone(),
        artifact_id: input.artifact_id.clone(),
        relation_to_source: None,
        stage_id: input.stage_id.clone(),
        bucket: input.bucket.clone(),
        key: input.key.clone(),
        version_id: input.version_id.clone(),
        etag: input.etag.clone(),
        checksum_sha256: input.checksum_sha256.clone(),
        size: input.size,
        last_modified: None,
        source_file_id: None,
        producer_run_id: None,
        status: StageStatus::Pending,
    };
    let mut files = database::register_s3_artifact_pointers(
        database_path,
        std::slice::from_ref(&registration),
    )?;
    let file = files
        .pop()
        .ok_or_else(|| "S3 source artifact registration returned no rows.".to_string())?;
    let now = Utc::now().to_rfc3339();
    record_registration_event(
        database_path,
        AppEventLevel::Info,
        "s3_source_artifact_registered",
        "S3 source artifact was manually registered.",
        &Uuid::new_v4().to_string(),
        &registration,
        &now,
    )?;
    Ok(file)
}

#[derive(Debug)]
struct S3StageTarget {
    stage: StageRecord,
    bucket: String,
    prefix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconciledFileOutcome {
    Inserted,
    Updated,
    Unchanged,
    Restored,
}

fn classify_registration_outcome(
    database_path: &Path,
    registration: &RegisterS3ArtifactPointerInput,
) -> Result<ReconciledFileOutcome, String> {
    let connection = database::open_connection(database_path)?;
    let file_path = s3_file_path(&registration.bucket, &registration.key);
    let Some(existing) = database::find_entity_file_by_path(&connection, &file_path)? else {
        return Ok(ReconciledFileOutcome::Inserted);
    };
    if !existing.file_exists {
        return Ok(ReconciledFileOutcome::Restored);
    }
    if existing_matches_registration(&existing, registration) {
        Ok(ReconciledFileOutcome::Unchanged)
    } else {
        Ok(ReconciledFileOutcome::Updated)
    }
}

fn existing_matches_registration(
    existing: &EntityFileRecord,
    registration: &RegisterS3ArtifactPointerInput,
) -> bool {
    existing.entity_id == registration.entity_id
        && existing.stage_id == registration.stage_id
        && existing.artifact_id.as_deref() == Some(registration.artifact_id.as_str())
        && existing.relation_to_source == registration.relation_to_source
        && existing.storage_provider == StorageProvider::S3
        && existing.bucket.as_deref() == Some(registration.bucket.as_str())
        && existing.key.as_deref() == Some(registration.key.as_str())
        && existing.version_id == registration.version_id
        && existing.etag == registration.etag
        && existing.checksum_sha256 == registration.checksum_sha256
        && existing.artifact_size == registration.size
        && existing.producer_run_id == registration.producer_run_id
        && registration
            .last_modified
            .as_ref()
            .map_or(true, |last_modified| {
                existing.file_mtime == last_modified.as_str()
            })
}

fn validate_stage_accepts_s3_source(
    stage: &StageRecord,
    input: &RegisterS3SourceArtifactRequest,
) -> Result<(), String> {
    if !stage.is_active {
        return Err(format!("Target stage '{}' is inactive.", stage.id));
    }
    let input_uri = stage.input_uri.as_deref().ok_or_else(|| {
        format!(
            "Target stage '{}' is not configured with an S3 input_uri.",
            stage.id
        )
    })?;
    let (bucket, prefix) = parse_s3_uri(input_uri)
        .map_err(|error| format!("Invalid S3 input_uri: {}", error.message))?;
    if bucket != input.bucket {
        return Err(format!(
            "S3 source bucket '{}' does not match stage '{}' input bucket '{}'.",
            input.bucket, stage.id, bucket
        ));
    }
    if !key_is_inside_prefix(&input.key, &prefix) {
        return Err(format!(
            "S3 source key '{}' is outside stage '{}' prefix '{}'.",
            input.key, stage.id, prefix
        ));
    }
    Ok(())
}

fn find_manual_artifact_conflict(
    database_path: &Path,
    stage_id: &str,
    entity_id: &str,
    artifact_id: &str,
    bucket: &str,
    key: &str,
) -> Result<Option<String>, String> {
    let files = database::list_entity_files(database_path, None)?;
    let requested_path = s3_file_path(bucket, key);
    for file in files {
        if file.stage_id != stage_id {
            continue;
        }
        if file.storage_provider != StorageProvider::S3 {
            continue;
        }
        if file.artifact_id.as_deref() != Some(artifact_id) {
            continue;
        }
        if file.file_path == requested_path && file.entity_id == entity_id {
            continue;
        }
        return Ok(Some(format!(
            "S3 artifact_id '{}' on stage '{}' is already registered for entity '{}' at '{}'.",
            artifact_id, stage_id, file.entity_id, file.file_path
        )));
    }
    Ok(None)
}

fn metadata_value(metadata: &HashMap<String, String>, name: &str) -> Option<String> {
    let normalized = metadata
        .iter()
        .map(|(key, value)| (key.to_ascii_lowercase(), value.trim()))
        .collect::<HashMap<_, _>>();
    let underscore_name = name.replace('-', "_");
    [
        name.to_string(),
        format!("x-amz-meta-{name}"),
        underscore_name.clone(),
        format!("x-amz-meta-{underscore_name}"),
    ]
    .iter()
    .find_map(|candidate| {
        normalized
            .get(candidate)
            .copied()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn key_is_inside_prefix(key: &str, prefix: &str) -> bool {
    let normalized_prefix = prefix.trim_end_matches('/');
    key == normalized_prefix || key.starts_with(&format!("{normalized_prefix}/"))
}

fn s3_file_path(bucket: &str, key: &str) -> String {
    format!("s3://{bucket}/{key}")
}

fn record_unmapped_event(
    database_path: &Path,
    scan_id: &str,
    target: &S3StageTarget,
    object: &S3ObjectMetadata,
    reason: &str,
    message: &str,
    created_at: &str,
) -> Result<(), String> {
    let connection = database::open_connection(database_path)?;
    database::insert_app_event(
        &connection,
        AppEventLevel::Warning,
        "s3_artifact_unmapped",
        message,
        Some(json!({
            "scan_id": scan_id,
            "reason": reason,
            "stage_id": target.stage.id,
            "bucket": object.bucket,
            "key": object.key,
            "etag": object.etag,
            "size": object.size,
        })),
        created_at,
    )
}

fn record_registration_event(
    database_path: &Path,
    level: AppEventLevel,
    code: &str,
    message: &str,
    scan_id: &str,
    input: &RegisterS3ArtifactPointerInput,
    created_at: &str,
) -> Result<(), String> {
    let connection = database::open_connection(database_path)?;
    database::insert_app_event(
        &connection,
        level,
        code,
        message,
        Some(json!({
            "scan_id": scan_id,
            "entity_id": input.entity_id,
            "stage_id": input.stage_id,
            "artifact_id": input.artifact_id,
            "bucket": input.bucket,
            "key": input.key,
            "version_id": input.version_id,
            "etag": input.etag,
            "checksum_sha256": input.checksum_sha256,
            "size": input.size,
        })),
        created_at,
    )
}

fn record_reconciliation_completed(
    database_path: &Path,
    summary: &S3ReconciliationSummary,
) -> Result<(), String> {
    let connection = database::open_connection(database_path)?;
    database::insert_app_event(
        &connection,
        AppEventLevel::Info,
        "s3_reconciliation_completed",
        "S3 reconciliation completed.",
        Some(json!({
            "scan_id": summary.scan_id,
            "stage_count": summary.stage_count,
            "listed_object_count": summary.listed_object_count,
            "metadata_tagged_count": summary.metadata_tagged_count,
            "registered_file_count": summary.registered_file_count,
            "updated_file_count": summary.updated_file_count,
            "unchanged_file_count": summary.unchanged_file_count,
            "missing_file_count": summary.missing_file_count,
            "restored_file_count": summary.restored_file_count,
            "unmapped_object_count": summary.unmapped_object_count,
            "elapsed_ms": summary.elapsed_ms,
        })),
        &summary.latest_reconciliation_at,
    )
}

fn write_reconciliation_settings(
    database_path: &Path,
    summary: &S3ReconciliationSummary,
) -> Result<(), String> {
    let connection = database::open_connection(database_path)?;
    database::set_setting(
        &connection,
        "last_s3_reconciliation_id",
        &summary.scan_id,
        &summary.latest_reconciliation_at,
    )?;
    database::set_setting(
        &connection,
        "last_s3_reconciliation_at",
        &summary.latest_reconciliation_at,
        &summary.latest_reconciliation_at,
    )?;
    database::set_setting(
        &connection,
        "last_scan_id",
        &summary.scan_id,
        &summary.latest_reconciliation_at,
    )?;
    database::set_setting(
        &connection,
        "last_scan_completed_at",
        &summary.latest_reconciliation_at,
        &summary.latest_reconciliation_at,
    )?;
    database::set_setting(
        &connection,
        "last_scan_invalid_count",
        &summary.unmapped_object_count.to_string(),
        &summary.latest_reconciliation_at,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{bootstrap_database, list_app_events, list_entity_files};
    use crate::domain::{ProjectConfig, RuntimeConfig, StageDefinition, StorageConfig};
    use crate::executor;
    use crate::s3_client::AwsS3MetadataClient;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    struct MockS3Client {
        lists: HashMap<(String, String), Vec<S3ObjectMetadata>>,
        heads: HashMap<(String, String), Option<S3ObjectMetadata>>,
    }

    impl MockS3Client {
        fn with_object(mut self, bucket: &str, prefix: &str, object: S3ObjectMetadata) -> Self {
            self.lists
                .entry((bucket.to_string(), prefix.to_string()))
                .or_default()
                .push(S3ObjectMetadata {
                    metadata: HashMap::new(),
                    ..object.clone()
                });
            self.heads
                .insert((bucket.to_string(), object.key.clone()), Some(object));
            self
        }
    }

    impl S3MetadataClient for MockS3Client {
        fn list_objects(
            &self,
            bucket: &str,
            prefix: &str,
        ) -> Result<Vec<S3ObjectMetadata>, String> {
            Ok(self
                .lists
                .get(&(bucket.to_string(), prefix.to_string()))
                .cloned()
                .unwrap_or_default())
        }

        fn head_object(&self, bucket: &str, key: &str) -> Result<Option<S3ObjectMetadata>, String> {
            Ok(self
                .heads
                .get(&(bucket.to_string(), key.to_string()))
                .cloned()
                .unwrap_or(None))
        }
    }

    fn test_config() -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            storage: Some(StorageConfig {
                provider: StorageProvider::S3,
                bucket: Some("steos-s3-data".to_string()),
                workspace_prefix: Some("main_dir".to_string()),
                region: None,
                endpoint: None,
            }),
            runtime: RuntimeConfig::default(),
            stages: vec![s3_stage("raw", "s3://steos-s3-data/main_dir/raw")],
        }
    }

    fn s3_stage(id: &str, input_uri: &str) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: String::new(),
            input_uri: Some(input_uri.to_string()),
            output_folder: String::new(),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: None,
            save_path_aliases: Vec::new(),
            allow_empty_outputs: false,
        }
    }

    fn object(key: &str, entity_id: Option<&str>, artifact_id: Option<&str>) -> S3ObjectMetadata {
        let mut metadata = HashMap::new();
        if let Some(entity_id) = entity_id {
            metadata.insert("beehive-entity-id".to_string(), entity_id.to_string());
        }
        if let Some(artifact_id) = artifact_id {
            metadata.insert("beehive-artifact-id".to_string(), artifact_id.to_string());
        }
        S3ObjectMetadata {
            bucket: "steos-s3-data".to_string(),
            key: key.to_string(),
            version_id: None,
            etag: Some(format!("etag-{key}")),
            checksum_sha256: None,
            size: Some(42),
            last_modified: Some("2026-05-13T00:00:00Z".to_string()),
            metadata,
        }
    }

    #[test]
    fn reconciliation_registers_metadata_tagged_objects() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config();
        bootstrap_database(&database_path, &config).expect("bootstrap");
        let client = MockS3Client::default().with_object(
            "steos-s3-data",
            "main_dir/raw",
            object(
                "main_dir/raw/entity-1.json",
                Some("entity-1"),
                Some("artifact-1"),
            ),
        );

        let summary =
            reconcile_s3_workspace_with_client(&database_path, &config, &client).expect("scan");
        let files = list_entity_files(&database_path, None).expect("files");
        let events = list_app_events(&database_path, 20).expect("events");

        assert_eq!(summary.listed_object_count, 1);
        assert_eq!(summary.metadata_tagged_count, 1);
        assert_eq!(summary.registered_file_count, 1);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].entity_id, "entity-1");
        assert_eq!(files[0].artifact_id.as_deref(), Some("artifact-1"));
        assert_eq!(files[0].storage_provider, StorageProvider::S3);
        assert!(events
            .iter()
            .any(|event| event.code == "s3_artifact_discovered"));
    }

    #[test]
    fn reconciliation_records_unmapped_objects_without_registration() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config();
        bootstrap_database(&database_path, &config).expect("bootstrap");
        let client = MockS3Client::default().with_object(
            "steos-s3-data",
            "main_dir/raw",
            object("main_dir/raw/unmapped.json", None, Some("artifact-1")),
        );

        let summary =
            reconcile_s3_workspace_with_client(&database_path, &config, &client).expect("scan");
        let files = list_entity_files(&database_path, None).expect("files");
        let events = list_app_events(&database_path, 20).expect("events");

        assert_eq!(summary.unmapped_object_count, 1);
        assert!(files.is_empty());
        assert!(events
            .iter()
            .any(|event| event.code == "s3_artifact_unmapped"));
    }

    #[test]
    fn reconciliation_marks_missing_and_restored_s3_artifacts() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config();
        bootstrap_database(&database_path, &config).expect("bootstrap");
        let object = object(
            "main_dir/raw/entity-1.json",
            Some("entity-1"),
            Some("artifact-1"),
        );
        let present_client =
            MockS3Client::default().with_object("steos-s3-data", "main_dir/raw", object.clone());
        reconcile_s3_workspace_with_client(&database_path, &config, &present_client)
            .expect("first scan");

        let missing_client = MockS3Client::default();
        let missing = reconcile_s3_workspace_with_client(&database_path, &config, &missing_client)
            .expect("missing scan");
        let missing_files = list_entity_files(&database_path, None).expect("missing files");

        assert_eq!(missing.missing_file_count, 1);
        assert!(!missing_files[0].file_exists);

        let restored_client =
            MockS3Client::default().with_object("steos-s3-data", "main_dir/raw", object);
        let restored =
            reconcile_s3_workspace_with_client(&database_path, &config, &restored_client)
                .expect("restored scan");
        let restored_files = list_entity_files(&database_path, None).expect("restored files");

        assert_eq!(restored.restored_file_count, 1);
        assert!(restored_files[0].file_exists);
    }

    #[test]
    fn manual_s3_source_registration_validates_stage_prefix_and_conflicts() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config();
        bootstrap_database(&database_path, &config).expect("bootstrap");
        let input = RegisterS3SourceArtifactRequest {
            stage_id: "raw".to_string(),
            entity_id: "entity-1".to_string(),
            artifact_id: "artifact-1".to_string(),
            bucket: "steos-s3-data".to_string(),
            key: "main_dir/raw/entity-1.json".to_string(),
            version_id: None,
            etag: Some("etag-source".to_string()),
            checksum_sha256: None,
            size: Some(42),
        };

        let file = register_s3_source_artifact(&database_path, &input).expect("register");
        let replay = register_s3_source_artifact(&database_path, &input).expect("replay");

        assert_eq!(file.id, replay.id);
        assert_eq!(file.artifact_id.as_deref(), Some("artifact-1"));

        let outside_prefix = RegisterS3SourceArtifactRequest {
            key: "main_dir/other/entity-1.json".to_string(),
            ..input.clone()
        };
        let prefix_error =
            register_s3_source_artifact(&database_path, &outside_prefix).expect_err("prefix");
        assert!(prefix_error.contains("outside stage"));

        let conflicting = RegisterS3SourceArtifactRequest {
            entity_id: "entity-2".to_string(),
            key: "main_dir/raw/entity-2.json".to_string(),
            ..input
        };
        let conflict_error =
            register_s3_source_artifact(&database_path, &conflicting).expect_err("conflict");
        assert!(conflict_error.contains("already registered"));
    }

    #[test]
    #[ignore = "real S3+n8n smoke; requires .env, uploaded smoke objects, and imported n8n workflow"]
    fn real_s3_n8n_smoke_one_artifact() {
        let report = run_real_s3_n8n_smoke(
            "/tmp/beehive_s3_smoke_workdir",
            1,
            false,
            "beehive.s3_single_smoke_report.v1",
        )
        .expect("real S3+n8n smoke");
        let run = report.runs.first().expect("single smoke run evidence");

        assert_eq!(
            report.claimed, 1,
            "expected exactly one claimed smoke source"
        );
        assert_eq!(
            report.succeeded, 1,
            "expected one successful real n8n smoke run"
        );
        assert_eq!(run.source_status, "done");
        assert_eq!(run.child_status, "pending");
        assert!(run.s3_output_exists);

        println!("B2_1_SOURCE_KEY={}", run.source_key);
        println!("B2_1_RUN_ID={}", run.run_id);
        println!("B2_1_STAGE_RUN_SUCCESS=true");
        println!("B2_1_OUTPUT_ARTIFACT_ID={}", run.output_artifact_id);
        println!("B2_1_OUTPUT_KEY={}", run.output_key);
        println!("B2_1_SOURCE_STATE={}", run.source_status);
        println!("B2_1_CHILD_STATE={}", run.child_status);
        println!("B2_1_S3_OUTPUT_EXISTS=true size={:?}", run.s3_output_size);
        println!("B2_1_REPORT={}", report.report_path.display());
    }

    #[test]
    #[ignore = "real S3+n8n batch smoke; requires .env, uploaded smoke objects, and imported BODY_JSON n8n workflow"]
    fn real_s3_n8n_smoke_batch_small() {
        require_real_batch_smoke_opt_in();
        let limit = batch_smoke_limit();
        let report = run_real_s3_n8n_smoke(
            "/tmp/beehive_s3_batch_smoke_workdir",
            limit,
            true,
            "beehive.s3_batch_smoke_report.v1",
        )
        .expect("real S3+n8n batch smoke");

        assert_eq!(report.claimed, limit, "expected claimed batch size");
        assert_eq!(report.succeeded, limit, "expected successful batch size");
        assert_eq!(report.runs.len() as u64, limit);
        for run in &report.runs {
            assert_eq!(run.source_status, "done");
            assert_eq!(run.child_status, "pending");
            assert!(run.s3_output_exists);
        }

        println!("B3_BATCH_SMOKE_LIMIT={limit}");
        println!("B3_BATCH_SMOKE_CLAIMED={}", report.claimed);
        println!("B3_BATCH_SMOKE_SUCCEEDED={}", report.succeeded);
        println!("B3_BATCH_SMOKE_REPORT={}", report.report_path.display());
        for run in &report.runs {
            println!(
                "B3_BATCH_SMOKE_RUN run_id={} source_key={} output_key={} source_state={} child_state={} s3_output_exists={} size={:?}",
                run.run_id,
                run.source_key,
                run.output_key,
                run.source_status,
                run.child_status,
                run.s3_output_exists,
                run.s3_output_size
            );
        }
    }

    struct RealSmokeReport {
        report_path: PathBuf,
        claimed: u64,
        succeeded: u64,
        runs: Vec<RealSmokeRunEvidence>,
    }

    struct RealSmokeRunEvidence {
        run_id: String,
        source_key: String,
        output_artifact_id: String,
        output_key: String,
        source_status: String,
        child_status: String,
        s3_output_exists: bool,
        s3_output_size: Option<u64>,
    }

    fn batch_smoke_limit() -> u64 {
        env::var("BEEHIVE_SMOKE_BATCH_LIMIT")
            .or_else(|_| env::var("BEEHIVE_BATCH_SMOKE_LIMIT"))
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3)
            .clamp(3, 5)
    }

    fn require_real_batch_smoke_opt_in() {
        assert_eq!(
            env::var("BEEHIVE_REAL_S3_BATCH_SMOKE").as_deref(),
            Ok("1"),
            "set BEEHIVE_REAL_S3_BATCH_SMOKE=1 to run the real S3+n8n batch smoke"
        );
    }

    fn run_real_s3_n8n_smoke(
        workdir_path: &str,
        limit: u64,
        reset_database: bool,
        report_schema: &str,
    ) -> Result<RealSmokeReport, String> {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| "missing repo root".to_string())?
            .to_path_buf();
        let env_values = load_smoke_env(&repo_root.join(".env"));
        let s3_host = required_smoke_env(&env_values, "S3_HOST");
        let s3_region = required_smoke_env(&env_values, "S3_REGION");
        let s3_key = required_smoke_env(&env_values, "S3_KEY");
        let s3_sec_key = required_smoke_env(&env_values, "S3_SEC_KEY");
        let bucket = required_smoke_env(&env_values, "S3_BUCKET_NAME");
        let prefix = required_smoke_env(&env_values, "BEEHIVE_SMOKE_PREFIX");
        let webhook = required_smoke_env(&env_values, "BEEHIVE_N8N_SMOKE_WEBHOOK");
        let endpoint = format!("https://{}", s3_host.trim_start_matches("https://"));

        env::set_var("S3_HOST", &s3_host);
        env::set_var("S3_REGION", &s3_region);
        env::set_var("S3_KEY", &s3_key);
        env::set_var("S3_SEC_KEY", &s3_sec_key);
        env::set_var("BEEHIVE_S3_ENDPOINT", &endpoint);
        env::set_var("BEEHIVE_S3_REGION", &s3_region);

        let workdir = PathBuf::from(workdir_path);
        fs::create_dir_all(&workdir).map_err(|error| format!("create smoke workdir: {error}"))?;
        let database_path = workdir.join("app.db");
        if reset_database && database_path.exists() {
            fs::remove_file(&database_path).map_err(|error| {
                format!(
                    "reset smoke database '{}': {error}",
                    database_path.display()
                )
            })?;
        }
        let config = real_smoke_config(&bucket, &prefix, &s3_region, &endpoint, &webhook);
        let pipeline_yaml = serde_yaml::to_string(&config)
            .map_err(|error| format!("serialize smoke pipeline: {error}"))?;
        fs::write(workdir.join("pipeline.yaml"), pipeline_yaml)
            .map_err(|error| format!("write smoke pipeline: {error}"))?;

        bootstrap_database(&database_path, &config)
            .map_err(|error| format!("bootstrap smoke database: {error}"))?;
        let reconciliation = reconcile_s3_workspace(&database_path, &config)
            .map_err(|error| format!("real S3 reconciliation: {error}"))?;
        println!(
            "B2_RECONCILE listed={} tagged={} registered={} updated={} unchanged={} unmapped={} missing={} restored={}",
            reconciliation.listed_object_count,
            reconciliation.metadata_tagged_count,
            reconciliation.registered_file_count,
            reconciliation.updated_file_count,
            reconciliation.unchanged_file_count,
            reconciliation.unmapped_object_count,
            reconciliation.missing_file_count,
            reconciliation.restored_file_count
        );

        let available_sources =
            reconciliation.metadata_tagged_count + reconciliation.unchanged_file_count;
        if available_sources < limit {
            return Err(format!(
                "expected at least {limit} metadata-tagged or already registered source objects, got {available_sources}"
            ));
        }

        let run_summary = executor::run_due_tasks(&workdir, &database_path, limit, 300, 300, 0)
            .map_err(|error| format!("run due tasks: {error}"))?;
        println!(
            "B2_RUN_SUMMARY claimed={} succeeded={} failed={} blocked={} retry_scheduled={} skipped={}",
            run_summary.claimed,
            run_summary.succeeded,
            run_summary.failed,
            run_summary.blocked,
            run_summary.retry_scheduled,
            run_summary.skipped
        );
        if run_summary.succeeded != limit {
            if let Ok(latest_run) = latest_stage_run(&database_path) {
                println!("B2_LATEST_RUN_ID={}", latest_run["run_id"]);
                println!("B2_LATEST_RUN_SOURCE_KEY={}", latest_run["source_key"]);
                println!("B2_LATEST_RUN_SUCCESS={}", latest_run["success"]);
                println!("B2_LATEST_RUN_HTTP_STATUS={}", latest_run["http_status"]);
                println!("B2_LATEST_RUN_ERROR_TYPE={}", latest_run["error_type"]);
            }
            return Err(format!(
                "expected {limit} successful real n8n smoke run(s), got {}",
                run_summary.succeeded
            ));
        }

        let storage = config.storage.as_ref().unwrap().s3_config().unwrap();
        let client = AwsS3MetadataClient::from_storage_config(&storage)
            .map_err(|error| format!("S3 client: {error}"))?;
        let runs = latest_successful_stage_runs(&database_path, limit)?;
        let mut evidence = Vec::new();
        for run in runs {
            let run_id = run.get("run_id").expect("run_id").clone();
            let entity_id = run.get("entity_id").expect("entity_id").clone();
            let source_key = run.get("source_key").expect("source_key").clone();
            let output_file = latest_output_file_for_run(&database_path, &run_id)?
                .ok_or_else(|| format!("missing registered output file for run {run_id}"))?;
            let output_key = output_file.key.clone().ok_or_else(|| {
                format!("registered output for run {run_id} is missing S3 object key")
            })?;
            let output_artifact_id = output_file.artifact_id.clone().unwrap_or_default();
            let source_status = stage_state_status(&database_path, &entity_id, "smoke_source")?;
            let child_status =
                stage_state_status(&database_path, &output_file.entity_id, "smoke_processed")?;
            let output_head = client
                .head_object(&bucket, &output_key)
                .map_err(|error| format!("head output object '{output_key}': {error}"))?;
            let s3_output_size = output_head.as_ref().and_then(|object| object.size);
            evidence.push(RealSmokeRunEvidence {
                run_id,
                source_key,
                output_artifact_id,
                output_key,
                source_status,
                child_status,
                s3_output_exists: output_head.is_some(),
                s3_output_size,
            });
        }
        evidence.reverse();

        let report_path = workdir.join(if limit == 1 {
            "single_smoke_report.json"
        } else {
            "batch_smoke_report.json"
        });
        let report_json = serde_json::json!({
            "schema": report_schema,
            "workdir": workdir,
            "bucket": bucket,
            "prefix": prefix,
            "requested_limit": limit,
            "reconciliation": {
                "listed": reconciliation.listed_object_count,
                "tagged": reconciliation.metadata_tagged_count,
                "registered": reconciliation.registered_file_count,
                "updated": reconciliation.updated_file_count,
                "unchanged": reconciliation.unchanged_file_count,
                "unmapped": reconciliation.unmapped_object_count,
                "missing": reconciliation.missing_file_count,
                "restored": reconciliation.restored_file_count
            },
            "run_summary": {
                "claimed": run_summary.claimed,
                "succeeded": run_summary.succeeded,
                "failed": run_summary.failed,
                "blocked": run_summary.blocked,
                "retry_scheduled": run_summary.retry_scheduled,
                "skipped": run_summary.skipped
            },
            "runs": evidence.iter().map(|run| serde_json::json!({
                "run_id": run.run_id,
                "source_key": run.source_key,
                "output_artifact_id": run.output_artifact_id,
                "output_key": run.output_key,
                "source_status": run.source_status,
                "child_status": run.child_status,
                "s3_output_exists": run.s3_output_exists,
                "s3_output_size": run.s3_output_size
            })).collect::<Vec<_>>()
        });
        fs::write(
            &report_path,
            serde_json::to_string_pretty(&report_json)
                .map_err(|error| format!("serialize smoke report: {error}"))?,
        )
        .map_err(|error| format!("write smoke report '{}': {error}", report_path.display()))?;

        Ok(RealSmokeReport {
            report_path,
            claimed: run_summary.claimed,
            succeeded: run_summary.succeeded,
            runs: evidence,
        })
    }

    fn real_smoke_config(
        bucket: &str,
        prefix: &str,
        region: &str,
        endpoint: &str,
        webhook: &str,
    ) -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive-s3-smoke".to_string(),
                workdir: ".".to_string(),
            },
            storage: Some(StorageConfig {
                provider: StorageProvider::S3,
                bucket: Some(bucket.to_string()),
                workspace_prefix: Some(prefix.trim_matches('/').to_string()),
                region: Some(region.to_string()),
                endpoint: Some(endpoint.to_string()),
            }),
            runtime: RuntimeConfig {
                scan_interval_sec: 5,
                max_parallel_tasks: 1,
                stuck_task_timeout_sec: 300,
                request_timeout_sec: 300,
                file_stability_delay_ms: 0,
            },
            stages: vec![
                StageDefinition {
                    id: "smoke_source".to_string(),
                    input_folder: String::new(),
                    input_uri: Some(format!("s3://{}/{}/raw", bucket, prefix.trim_matches('/'))),
                    output_folder: String::new(),
                    workflow_url: webhook.to_string(),
                    max_attempts: 2,
                    retry_delay_sec: 10,
                    next_stage: None,
                    save_path_aliases: vec![
                        format!("{}/raw", prefix.trim_matches('/')),
                        format!("/{}/raw", prefix.trim_matches('/')),
                    ],
                    allow_empty_outputs: false,
                },
                StageDefinition {
                    id: "smoke_processed".to_string(),
                    input_folder: String::new(),
                    input_uri: Some(format!(
                        "s3://{}/{}/processed",
                        bucket,
                        prefix.trim_matches('/')
                    )),
                    output_folder: String::new(),
                    workflow_url: "http://localhost:5678/webhook/not-used-terminal-smoke-target"
                        .to_string(),
                    max_attempts: 1,
                    retry_delay_sec: 0,
                    next_stage: None,
                    save_path_aliases: vec![
                        format!("{}/processed", prefix.trim_matches('/')),
                        format!("/{}/processed", prefix.trim_matches('/')),
                        format!("s3://{}/{}/processed", bucket, prefix.trim_matches('/')),
                    ],
                    allow_empty_outputs: true,
                },
            ],
        }
    }

    fn load_smoke_env(path: &Path) -> HashMap<String, String> {
        let mut values = HashMap::new();
        if let Ok(content) = fs::read_to_string(path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = trimmed.split_once('=') {
                    values.insert(key.trim().to_string(), trim_env_value(value.trim()));
                }
            }
        }
        values
    }

    fn trim_env_value(value: &str) -> String {
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            return value[1..value.len() - 1].to_string();
        }
        value.to_string()
    }

    fn required_smoke_env(values: &HashMap<String, String>, key: &str) -> String {
        env::var(key)
            .ok()
            .or_else(|| values.get(key).cloned())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| panic!("missing required smoke env {key}"))
    }

    fn latest_stage_run(database_path: &Path) -> Result<HashMap<String, String>, String> {
        let connection = database::open_connection(database_path)?;
        connection
            .query_row(
                r#"
                SELECT
                    run_id,
                    entity_id,
                    stage_id,
                    COALESCE(success, 0),
                    COALESCE(http_status, -1),
                    COALESCE(error_type, ''),
                    COALESCE(request_json, '')
                FROM stage_runs
                ORDER BY id DESC
                LIMIT 1
                "#,
                [],
                |row| {
                    let run_id: String = row.get(0)?;
                    let entity_id: String = row.get(1)?;
                    let stage_id: String = row.get(2)?;
                    let success: i64 = row.get(3)?;
                    let http_status: i64 = row.get(4)?;
                    let error_type: String = row.get(5)?;
                    let request_json: String = row.get(6)?;
                    let source_key = source_key_from_request_json(&request_json);
                    let mut out = HashMap::new();
                    out.insert("run_id".to_string(), run_id);
                    out.insert("entity_id".to_string(), entity_id);
                    out.insert("stage_id".to_string(), stage_id);
                    out.insert("success".to_string(), success.to_string());
                    out.insert("http_status".to_string(), http_status.to_string());
                    out.insert("error_type".to_string(), error_type);
                    out.insert("source_key".to_string(), source_key);
                    Ok(out)
                },
            )
            .map_err(|error| format!("failed to load latest stage run: {error}"))
    }

    fn latest_successful_stage_runs(
        database_path: &Path,
        limit: u64,
    ) -> Result<Vec<HashMap<String, String>>, String> {
        let connection = database::open_connection(database_path)?;
        let mut statement = connection
            .prepare(
                r#"
                SELECT
                    run_id,
                    entity_id,
                    COALESCE(request_json, '')
                FROM stage_runs
                WHERE stage_id = 'smoke_source'
                  AND COALESCE(success, 0) = 1
                ORDER BY id DESC
                LIMIT ?1
                "#,
            )
            .map_err(|error| format!("failed to prepare successful stage run query: {error}"))?;
        let rows = statement
            .query_map([limit], |row| {
                let run_id: String = row.get(0)?;
                let entity_id: String = row.get(1)?;
                let request_json: String = row.get(2)?;
                let source_key = source_key_from_request_json(&request_json);
                let mut out = HashMap::new();
                out.insert("run_id".to_string(), run_id);
                out.insert("entity_id".to_string(), entity_id);
                out.insert("source_key".to_string(), source_key);
                Ok(out)
            })
            .map_err(|error| format!("failed to query successful stage runs: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read successful stage runs: {error}"))
    }

    fn source_key_from_request_json(request_json: &str) -> String {
        serde_json::from_str::<serde_json::Value>(request_json)
            .ok()
            .and_then(|value| {
                value["source_key"]
                    .as_str()
                    .or_else(|| value["source"]["key"].as_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default()
    }

    fn latest_output_file_for_run(
        database_path: &Path,
        run_id: &str,
    ) -> Result<Option<EntityFileRecord>, String> {
        Ok(database::list_entity_files(database_path, None)?
            .into_iter()
            .filter(|file| {
                file.stage_id == "smoke_processed"
                    && file.producer_run_id.as_deref() == Some(run_id)
            })
            .max_by_key(|file| file.id))
    }

    fn stage_state_status(
        database_path: &Path,
        entity_id: &str,
        stage_id: &str,
    ) -> Result<String, String> {
        database::get_stage_state_status(database_path, entity_id, stage_id)?
            .ok_or_else(|| format!("missing state for entity '{entity_id}' stage '{stage_id}'"))
    }
}

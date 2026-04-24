use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use chrono::Utc;
use rusqlite::Transaction;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::database::{
    ensure_entity_stub, find_entity_by_id, find_entity_file_by_entity_stage,
    find_entity_file_by_path, insert_app_event, list_entity_files,
    load_active_stages_from_connection, mark_missing_files_for_active_stages, open_connection,
    recompute_entity_summaries, set_setting, system_time_to_rfc3339, upsert_entity_file,
    upsert_entity_stage_state, EntityFileWriteOutcome, PersistEntityFileInput,
    PersistEntityStageStateInput,
};
use crate::domain::{
    AppEventLevel, ConfigValidationIssue, EntityValidationStatus, ScanSummary,
    StageDirectoryProvisionSummary, StageRecord, StageStatus, ValidationSeverity,
};
use crate::workdir::path_string;

pub fn ensure_stage_directories(
    workdir_path: &Path,
    database_path: &Path,
) -> Result<StageDirectoryProvisionSummary, String> {
    let connection = open_connection(database_path)?;
    let active_stages = load_active_stages_from_connection(&connection)?;
    let summary = ensure_stage_directories_for_stages(workdir_path, &active_stages)?;
    if summary.created_directory_count > 0 {
        insert_app_event(
            &connection,
            AppEventLevel::Info,
            "stage_directories_provisioned",
            "Missing stage directories were created during Stage 3 provisioning.",
            Some(json!({
                "created_directory_count": summary.created_directory_count,
                "paths": summary.created_paths,
            })),
            &Utc::now().to_rfc3339(),
        )?;
    }
    Ok(summary)
}

pub fn scan_workspace(workdir_path: &Path, database_path: &Path) -> Result<ScanSummary, String> {
    let started_at = Instant::now();
    let started_at_rfc3339 = Utc::now().to_rfc3339();
    let scan_id = format!(
        "scan-{}",
        Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_else(|| Utc::now().timestamp_micros() * 1000)
    );

    let provision_summary = ensure_stage_directories(workdir_path, database_path)?;

    let mut connection = open_connection(database_path)?;
    let active_stages = load_active_stages_from_connection(&connection)?;
    let active_stage_ids = active_stages
        .iter()
        .map(|stage| stage.id.clone())
        .collect::<HashSet<_>>();

    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start reconciliation transaction: {error}"))?;

    insert_app_event(
        &transaction,
        AppEventLevel::Info,
        "workspace_reconciliation_started",
        "Workspace reconciliation started.",
        Some(json!({
            "scan_id": scan_id,
            "active_stage_count": active_stages.len(),
            "workdir_path": path_string(workdir_path),
            "created_directory_count": provision_summary.created_directory_count,
        })),
        &started_at_rfc3339,
    )?;

    let mut summary =
        MutableScanSummary::new(scan_id.clone(), provision_summary.created_directory_count);
    let mut seen_paths = HashSet::new();
    let mut newly_registered_entities = HashSet::new();

    for stage in &active_stages {
        scan_stage(
            &transaction,
            workdir_path,
            stage,
            &active_stage_ids,
            &scan_id,
            &mut seen_paths,
            &mut summary,
            &mut newly_registered_entities,
        )?;
    }

    let finished_at = Utc::now().to_rfc3339();
    summary.missing_file_count = mark_missing_files_for_active_stages(
        &transaction,
        &active_stage_ids,
        &seen_paths,
        &scan_id,
        &finished_at,
    )?;
    summary.registered_entity_count = newly_registered_entities.len() as u64;

    recompute_entity_summaries(&transaction)?;

    summary.elapsed_ms = started_at.elapsed().as_millis();
    summary.latest_discovery_at = finished_at.clone();

    set_setting(&transaction, "last_scan_id", &scan_id, &finished_at)?;
    set_setting(
        &transaction,
        "last_scan_completed_at",
        &finished_at,
        &finished_at,
    )?;
    set_setting(
        &transaction,
        "last_scan_invalid_count",
        &summary.invalid_count.to_string(),
        &finished_at,
    )?;
    set_setting(
        &transaction,
        "last_scan_duplicate_count",
        &summary.duplicate_count.to_string(),
        &finished_at,
    )?;
    set_setting(
        &transaction,
        "last_scan_missing_count",
        &summary.missing_file_count.to_string(),
        &finished_at,
    )?;
    set_setting(
        &transaction,
        "last_scan_restored_count",
        &summary.restored_file_count.to_string(),
        &finished_at,
    )?;
    set_setting(
        &transaction,
        "last_scan_created_directory_count",
        &summary.created_directory_count.to_string(),
        &finished_at,
    )?;

    insert_app_event(
        &transaction,
        AppEventLevel::Info,
        "workspace_reconciliation_completed",
        "Workspace reconciliation completed.",
        Some(json!({
            "scan_id": scan_id,
            "scanned_file_count": summary.scanned_file_count,
            "registered_file_count": summary.registered_file_count,
            "registered_entity_count": summary.registered_entity_count,
            "updated_file_count": summary.updated_file_count,
            "unchanged_file_count": summary.unchanged_file_count,
            "missing_file_count": summary.missing_file_count,
            "restored_file_count": summary.restored_file_count,
            "invalid_count": summary.invalid_count,
            "duplicate_count": summary.duplicate_count,
            "created_directory_count": summary.created_directory_count,
            "elapsed_ms": summary.elapsed_ms,
        })),
        &finished_at,
    )?;

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit workspace reconciliation: {error}"))?;

    let mut result = summary.into_result();
    result.managed_copy_count = list_entity_files(database_path, None)?
        .into_iter()
        .filter(|file| file.is_managed_copy && file.file_exists)
        .count() as u64;

    Ok(result)
}

pub(crate) fn ensure_stage_directories_for_stage_ids(
    workdir_path: &Path,
    database_path: &Path,
    stage_ids: &[String],
) -> Result<StageDirectoryProvisionSummary, String> {
    let connection = open_connection(database_path)?;
    let active_stage_map = load_active_stages_from_connection(&connection)?
        .into_iter()
        .map(|stage| (stage.id.clone(), stage))
        .collect::<HashMap<_, _>>();
    let stages = stage_ids
        .iter()
        .filter_map(|stage_id| active_stage_map.get(stage_id).cloned())
        .collect::<Vec<_>>();
    ensure_stage_directories_for_stages(workdir_path, &stages)
}

fn ensure_stage_directories_for_stages(
    workdir_path: &Path,
    stages: &[StageRecord],
) -> Result<StageDirectoryProvisionSummary, String> {
    let mut created_paths = Vec::new();

    for stage in stages {
        let input_path = workdir_path.join(&stage.input_folder);
        create_directory_if_missing(&input_path, &mut created_paths)?;

        if !stage.output_folder.trim().is_empty() {
            let output_path = workdir_path.join(&stage.output_folder);
            create_directory_if_missing(&output_path, &mut created_paths)?;
        }
    }

    Ok(StageDirectoryProvisionSummary {
        created_directory_count: created_paths.len() as u64,
        created_paths,
    })
}

fn create_directory_if_missing(path: &Path, created_paths: &mut Vec<String>) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    fs::create_dir_all(path).map_err(|error| {
        format!(
            "Failed to create stage directory '{}': {error}",
            path.display()
        )
    })?;
    created_paths.push(path_string(path));
    Ok(())
}

fn scan_stage(
    transaction: &Transaction<'_>,
    workdir_path: &Path,
    stage: &StageRecord,
    active_stage_ids: &HashSet<String>,
    scan_id: &str,
    seen_paths: &mut HashSet<String>,
    summary: &mut MutableScanSummary,
    newly_registered_entities: &mut HashSet<String>,
) -> Result<(), String> {
    let input_dir = workdir_path.join(&stage.input_folder);
    if !input_dir.exists() || !input_dir.is_dir() {
        return Ok(());
    }

    let entries = fs::read_dir(&input_dir).map_err(|error| {
        format!(
            "Failed to read stage input folder '{}': {error}",
            input_dir.display()
        )
    })?;

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                record_file_error(
                    transaction,
                    scan_id,
                    stage,
                    input_dir.join("unknown-entry").as_path(),
                    "directory_entry_read_failed",
                    format!(
                        "Failed to read a directory entry in '{}': {error}",
                        input_dir.display()
                    ),
                )?;
                summary.invalid_count += 1;
                continue;
            }
        };

        let path = entry.path();
        let is_json_file = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
        if !is_json_file || !path.is_file() {
            continue;
        }

        summary.scanned_file_count += 1;
        seen_paths.insert(path_string(&path));

        match process_json_file(
            transaction,
            stage,
            &path,
            active_stage_ids,
            scan_id,
            newly_registered_entities,
        )? {
            FileProcessOutcome::Inserted => summary.registered_file_count += 1,
            FileProcessOutcome::Updated => summary.updated_file_count += 1,
            FileProcessOutcome::Unchanged => summary.unchanged_file_count += 1,
            FileProcessOutcome::Restored => summary.restored_file_count += 1,
            FileProcessOutcome::Invalid => summary.invalid_count += 1,
            FileProcessOutcome::Duplicate => summary.duplicate_count += 1,
        }
    }

    Ok(())
}

fn process_json_file(
    transaction: &Transaction<'_>,
    stage: &StageRecord,
    file_path: &Path,
    active_stage_ids: &HashSet<String>,
    scan_id: &str,
    newly_registered_entities: &mut HashSet<String>,
) -> Result<FileProcessOutcome, String> {
    let file_path_string = path_string(file_path);
    let file_name = file_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown.json")
        .to_string();

    let metadata = match fs::metadata(file_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            record_file_error(
                transaction,
                scan_id,
                stage,
                file_path,
                "file_metadata_unavailable",
                format!(
                    "Failed to read file metadata for '{}': {error}",
                    file_path.display()
                ),
            )?;
            return Ok(FileProcessOutcome::Invalid);
        }
    };

    let file_size = metadata.len();
    let file_mtime = match metadata.modified() {
        Ok(modified) => system_time_to_rfc3339(modified),
        Err(error) => {
            record_file_error(
                transaction,
                scan_id,
                stage,
                file_path,
                "file_metadata_unavailable",
                format!(
                    "Failed to read file modified time for '{}': {error}",
                    file_path.display()
                ),
            )?;
            return Ok(FileProcessOutcome::Invalid);
        }
    };

    let bytes = match fs::read(file_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            record_file_error(
                transaction,
                scan_id,
                stage,
                file_path,
                "file_read_failed",
                format!(
                    "Failed to read JSON file '{}': {error}",
                    file_path.display()
                ),
            )?;
            return Ok(FileProcessOutcome::Invalid);
        }
    };
    let checksum = format!("{:x}", Sha256::digest(&bytes));

    let json_value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => value,
        Err(error) => {
            record_file_error(
                transaction,
                scan_id,
                stage,
                file_path,
                "invalid_json_file",
                format!("File '{}' is not valid JSON: {error}", file_path.display()),
            )?;
            return Ok(FileProcessOutcome::Invalid);
        }
    };

    let Some(root) = json_value.as_object() else {
        record_file_error(
            transaction,
            scan_id,
            stage,
            file_path,
            "invalid_json_file",
            format!(
                "File '{}' must contain a JSON object at the root.",
                file_path.display()
            ),
        )?;
        return Ok(FileProcessOutcome::Invalid);
    };

    let Some(entity_id) = required_string(root, "id") else {
        record_file_error(
            transaction,
            scan_id,
            stage,
            file_path,
            "missing_entity_id",
            format!(
                "File '{}' is missing required field 'id'.",
                file_path.display()
            ),
        )?;
        return Ok(FileProcessOutcome::Invalid);
    };

    let Some(payload_value) = root.get("payload").filter(|value| !value.is_null()) else {
        record_file_error(
            transaction,
            scan_id,
            stage,
            file_path,
            "missing_payload",
            format!(
                "File '{}' is missing required field 'payload'.",
                file_path.display()
            ),
        )?;
        return Ok(FileProcessOutcome::Invalid);
    };

    if let Some(existing_for_path) = find_entity_file_by_path(transaction, &file_path_string)? {
        if existing_for_path.entity_id != entity_id {
            record_file_error(
                transaction,
                scan_id,
                stage,
                file_path,
                "entity_id_changed_for_path",
                format!(
                    "File '{}' now declares entity id '{}' but was already registered as '{}'.",
                    file_path.display(),
                    entity_id,
                    existing_for_path.entity_id
                ),
            )?;
            return Ok(FileProcessOutcome::Duplicate);
        }
    }

    if let Some(existing_stage_file) =
        find_entity_file_by_entity_stage(transaction, &entity_id, &stage.id)?
    {
        if existing_stage_file.file_path != file_path_string {
            record_file_error(
                transaction,
                scan_id,
                stage,
                file_path,
                "duplicate_entity_in_stage",
                format!(
                    "Entity '{}' is already registered at '{}' for stage '{}'; duplicate path '{}' was not registered.",
                    entity_id, existing_stage_file.file_path, stage.id, file_path.display()
                ),
            )?;
            return Ok(FileProcessOutcome::Duplicate);
        }
    }

    let mut validation_errors = Vec::new();
    if let Some(json_stage) = root.get("current_stage").and_then(Value::as_str) {
        if json_stage != stage.id {
            validation_errors.push(issue(
                ValidationSeverity::Warning,
                "current_stage_mismatch",
                "current_stage",
                format!(
                    "File declares current_stage '{json_stage}', but it was discovered under stage '{}'.",
                    stage.id
                ),
            ));
        }
    }

    let explicit_next_stage = root
        .get("next_stage")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let next_stage = explicit_next_stage
        .clone()
        .or_else(|| stage.next_stage.clone());

    if let Some(next_stage_id) = next_stage.as_ref() {
        if !active_stage_ids.contains(next_stage_id) {
            validation_errors.push(issue(
                ValidationSeverity::Warning,
                "unknown_or_inactive_next_stage",
                "next_stage",
                format!(
                    "Next stage '{next_stage_id}' is not an active stage in the current pipeline."
                ),
            ));
        }
    }

    let validation_status = if validation_errors.is_empty() {
        EntityValidationStatus::Valid
    } else {
        EntityValidationStatus::Warning
    };
    let now = Utc::now().to_rfc3339();
    let entity_was_known = find_entity_by_id(transaction, &entity_id)?.is_some();
    ensure_entity_stub(transaction, &entity_id, &now)?;

    let status = parse_status(root.get("status"));
    let file = PersistEntityFileInput {
        entity_id: entity_id.clone(),
        stage_id: stage.id.clone(),
        file_path: file_path_string.clone(),
        file_name,
        checksum,
        file_mtime,
        file_size,
        payload_json: serialize_json(payload_value)?,
        meta_json: serialize_json(
            &root
                .get("meta")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new())),
        )?,
        current_stage: Some(stage.id.clone()),
        next_stage,
        status: status.clone(),
        validation_status,
        validation_errors,
        is_managed_copy: extract_is_managed_copy(root),
        copy_source_file_id: None,
        first_seen_at: now.clone(),
        last_seen_at: now.clone(),
        updated_at: now.clone(),
    };

    let (write_outcome, file_id) = upsert_entity_file(transaction, &file)?;
    upsert_entity_stage_state(
        transaction,
        &PersistEntityStageStateInput {
            entity_id: file.entity_id.clone(),
            stage_id: stage.id.clone(),
            file_path: file_path_string.clone(),
            file_instance_id: Some(file_id),
            file_exists: true,
            status,
            max_attempts: stage.max_attempts,
            discovered_at: now.clone(),
            last_seen_at: now.clone(),
            updated_at: now.clone(),
        },
    )?;

    match write_outcome {
        EntityFileWriteOutcome::Inserted => {
            insert_app_event(
                transaction,
                AppEventLevel::Info,
                "file_discovered",
                &format!(
                    "Discovered file '{}' for entity '{}'.",
                    file_path.display(),
                    entity_id
                ),
                Some(json!({
                    "scan_id": scan_id,
                    "entity_id": entity_id,
                    "stage_id": stage.id,
                    "file_path": file_path_string,
                    "file_id": file_id,
                })),
                &now,
            )?;
            if !entity_was_known {
                newly_registered_entities.insert(file.entity_id.clone());
            }
            Ok(FileProcessOutcome::Inserted)
        }
        EntityFileWriteOutcome::Updated => {
            insert_app_event(
                transaction,
                AppEventLevel::Info,
                "file_updated",
                &format!(
                    "Updated file '{}' for entity '{}'.",
                    file_path.display(),
                    file.entity_id
                ),
                Some(json!({
                    "scan_id": scan_id,
                    "entity_id": file.entity_id,
                    "stage_id": stage.id,
                    "file_path": file.file_path,
                    "file_id": file_id,
                })),
                &now,
            )?;
            Ok(FileProcessOutcome::Updated)
        }
        EntityFileWriteOutcome::Restored => {
            insert_app_event(
                transaction,
                AppEventLevel::Info,
                "file_restored",
                &format!(
                    "Restored file '{}' for entity '{}'.",
                    file_path.display(),
                    file.entity_id
                ),
                Some(json!({
                    "scan_id": scan_id,
                    "entity_id": file.entity_id,
                    "stage_id": stage.id,
                    "file_path": file.file_path,
                    "file_id": file_id,
                })),
                &now,
            )?;
            Ok(FileProcessOutcome::Restored)
        }
        EntityFileWriteOutcome::Unchanged => Ok(FileProcessOutcome::Unchanged),
    }
}

fn required_string(root: &Map<String, Value>, field: &str) -> Option<String> {
    root.get(field)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn extract_is_managed_copy(root: &Map<String, Value>) -> bool {
    root.get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("beehive"))
        .and_then(Value::as_object)
        .and_then(|beehive| beehive.get("copy_source_stage"))
        .is_some()
}

fn parse_status(value: Option<&Value>) -> StageStatus {
    match value.and_then(Value::as_str).unwrap_or("pending") {
        "queued" => StageStatus::Queued,
        "in_progress" => StageStatus::InProgress,
        "retry_wait" => StageStatus::RetryWait,
        "done" => StageStatus::Done,
        "failed" => StageStatus::Failed,
        "blocked" => StageStatus::Blocked,
        "skipped" => StageStatus::Skipped,
        _ => StageStatus::Pending,
    }
}

fn record_file_error(
    transaction: &Transaction<'_>,
    scan_id: &str,
    stage: &StageRecord,
    file_path: &Path,
    code: &str,
    message: String,
) -> Result<(), String> {
    insert_app_event(
        transaction,
        AppEventLevel::Error,
        code,
        &message,
        Some(json!({
            "scan_id": scan_id,
            "stage_id": stage.id.clone(),
            "file_path": path_string(file_path),
            "file_name": file_path.file_name().and_then(|value| value.to_str()).unwrap_or("unknown"),
        })),
        &Utc::now().to_rfc3339(),
    )
}

fn issue(
    severity: ValidationSeverity,
    code: impl Into<String>,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ConfigValidationIssue {
    ConfigValidationIssue {
        severity,
        code: code.into(),
        path: path.into(),
        message: message.into(),
    }
}

fn serialize_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("Failed to serialize JSON: {error}"))
}

struct MutableScanSummary {
    scan_id: String,
    scanned_file_count: u64,
    registered_file_count: u64,
    registered_entity_count: u64,
    updated_file_count: u64,
    unchanged_file_count: u64,
    missing_file_count: u64,
    restored_file_count: u64,
    invalid_count: u64,
    duplicate_count: u64,
    created_directory_count: u64,
    managed_copy_count: u64,
    elapsed_ms: u128,
    latest_discovery_at: String,
}

impl MutableScanSummary {
    fn new(scan_id: String, created_directory_count: u64) -> Self {
        Self {
            scan_id,
            scanned_file_count: 0,
            registered_file_count: 0,
            registered_entity_count: 0,
            updated_file_count: 0,
            unchanged_file_count: 0,
            missing_file_count: 0,
            restored_file_count: 0,
            invalid_count: 0,
            duplicate_count: 0,
            created_directory_count,
            managed_copy_count: 0,
            elapsed_ms: 0,
            latest_discovery_at: String::new(),
        }
    }

    fn into_result(self) -> ScanSummary {
        ScanSummary {
            scan_id: self.scan_id,
            scanned_file_count: self.scanned_file_count,
            registered_file_count: self.registered_file_count,
            registered_entity_count: self.registered_entity_count,
            updated_file_count: self.updated_file_count,
            unchanged_file_count: self.unchanged_file_count,
            missing_file_count: self.missing_file_count,
            restored_file_count: self.restored_file_count,
            invalid_count: self.invalid_count,
            duplicate_count: self.duplicate_count,
            created_directory_count: self.created_directory_count,
            managed_copy_count: self.managed_copy_count,
            elapsed_ms: self.elapsed_ms,
            latest_discovery_at: self.latest_discovery_at,
        }
    }
}

enum FileProcessOutcome {
    Inserted,
    Updated,
    Unchanged,
    Restored,
    Invalid,
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{bootstrap_database, get_entity_detail, list_app_events, list_entities};
    use crate::domain::{
        EntityFilters, PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition,
    };

    fn test_config(stages: Vec<StageDefinition>) -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            runtime: RuntimeConfig::default(),
            stages,
        }
    }

    fn stage(
        id: &str,
        input_folder: &str,
        output_folder: &str,
        next_stage: Option<&str>,
    ) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: input_folder.to_string(),
            output_folder: output_folder.to_string(),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: next_stage.map(ToOwned::to_owned),
        }
    }

    fn write_json(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, contents).expect("write json");
    }

    #[test]
    fn missing_active_stage_directories_are_created() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        fs::create_dir_all(&workdir).expect("create workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "ingest",
                "stages/incoming",
                "stages/outgoing",
                None,
            )]),
        )
        .expect("bootstrap");

        let summary = ensure_stage_directories(&workdir, &database_path).expect("provision");

        assert_eq!(summary.created_directory_count, 2);
        assert!(workdir.join("stages").join("incoming").exists());
        assert!(workdir.join("stages").join("outgoing").exists());
    }

    #[test]
    fn stage_directory_provisioning_is_idempotent() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        fs::create_dir_all(&workdir).expect("create workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "ingest",
                "stages/incoming",
                "stages/outgoing",
                None,
            )]),
        )
        .expect("bootstrap");

        let first = ensure_stage_directories(&workdir, &database_path).expect("first");
        let second = ensure_stage_directories(&workdir, &database_path).expect("second");

        assert_eq!(first.created_directory_count, 2);
        assert_eq!(second.created_directory_count, 0);
    }

    #[test]
    fn same_entity_id_in_different_stages_is_allowed() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage(
                    "incoming",
                    "stages/incoming",
                    "stages/incoming-out",
                    Some("normalized"),
                ),
                stage(
                    "normalized",
                    "stages/normalized",
                    "stages/normalized-out",
                    None,
                ),
            ]),
        )
        .expect("bootstrap");

        write_json(
            &workdir
                .join("stages")
                .join("incoming")
                .join("entity-1.json"),
            r#"{"id":"entity-1","payload":{"step":"incoming"}}"#,
        );
        write_json(
            &workdir
                .join("stages")
                .join("normalized")
                .join("entity-1.json"),
            r#"{"id":"entity-1","payload":{"step":"normalized"}}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");

        assert_eq!(summary.registered_file_count, 2);
        assert_eq!(detail.files.len(), 2);
        assert_eq!(detail.entity.file_count, 2);
    }

    #[test]
    fn same_entity_id_twice_in_same_stage_is_rejected() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/out",
                None,
            )]),
        )
        .expect("bootstrap");

        write_json(
            &workdir
                .join("stages")
                .join("incoming")
                .join("entity-1-a.json"),
            r#"{"id":"entity-1","payload":{"step":"a"}}"#,
        );
        write_json(
            &workdir
                .join("stages")
                .join("incoming")
                .join("entity-1-b.json"),
            r#"{"id":"entity-1","payload":{"step":"b"}}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let events = list_app_events(&database_path, 20).expect("events");
        let detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");

        assert_eq!(summary.duplicate_count, 1);
        assert_eq!(detail.files.len(), 1);
        assert!(events
            .iter()
            .any(|event| event.code == "duplicate_entity_in_stage"));
    }

    #[test]
    fn deleted_file_is_marked_missing_and_restored_when_it_reappears() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let file_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/out",
                None,
            )]),
        )
        .expect("bootstrap");

        write_json(&file_path, r#"{"id":"entity-1","payload":{"value":1}}"#);
        scan_workspace(&workdir, &database_path).expect("first scan");
        fs::remove_file(&file_path).expect("remove file");

        let missing_summary = scan_workspace(&workdir, &database_path).expect("second scan");
        let missing_detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");

        assert_eq!(missing_summary.missing_file_count, 1);
        assert!(!missing_detail.files[0].file_exists);
        assert!(missing_detail.files[0].missing_since.is_some());

        write_json(&file_path, r#"{"id":"entity-1","payload":{"value":2}}"#);
        let restored_summary = scan_workspace(&workdir, &database_path).expect("third scan");
        let restored_detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");

        assert_eq!(restored_summary.restored_file_count, 1);
        assert!(restored_detail.files[0].file_exists);
        assert_eq!(restored_detail.files[0].missing_since, None);
    }

    #[test]
    fn changed_file_updates_checksum_and_timestamp() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let file_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/out",
                None,
            )]),
        )
        .expect("bootstrap");

        write_json(&file_path, r#"{"id":"entity-1","payload":{"value":1}}"#);
        scan_workspace(&workdir, &database_path).expect("first scan");
        let before = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");

        std::thread::sleep(std::time::Duration::from_millis(20));
        write_json(&file_path, r#"{"id":"entity-1","payload":{"value":2}}"#);
        let summary = scan_workspace(&workdir, &database_path).expect("second scan");
        let after = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");

        assert_eq!(summary.updated_file_count, 1);
        assert_ne!(before.files[0].checksum, after.files[0].checksum);
        assert_ne!(before.files[0].updated_at, after.files[0].updated_at);
    }

    #[test]
    fn malformed_json_is_recorded_without_crashing() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/out",
                None,
            )]),
        )
        .expect("bootstrap");

        write_json(
            &workdir.join("stages").join("incoming").join("broken.json"),
            r#"{"id":"broken""#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let events = list_app_events(&database_path, 20).expect("events");

        assert_eq!(summary.invalid_count, 1);
        assert!(events.iter().any(|event| event.code == "invalid_json_file"));
    }

    #[test]
    fn missing_id_is_recorded_without_registration() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/out",
                None,
            )]),
        )
        .expect("bootstrap");

        write_json(
            &workdir
                .join("stages")
                .join("incoming")
                .join("missing-id.json"),
            r#"{"payload":{"ok":true}}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let entities = list_entities(&database_path, &EntityFilters::default()).expect("entities");
        let events = list_app_events(&database_path, 20).expect("events");

        assert_eq!(summary.invalid_count, 1);
        assert!(entities.is_empty());
        assert!(events.iter().any(|event| event.code == "missing_entity_id"));
    }

    #[test]
    fn missing_payload_is_recorded_without_registration() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/out",
                None,
            )]),
        )
        .expect("bootstrap");

        write_json(
            &workdir
                .join("stages")
                .join("incoming")
                .join("missing-payload.json"),
            r#"{"id":"entity-1"}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let entities = list_entities(&database_path, &EntityFilters::default()).expect("entities");
        let events = list_app_events(&database_path, 20).expect("events");

        assert_eq!(summary.invalid_count, 1);
        assert!(entities.is_empty());
        assert!(events.iter().any(|event| event.code == "missing_payload"));
    }

    #[test]
    fn same_file_path_changing_entity_id_is_rejected() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let file_path = workdir.join("stages").join("incoming").join("entity.json");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/out",
                None,
            )]),
        )
        .expect("bootstrap");

        write_json(&file_path, r#"{"id":"entity-1","payload":{"ok":true}}"#);
        scan_workspace(&workdir, &database_path).expect("first scan");
        write_json(&file_path, r#"{"id":"entity-9","payload":{"ok":true}}"#);

        let summary = scan_workspace(&workdir, &database_path).expect("second scan");
        let detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");
        let events = list_app_events(&database_path, 20).expect("events");

        assert_eq!(summary.duplicate_count, 1);
        assert_eq!(detail.files[0].entity_id, "entity-1");
        assert!(events
            .iter()
            .any(|event| event.code == "entity_id_changed_for_path"));
    }

    #[test]
    fn inactive_stages_are_not_scanned_for_new_files() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage(
                    "incoming",
                    "stages/incoming",
                    "stages/incoming-out",
                    Some("normalized"),
                ),
                stage(
                    "normalized",
                    "stages/normalized",
                    "stages/normalized-out",
                    None,
                ),
            ]),
        )
        .expect("bootstrap one");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "normalized",
                "stages/normalized",
                "stages/normalized-out",
                None,
            )]),
        )
        .expect("bootstrap two");

        write_json(
            &workdir
                .join("stages")
                .join("incoming")
                .join("entity-a.json"),
            r#"{"id":"entity-a","payload":{"ok":true}}"#,
        );
        write_json(
            &workdir
                .join("stages")
                .join("normalized")
                .join("entity-b.json"),
            r#"{"id":"entity-b","payload":{"ok":true}}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let entities = list_entities(&database_path, &EntityFilters::default()).expect("entities");

        assert_eq!(summary.scanned_file_count, 1);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_id, "entity-b");
    }
}

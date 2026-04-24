use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use chrono::Utc;
use rusqlite::Transaction;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::database::{
    find_entity_by_file_path, find_entity_by_id, insert_app_event,
    load_active_stages_from_connection, open_connection, set_setting, system_time_to_rfc3339,
    upsert_entity, upsert_entity_stage_state, EntityWriteOutcome, PersistEntityInput,
    PersistEntityStageStateInput,
};
use crate::domain::{
    AppEventLevel, ConfigValidationIssue, EntityValidationStatus, ScanSummary, StageRecord,
    StageStatus, ValidationSeverity,
};
use crate::workdir::path_string;

pub fn scan_workspace(workdir_path: &Path, database_path: &Path) -> Result<ScanSummary, String> {
    let started_at = Instant::now();
    let scan_started_at = Utc::now().to_rfc3339();
    let scan_id = format!(
        "scan-{}",
        Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_else(|| Utc::now().timestamp_micros() * 1000)
    );

    let mut connection = open_connection(database_path)?;
    let active_stages = load_active_stages_from_connection(&connection)?;
    let active_stage_ids = active_stages
        .iter()
        .map(|stage| stage.id.clone())
        .collect::<HashSet<_>>();

    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start discovery transaction: {error}"))?;

    insert_app_event(
        &transaction,
        AppEventLevel::Info,
        "scan_started",
        "Workspace scan started.",
        Some(json!({
            "scan_id": scan_id,
            "active_stage_count": active_stages.len(),
            "workdir_path": path_string(workdir_path),
        })),
        &scan_started_at,
    )?;

    let mut summary = MutableScanSummary::new(scan_id.clone());

    for stage in active_stages {
        scan_stage(
            &transaction,
            workdir_path,
            &stage,
            &active_stage_ids,
            &scan_id,
            &mut summary,
        )?;
    }

    let finished_at = Utc::now().to_rfc3339();
    let elapsed_ms = started_at.elapsed().as_millis();
    summary.elapsed_ms = elapsed_ms;
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
        "last_scan_error_count",
        &(summary.invalid_count + summary.duplicate_count).to_string(),
        &finished_at,
    )?;

    insert_app_event(
        &transaction,
        AppEventLevel::Info,
        "scan_completed",
        "Workspace scan completed.",
        Some(json!({
            "scan_id": scan_id,
            "scanned_file_count": summary.scanned_file_count,
            "registered_count": summary.registered_count,
            "updated_count": summary.updated_count,
            "unchanged_count": summary.unchanged_count,
            "invalid_count": summary.invalid_count,
            "duplicate_count": summary.duplicate_count,
            "elapsed_ms": elapsed_ms,
        })),
        &finished_at,
    )?;

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit workspace scan: {error}"))?;

    Ok(summary.into_result())
}

fn scan_stage(
    transaction: &Transaction<'_>,
    workdir_path: &Path,
    stage: &StageRecord,
    active_stage_ids: &HashSet<String>,
    scan_id: &str,
    summary: &mut MutableScanSummary,
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
        match process_json_file(transaction, stage, &path, active_stage_ids, scan_id)? {
            FileProcessOutcome::Inserted => summary.registered_count += 1,
            FileProcessOutcome::Updated => summary.updated_count += 1,
            FileProcessOutcome::Unchanged => summary.unchanged_count += 1,
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

    if let Some(existing_for_path) = find_entity_by_file_path(transaction, &file_path_string)? {
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
            return Ok(FileProcessOutcome::Invalid);
        }
    }

    if let Some(existing_entity) = find_entity_by_id(transaction, &entity_id)? {
        if existing_entity.file_path != file_path_string {
            record_file_error(
                transaction,
                scan_id,
                stage,
                file_path,
                "duplicate_entity_id",
                format!(
                    "Entity id '{}' already exists at '{}'; duplicate file '{}' was not registered.",
                    entity_id, existing_entity.file_path, file_path.display()
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

    let entity = PersistEntityInput {
        entity_id: entity_id.clone(),
        file_path: file_path_string.clone(),
        file_name,
        stage_id: stage.id.clone(),
        current_stage: Some(stage.id.clone()),
        next_stage,
        status: StageStatus::Pending,
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
        validation_status,
        validation_errors,
        discovered_at: now.clone(),
        updated_at: now.clone(),
    };

    let outcome = upsert_entity(transaction, &entity)?;
    upsert_entity_stage_state(
        transaction,
        &PersistEntityStageStateInput {
            entity_id: entity.entity_id,
            stage_id: stage.id.clone(),
            file_path: file_path_string,
            status: StageStatus::Pending,
            max_attempts: stage.max_attempts,
            discovered_at: now.clone(),
            updated_at: now,
        },
    )?;

    Ok(match outcome {
        EntityWriteOutcome::Inserted => FileProcessOutcome::Inserted,
        EntityWriteOutcome::Updated => FileProcessOutcome::Updated,
        EntityWriteOutcome::Unchanged => FileProcessOutcome::Unchanged,
    })
}

fn required_string(root: &Map<String, Value>, field: &str) -> Option<String> {
    root.get(field)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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
    registered_count: u64,
    updated_count: u64,
    unchanged_count: u64,
    invalid_count: u64,
    duplicate_count: u64,
    elapsed_ms: u128,
    latest_discovery_at: String,
}

impl MutableScanSummary {
    fn new(scan_id: String) -> Self {
        Self {
            scan_id,
            scanned_file_count: 0,
            registered_count: 0,
            updated_count: 0,
            unchanged_count: 0,
            invalid_count: 0,
            duplicate_count: 0,
            elapsed_ms: 0,
            latest_discovery_at: String::new(),
        }
    }

    fn into_result(self) -> ScanSummary {
        ScanSummary {
            scan_id: self.scan_id,
            scanned_file_count: self.scanned_file_count,
            registered_count: self.registered_count,
            updated_count: self.updated_count,
            unchanged_count: self.unchanged_count,
            invalid_count: self.invalid_count,
            duplicate_count: self.duplicate_count,
            elapsed_ms: self.elapsed_ms,
            latest_discovery_at: self.latest_discovery_at,
        }
    }
}

enum FileProcessOutcome {
    Inserted,
    Updated,
    Unchanged,
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

    fn stage(id: &str, input_folder: &str, next_stage: Option<&str>) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: input_folder.to_string(),
            output_folder: format!("stages/{id}-out"),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: next_stage.map(ToOwned::to_owned),
        }
    }

    fn write_json(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write json");
    }

    #[test]
    fn inactive_stages_are_not_scanned() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        let normalize_dir = workdir.join("stages").join("normalize");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        fs::create_dir_all(&normalize_dir).expect("create normalize dir");
        let database_path = workdir.join("app.db");

        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("ingest", "stages/incoming", Some("normalize")),
                stage("normalize", "stages/normalize", None),
            ]),
        )
        .expect("bootstrap first");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("normalize", "stages/normalize", None)]),
        )
        .expect("bootstrap second");

        write_json(
            &ingest_dir.join("entity-1.json"),
            r#"{"id":"entity-1","payload":{"ok":true}}"#,
        );
        write_json(
            &normalize_dir.join("entity-2.json"),
            r#"{"id":"entity-2","payload":{"ok":true}}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan workspace");
        let entities = list_entities(&database_path, &EntityFilters::default()).expect("entities");

        assert_eq!(summary.scanned_file_count, 1);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_id, "entity-2");
    }

    #[test]
    fn valid_json_entity_is_registered() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("ingest", "stages/incoming", Some("normalize")),
                stage("normalize", "stages/normalize", None),
            ]),
        )
        .expect("bootstrap");
        write_json(
            &ingest_dir.join("entity-1.json"),
            r#"{
  "id": "entity-1",
  "current_stage": "ingest",
  "next_stage": "normalize",
  "payload": {"hello": "world"},
  "meta": {"source": "manual"}
}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let entities = list_entities(&database_path, &EntityFilters::default()).expect("entities");
        let detail = get_entity_detail(&database_path, "entity-1")
            .expect("entity detail")
            .expect("detail exists");

        assert_eq!(summary.registered_count, 1);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].status, "pending");
        assert_eq!(entities[0].validation_status, EntityValidationStatus::Valid);
        assert_eq!(detail.stage_states.len(), 1);
    }

    #[test]
    fn malformed_json_is_recorded_as_discovery_error() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("ingest", "stages/incoming", None)]),
        )
        .expect("bootstrap");
        write_json(&ingest_dir.join("broken.json"), r#"{"id":"broken""#);

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let events = list_app_events(&database_path, 10).expect("events");

        assert_eq!(summary.invalid_count, 1);
        assert!(events.iter().any(|event| event.code == "invalid_json_file"));
    }

    #[test]
    fn missing_id_is_recorded_and_not_registered() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("ingest", "stages/incoming", None)]),
        )
        .expect("bootstrap");
        write_json(
            &ingest_dir.join("missing-id.json"),
            r#"{"payload":{"ok":true}}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let entities = list_entities(&database_path, &EntityFilters::default()).expect("entities");
        let events = list_app_events(&database_path, 10).expect("events");

        assert_eq!(summary.invalid_count, 1);
        assert!(entities.is_empty());
        assert!(events.iter().any(|event| event.code == "missing_entity_id"));
    }

    #[test]
    fn missing_payload_is_recorded_and_not_registered() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("ingest", "stages/incoming", None)]),
        )
        .expect("bootstrap");
        write_json(
            &ingest_dir.join("missing-payload.json"),
            r#"{"id":"entity-1"}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let events = list_app_events(&database_path, 10).expect("events");

        assert_eq!(summary.invalid_count, 1);
        assert!(events.iter().any(|event| event.code == "missing_payload"));
    }

    #[test]
    fn duplicate_entity_id_in_different_paths_is_detected() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        let normalize_dir = workdir.join("stages").join("normalize");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        fs::create_dir_all(&normalize_dir).expect("create normalize dir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("ingest", "stages/incoming", Some("normalize")),
                stage("normalize", "stages/normalize", None),
            ]),
        )
        .expect("bootstrap");
        write_json(
            &ingest_dir.join("entity-1.json"),
            r#"{"id":"entity-1","payload":{"ok":true}}"#,
        );
        write_json(
            &normalize_dir.join("entity-1-copy.json"),
            r#"{"id":"entity-1","payload":{"ok":true}}"#,
        );

        let summary = scan_workspace(&workdir, &database_path).expect("scan");
        let entities = list_entities(&database_path, &EntityFilters::default()).expect("entities");
        let events = list_app_events(&database_path, 10).expect("events");

        assert_eq!(summary.registered_count, 1);
        assert_eq!(summary.duplicate_count, 1);
        assert_eq!(entities.len(), 1);
        assert!(events
            .iter()
            .any(|event| event.code == "duplicate_entity_id"));
    }

    #[test]
    fn rescan_is_idempotent_for_unchanged_files() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("ingest", "stages/incoming", None)]),
        )
        .expect("bootstrap");
        write_json(
            &ingest_dir.join("entity-1.json"),
            r#"{"id":"entity-1","payload":{"ok":true}}"#,
        );

        let first = scan_workspace(&workdir, &database_path).expect("first scan");
        let second = scan_workspace(&workdir, &database_path).expect("second scan");
        let entities = list_entities(&database_path, &EntityFilters::default()).expect("entities");

        assert_eq!(first.registered_count, 1);
        assert_eq!(second.unchanged_count, 1);
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn changed_file_updates_checksum_and_timestamp() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("ingest", "stages/incoming", None)]),
        )
        .expect("bootstrap");
        let file_path = ingest_dir.join("entity-1.json");
        write_json(&file_path, r#"{"id":"entity-1","payload":{"value":1}}"#);

        scan_workspace(&workdir, &database_path).expect("first scan");
        let first_entity = list_entities(&database_path, &EntityFilters::default())
            .expect("entities")
            .remove(0);
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_json(&file_path, r#"{"id":"entity-1","payload":{"value":2}}"#);

        let second_summary = scan_workspace(&workdir, &database_path).expect("second scan");
        let second_entity = list_entities(&database_path, &EntityFilters::default())
            .expect("entities")
            .remove(0);

        assert_eq!(second_summary.updated_count, 1);
        assert_ne!(first_entity.checksum, second_entity.checksum);
        assert_ne!(first_entity.updated_at, second_entity.updated_at);
    }

    #[test]
    fn current_stage_mismatch_is_stored_as_warning() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let ingest_dir = workdir.join("stages").join("incoming");
        fs::create_dir_all(&ingest_dir).expect("create ingest dir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("ingest", "stages/incoming", None)]),
        )
        .expect("bootstrap");
        write_json(
            &ingest_dir.join("entity-1.json"),
            r#"{"id":"entity-1","current_stage":"normalize","payload":{"ok":true}}"#,
        );

        scan_workspace(&workdir, &database_path).expect("scan");
        let entity = list_entities(&database_path, &EntityFilters::default())
            .expect("entities")
            .remove(0);

        assert_eq!(entity.validation_status, EntityValidationStatus::Warning);
        assert!(entity
            .validation_errors
            .iter()
            .any(|issue| issue.code == "current_stage_mismatch"));
    }
}

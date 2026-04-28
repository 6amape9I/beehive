use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OpenFlags, OptionalExtension, Transaction};
use serde_json::{json, Value};

use crate::domain::{
    AppEventLevel, AppEventRecord, ConfigValidationIssue, DatabaseState, EntityDetailPayload,
    EntityFileRecord, EntityFilters, EntityListQuery, EntityRecord, EntityStageStateRecord,
    EntityTableRow, EntityTimelineItem, EntityValidationStatus, InvalidDiscoveryRecord,
    PipelineConfig, RuntimeSummary, StageDefinition, StageRecord, StageRunRecord, StageStatus,
    StatusCount, WorkspaceEntityTrail, WorkspaceEntityTrailEdge, WorkspaceEntityTrailNode,
    WorkspaceExplorerResult, WorkspaceExplorerTotals, WorkspaceFileNode, WorkspaceStageTree,
    WorkspaceStageTreeCounters,
};
use crate::state_machine::{
    parse_status as parse_runtime_status, status_value as runtime_status_value,
    validate_transition, RuntimeTransitionReason,
};
use crate::workdir::path_string;

pub(crate) mod entities;
pub(crate) use entities::{
    evaluate_entity_file_allowed_actions, record_entity_file_json_edit_rejected,
};

const SCHEMA_VERSION: u32 = 4;

pub(crate) struct PersistEntityFileInput {
    pub entity_id: String,
    pub stage_id: String,
    pub file_path: String,
    pub file_name: String,
    pub checksum: String,
    pub file_mtime: String,
    pub file_size: u64,
    pub payload_json: String,
    pub meta_json: String,
    pub current_stage: Option<String>,
    pub next_stage: Option<String>,
    pub status: StageStatus,
    pub validation_status: EntityValidationStatus,
    pub validation_errors: Vec<ConfigValidationIssue>,
    pub is_managed_copy: bool,
    pub copy_source_file_id: Option<i64>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub updated_at: String,
}

pub(crate) struct PersistEntityStageStateInput {
    pub entity_id: String,
    pub stage_id: String,
    pub file_path: String,
    pub file_instance_id: Option<i64>,
    pub file_exists: bool,
    pub status: StageStatus,
    pub max_attempts: u64,
    pub discovered_at: String,
    pub last_seen_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityFileWriteOutcome {
    Inserted,
    Updated,
    Unchanged,
    Restored,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeTaskRecord {
    pub state_id: i64,
    pub entity_id: String,
    pub stage_id: String,
    pub status: String,
    pub attempts: u64,
    pub max_attempts: u64,
    pub file_path: String,
    pub file_instance_id: i64,
    pub file_exists: bool,
    pub workflow_url: String,
    pub retry_delay_sec: u64,
    pub next_stage: Option<String>,
}

pub struct EntityTablePage {
    pub entities: Vec<EntityTableRow>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
    pub available_statuses: Vec<String>,
}

pub(crate) struct NewStageRunInput {
    pub run_id: String,
    pub entity_id: String,
    pub entity_file_id: i64,
    pub stage_id: String,
    pub attempt_no: u64,
    pub workflow_url: String,
    pub request_json: String,
    pub started_at: String,
}

pub(crate) struct FinishStageRunInput {
    pub run_id: String,
    pub response_json: Option<String>,
    pub http_status: Option<i64>,
    pub success: bool,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub finished_at: String,
    pub duration_ms: u64,
}

struct StageStateTransitionContext {
    status: String,
    entity_id: String,
    stage_id: String,
}

pub(crate) struct StageStateIdentity {
    pub(crate) id: i64,
    pub(crate) status: String,
}

pub fn bootstrap_database(path: &Path, config: &PipelineConfig) -> Result<DatabaseState, String> {
    let mut connection = open_connection(path)?;
    ensure_schema(&mut connection)?;
    sync_stages(&mut connection, &config.stages)?;

    let stages = load_stage_records_from_connection(&connection)?;
    let schema_version = current_schema_version(&connection)?;
    let active_stage_count = stages.iter().filter(|stage| stage.is_active).count() as u64;
    let inactive_stage_count = stages.len() as u64 - active_stage_count;

    Ok(DatabaseState {
        database_path: path_string(path),
        is_ready: true,
        schema_version,
        stage_count: stages.len() as u64,
        synced_stage_ids: stages.iter().map(|stage| stage.id.clone()).collect(),
        active_stage_count,
        inactive_stage_count,
    })
}

pub fn open_connection(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create SQLite parent directory '{}': {error}",
                parent.display()
            )
        })?;
    }
    let connection = Connection::open(path).map_err(|error| {
        format!(
            "Failed to open SQLite database '{}': {error}",
            path.display()
        )
    })?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| format!("Failed to enable SQLite foreign keys: {error}"))?;
    Ok(connection)
}

fn open_readonly_connection(path: &Path) -> Result<Connection, String> {
    if !path.exists() {
        return Err(format!(
            "SQLite database '{}' does not exist.",
            path.display()
        ));
    }
    let connection =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| {
            format!(
                "Failed to open SQLite database '{}': {error}",
                path.display()
            )
        })?;
    Ok(connection)
}

pub fn get_runtime_summary(path: &Path) -> Result<RuntimeSummary, String> {
    let connection = open_connection(path)?;
    let schema_version = current_schema_version(&connection)?;
    let active_stage_count = query_count(
        &connection,
        "SELECT COUNT(*) FROM stages WHERE is_active = 1",
        [],
    )?;
    let inactive_stage_count = query_count(
        &connection,
        "SELECT COUNT(*) FROM stages WHERE is_active = 0",
        [],
    )?;
    let total_entities = query_count(&connection, "SELECT COUNT(*) FROM entities", [])?;
    let present_file_count = query_count(
        &connection,
        "SELECT COUNT(*) FROM entity_files WHERE file_exists = 1",
        [],
    )?;
    let missing_file_count = query_count(
        &connection,
        "SELECT COUNT(*) FROM entity_files WHERE file_exists = 0",
        [],
    )?;
    let managed_copy_count = query_count(
        &connection,
        "SELECT COUNT(*) FROM entity_files WHERE is_managed_copy = 1",
        [],
    )?;
    let invalid_file_count = load_setting(&connection, "last_scan_invalid_count")?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let last_reconciliation_at = load_setting(&connection, "last_scan_completed_at")?;
    let entities_by_status = load_status_counts(&connection)?;
    let execution_status_counts = load_execution_status_counts(&connection)?;

    Ok(RuntimeSummary {
        schema_version,
        active_stage_count,
        inactive_stage_count,
        total_entities,
        present_file_count,
        missing_file_count,
        managed_copy_count,
        invalid_file_count,
        entities_by_status,
        execution_status_counts,
        last_reconciliation_at,
    })
}

pub fn list_stages(path: &Path) -> Result<Vec<StageRecord>, String> {
    let connection = open_connection(path)?;
    load_stage_records_from_connection(&connection)
}

#[allow(dead_code)]
pub fn list_entities(path: &Path, filters: &EntityFilters) -> Result<Vec<EntityRecord>, String> {
    let connection = open_connection(path)?;
    let mut entities = load_entities_from_connection(&connection)?;

    if let Some(stage_id) = filters.stage_id.as_ref().filter(|value| !value.is_empty()) {
        entities.retain(|entity| entity.current_stage_id.as_deref() == Some(stage_id.as_str()));
    }
    if let Some(status) = filters.status.as_ref().filter(|value| !value.is_empty()) {
        entities.retain(|entity| entity.current_status == *status);
    }
    if let Some(validation_status) = filters.validation_status.as_ref() {
        entities.retain(|entity| &entity.validation_status == validation_status);
    }
    if let Some(search) = filters
        .search
        .as_ref()
        .map(|value| value.trim().to_lowercase())
    {
        if !search.is_empty() {
            entities.retain(|entity| {
                entity.entity_id.to_lowercase().contains(&search)
                    || entity
                        .latest_file_path
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&search)
            });
        }
    }

    Ok(entities)
}

pub fn list_entity_table_page(
    path: &Path,
    query: &EntityListQuery,
) -> Result<EntityTablePage, String> {
    let connection = open_connection(path)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * page_size;

    let mut where_clauses = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();

    if let Some(search) = query
        .search
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        where_clauses.push(
            "(LOWER(entity.entity_id) LIKE ? OR LOWER(COALESCE(entity.latest_file_path, '')) LIKE ? OR LOWER(COALESCE(latest_file.file_name, '')) LIKE ? OR LOWER(COALESCE(latest_file.payload_json, '')) LIKE ?)"
                .to_string(),
        );
        let pattern = format!("%{}%", search.to_lowercase());
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern));
    }
    if let Some(stage_id) = query
        .stage_id
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        where_clauses.push("entity.current_stage_id = ?".to_string());
        values.push(SqlValue::Text(stage_id.to_string()));
    }
    if let Some(status) = query
        .status
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        where_clauses.push("COALESCE(state.status, entity.current_status) = ?".to_string());
        values.push(SqlValue::Text(status.to_string()));
    }
    if let Some(validation_status) = query.validation_status.as_ref() {
        where_clauses.push("entity.validation_status = ?".to_string());
        values.push(SqlValue::Text(
            validation_status_value(validation_status).to_string(),
        ));
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };
    let order_by = entity_table_sort_expression(query.sort_by.as_deref());
    let direction = match query.sort_direction.as_deref() {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    let count_sql = format!(
        r#"
        SELECT COUNT(*)
        FROM entities entity
        LEFT JOIN entity_files latest_file
          ON latest_file.id = entity.latest_file_id
        LEFT JOIN entity_stage_states state
          ON state.entity_id = entity.entity_id
         AND state.stage_id = entity.current_stage_id
        {where_sql}
        "#
    );
    let total = connection
        .query_row(&count_sql, params_from_iter(values.iter()), |row| {
            row.get::<_, i64>(0)
        })
        .map(|value| value as u64)
        .map_err(|error| format!("Failed to count entity table rows: {error}"))?;

    let mut page_values = values.clone();
    page_values.push(SqlValue::Integer(page_size as i64));
    page_values.push(SqlValue::Integer(offset as i64));
    let sql = format!(
        r#"
        SELECT
            entity.entity_id,
            entity.current_stage_id,
            COALESCE(state.status, entity.current_status) AS runtime_status,
            entity.latest_file_path,
            entity.latest_file_id,
            latest_file.payload_json,
            entity.file_count,
            state.attempts,
            state.max_attempts,
            state.last_error,
            state.last_http_status,
            state.next_retry_at,
            state.last_started_at,
            state.last_finished_at,
            entity.validation_status,
            entity.updated_at,
            entity.last_seen_at
        FROM entities entity
        LEFT JOIN entity_files latest_file
          ON latest_file.id = entity.latest_file_id
        LEFT JOIN entity_stage_states state
          ON state.entity_id = entity.entity_id
         AND state.stage_id = entity.current_stage_id
        {where_sql}
        ORDER BY {order_by} {direction}, entity.entity_id ASC
        LIMIT ? OFFSET ?
        "#
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("Failed to prepare entity table query: {error}"))?;
    let rows = statement
        .query_map(
            params_from_iter(page_values.iter()),
            entity_table_row_from_row,
        )
        .map_err(|error| format!("Failed to query entity table rows: {error}"))?;
    let mut entities = Vec::new();
    for row in rows {
        entities.push(row.map_err(|error| format!("Failed to read entity table row: {error}"))?);
    }

    let available_statuses = load_available_entity_statuses(&connection)?;
    Ok(EntityTablePage {
        entities,
        total,
        page,
        page_size,
        available_statuses,
    })
}

pub fn list_entity_files(
    path: &Path,
    entity_id: Option<&str>,
) -> Result<Vec<EntityFileRecord>, String> {
    let connection = open_connection(path)?;
    load_entity_files_from_connection(&connection, entity_id)
}

#[allow(dead_code)]
pub fn get_entity_detail(
    path: &Path,
    entity_id: &str,
) -> Result<Option<EntityDetailPayload>, String> {
    get_entity_detail_with_selection(path, entity_id, None)
}

pub fn get_entity_detail_with_selection(
    path: &Path,
    entity_id: &str,
    selected_file_id: Option<i64>,
) -> Result<Option<EntityDetailPayload>, String> {
    let connection = open_connection(path)?;
    let Some(entity) = find_entity_by_id(&connection, entity_id)? else {
        return Ok(None);
    };
    let files = load_entity_files_from_connection(&connection, Some(entity_id))?;
    let stage_states = load_stage_states_for_entity(&connection, entity_id)?;
    let stage_runs = load_stage_runs_from_connection(&connection, Some(entity_id), 100)?;
    let timeline = build_entity_timeline(&connection, &stage_states)?;
    let json_preview = build_json_preview(
        files
            .iter()
            .find(|file| Some(file.id) == entity.latest_file_id)
            .or_else(|| files.first()),
    )?;
    let selected_file = selected_file_id
        .and_then(|id| files.iter().find(|file| file.id == id))
        .or_else(|| {
            files
                .iter()
                .find(|file| Some(file.id) == entity.latest_file_id)
        })
        .or_else(|| files.first());
    let selected_file_json = selected_file.map(build_full_file_json).transpose()?;
    let allowed_actions = entities::build_stage_allowed_actions(&stage_states);
    let file_allowed_actions = entities::build_file_allowed_actions(&files, &stage_states);

    Ok(Some(EntityDetailPayload {
        entity,
        files,
        stage_states,
        stage_runs,
        timeline,
        latest_json_preview: json_preview,
        selected_file_json,
        allowed_actions,
        file_allowed_actions,
    }))
}

pub fn list_app_events(path: &Path, limit: u32) -> Result<Vec<AppEventRecord>, String> {
    let connection = open_connection(path)?;
    load_app_events_from_connection(&connection, limit)
}

pub fn list_stage_runs(
    path: &Path,
    entity_id: Option<&str>,
) -> Result<Vec<StageRunRecord>, String> {
    let connection = open_connection(path)?;
    load_stage_runs_from_connection(&connection, entity_id, 100)
}

pub fn get_stage_state_status(
    path: &Path,
    entity_id: &str,
    stage_id: &str,
) -> Result<Option<String>, String> {
    let connection = open_connection(path)?;
    find_stage_state_identity(&connection, entity_id, stage_id).map(|state| state.map(|s| s.status))
}

pub fn record_manual_retry_event(
    path: &Path,
    entity_id: &str,
    stage_id: &str,
    previous_status: Option<&str>,
    new_status: Option<&str>,
    operator_comment: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(path)?;
    let now = Utc::now().to_rfc3339();
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "manual_retry_now",
        &format!("Manual retry requested for entity '{entity_id}' on stage '{stage_id}'."),
        Some(json!({
            "action": "retry_now",
            "entity_id": entity_id,
            "stage_id": stage_id,
            "operator_comment": operator_comment,
            "previous_status": previous_status,
            "new_status": new_status,
        })),
        &now,
    )
}

pub fn reset_entity_stage_to_pending(
    path: &Path,
    entity_id: &str,
    stage_id: &str,
    operator_comment: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(path)?;
    let now = Utc::now().to_rfc3339();
    let state = find_stage_state_identity(&connection, entity_id, stage_id)?.ok_or_else(|| {
        format!("No stage state exists for entity '{entity_id}' on stage '{stage_id}'.")
    })?;

    if state.status == "pending" {
        insert_app_event(
            &connection,
            AppEventLevel::Info,
            "manual_reset_noop",
            &format!("Manual reset requested for already-pending entity '{entity_id}' on stage '{stage_id}'."),
            Some(json!({
                "action": "reset_to_pending",
                "entity_id": entity_id,
                "stage_id": stage_id,
                "operator_comment": operator_comment,
                "previous_status": state.status,
                "new_status": "pending",
            })),
            &now,
        )?;
        return Ok(());
    }

    ensure_runtime_transition(
        &state.status,
        &StageStatus::Pending,
        RuntimeTransitionReason::ManualReset,
        Some(state.id),
        Some(entity_id),
        Some(stage_id),
    )?;
    connection
        .execute(
            r#"
            UPDATE entity_stage_states
            SET status = 'pending',
                attempts = 0,
                last_error = NULL,
                last_http_status = NULL,
                next_retry_at = NULL,
                updated_at = ?2
            WHERE id = ?1
            "#,
            params![state.id, now],
        )
        .map_err(|error| {
            format!(
                "Failed to reset entity '{entity_id}' on stage '{stage_id}' to pending: {error}"
            )
        })?;
    update_entity_summary_from_state(&connection, state.id, StageStatus::Pending, &now)?;
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "manual_reset_to_pending",
        &format!("Manual reset moved entity '{entity_id}' on stage '{stage_id}' to pending."),
        Some(json!({
            "action": "reset_to_pending",
            "entity_id": entity_id,
            "stage_id": stage_id,
            "operator_comment": operator_comment,
            "previous_status": state.status,
            "new_status": "pending",
        })),
        &now,
    )
}

pub fn skip_entity_stage(
    path: &Path,
    entity_id: &str,
    stage_id: &str,
    operator_comment: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(path)?;
    let now = Utc::now().to_rfc3339();
    let state = find_stage_state_identity(&connection, entity_id, stage_id)?.ok_or_else(|| {
        format!("No stage state exists for entity '{entity_id}' on stage '{stage_id}'.")
    })?;

    ensure_runtime_transition(
        &state.status,
        &StageStatus::Skipped,
        RuntimeTransitionReason::ManualSkip,
        Some(state.id),
        Some(entity_id),
        Some(stage_id),
    )?;
    connection
        .execute(
            r#"
            UPDATE entity_stage_states
            SET status = 'skipped',
                next_retry_at = NULL,
                updated_at = ?2
            WHERE id = ?1
            "#,
            params![state.id, now],
        )
        .map_err(|error| {
            format!("Failed to skip entity '{entity_id}' on stage '{stage_id}': {error}")
        })?;
    update_entity_summary_from_state(&connection, state.id, StageStatus::Skipped, &now)?;
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "manual_skip",
        &format!("Manual skip marked entity '{entity_id}' on stage '{stage_id}' as skipped."),
        Some(json!({
            "action": "skip",
            "entity_id": entity_id,
            "stage_id": stage_id,
            "operator_comment": operator_comment,
            "previous_status": state.status,
            "new_status": "skipped",
        })),
        &now,
    )
}

pub fn get_workspace_explorer(
    workdir_path: &Path,
    database_path: &Path,
) -> Result<WorkspaceExplorerResult, String> {
    let connection = open_readonly_connection(database_path)?;
    let generated_at = Utc::now().to_rfc3339();
    let stages = load_stage_records_from_connection(&connection)?;
    let stage_order: HashMap<String, usize> = stages
        .iter()
        .enumerate()
        .map(|(index, stage)| (stage.id.clone(), index))
        .collect();
    let files = load_entity_files_from_connection(&connection, None)?;
    let stage_states = load_all_stage_states_from_connection(&connection)?;
    let state_by_file_id: HashMap<i64, EntityStageStateRecord> = stage_states
        .iter()
        .filter_map(|state| {
            state
                .file_instance_id
                .map(|file_id| (file_id, state.clone()))
        })
        .collect();
    let mut state_by_stage: HashMap<String, Vec<EntityStageStateRecord>> = HashMap::new();
    let mut created_child_by_source_file: HashMap<i64, String> = HashMap::new();
    for state in &stage_states {
        state_by_stage
            .entry(state.stage_id.clone())
            .or_default()
            .push(state.clone());
        if let (Some(file_id), Some(child_path)) =
            (state.file_instance_id, state.created_child_path.as_ref())
        {
            created_child_by_source_file.insert(file_id, child_path.clone());
        }
    }

    let mut file_lookup: HashMap<i64, EntityFileRecord> = HashMap::new();
    for file in &files {
        file_lookup.insert(file.id, file.clone());
    }

    let last_scan_at = load_setting(&connection, "last_scan_completed_at")?;
    let mut invalid_by_stage = load_invalid_discovery_records_for_latest_scan(&connection)?;
    let mut files_by_stage: HashMap<String, Vec<WorkspaceFileNode>> = HashMap::new();

    for file in &files {
        let absolute_path = workdir_path.join(&file.file_path);
        let parent_exists = absolute_path
            .parent()
            .map(|parent| parent.exists())
            .unwrap_or(false);
        let source_file = file
            .copy_source_file_id
            .and_then(|source_file_id| file_lookup.get(&source_file_id));
        let runtime_status = state_by_file_id
            .get(&file.id)
            .map(|state| state.status.clone())
            .or_else(|| {
                stage_states
                    .iter()
                    .find(|state| {
                        state.entity_id == file.entity_id && state.stage_id == file.stage_id
                    })
                    .map(|state| state.status.clone())
            });

        files_by_stage
            .entry(file.stage_id.clone())
            .or_default()
            .push(WorkspaceFileNode {
                entity_file_id: file.id,
                entity_id: file.entity_id.clone(),
                stage_id: file.stage_id.clone(),
                file_name: file.file_name.clone(),
                file_path: file.file_path.clone(),
                file_exists: file.file_exists,
                missing_since: file.missing_since.clone(),
                is_managed_copy: file.is_managed_copy,
                copy_source_file_id: file.copy_source_file_id,
                copy_source_entity_id: source_file.map(|source| source.entity_id.clone()),
                copy_source_stage_id: source_file.map(|source| source.stage_id.clone()),
                runtime_status,
                file_status: file.status.clone(),
                validation_status: file.validation_status.clone(),
                validation_errors: file.validation_errors.clone(),
                current_stage: file.current_stage.clone(),
                next_stage: file.next_stage.clone(),
                checksum: file.checksum.clone(),
                file_size: file.file_size,
                file_mtime: file.file_mtime.clone(),
                updated_at: file.updated_at.clone(),
                can_open_file: file.file_exists && absolute_path.exists(),
                can_open_folder: parent_exists,
            });
    }

    let mut totals = WorkspaceExplorerTotals {
        stages_total: stages.len() as u64,
        active_stages_total: stages.iter().filter(|stage| stage.is_active).count() as u64,
        inactive_stages_total: stages.iter().filter(|stage| !stage.is_active).count() as u64,
        entities_total: query_count(&connection, "SELECT COUNT(*) FROM entities", [])?,
        registered_files_total: files.len() as u64,
        present_files_total: files.iter().filter(|file| file.file_exists).count() as u64,
        missing_files_total: files.iter().filter(|file| !file.file_exists).count() as u64,
        invalid_files_total: 0,
        managed_copies_total: files.iter().filter(|file| file.is_managed_copy).count() as u64,
    };

    let stage_trees = stages
        .into_iter()
        .map(|stage| {
            let stage_files = files_by_stage.remove(&stage.id).unwrap_or_default();
            let invalid_files = invalid_by_stage.remove(&stage.id).unwrap_or_default();
            totals.invalid_files_total += invalid_files.len() as u64;
            let counters = build_workspace_stage_counters(
                &stage_files,
                state_by_stage
                    .get(&stage.id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                invalid_files.len() as u64,
            );
            let folder_path = workdir_path.join(&stage.input_folder);
            WorkspaceStageTree {
                stage_id: stage.id,
                input_folder: stage.input_folder,
                output_folder: non_empty_string(stage.output_folder),
                workflow_url: non_empty_string(stage.workflow_url),
                next_stage: stage.next_stage,
                is_active: stage.is_active,
                archived_at: stage.archived_at,
                folder_path: path_string(&folder_path),
                folder_exists: folder_path.exists(),
                files: stage_files,
                invalid_files,
                counters,
            }
        })
        .collect();

    let entity_trails = build_workspace_entity_trails(
        workdir_path,
        &files,
        &state_by_file_id,
        &stage_order,
        &created_child_by_source_file,
    );

    Ok(WorkspaceExplorerResult {
        generated_at,
        workdir_path: path_string(workdir_path),
        last_scan_at,
        stages: stage_trees,
        entity_trails,
        totals,
        errors: Vec::new(),
    })
}

fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn build_workspace_stage_counters(
    files: &[WorkspaceFileNode],
    states: &[EntityStageStateRecord],
    invalid_files: u64,
) -> WorkspaceStageTreeCounters {
    let mut counters = WorkspaceStageTreeCounters {
        registered_files: files.len() as u64,
        present_files: files.iter().filter(|file| file.file_exists).count() as u64,
        missing_files: files.iter().filter(|file| !file.file_exists).count() as u64,
        invalid_files,
        managed_copies: files.iter().filter(|file| file.is_managed_copy).count() as u64,
        ..WorkspaceStageTreeCounters::default()
    };

    for state in states {
        match state.status.as_str() {
            "pending" => counters.pending += 1,
            "queued" => counters.queued += 1,
            "in_progress" => counters.in_progress += 1,
            "retry_wait" => counters.retry_wait += 1,
            "done" => counters.done += 1,
            "failed" => counters.failed += 1,
            "blocked" => counters.blocked += 1,
            "skipped" => counters.skipped += 1,
            _ => {}
        }
    }

    counters
}

fn load_invalid_discovery_records_for_latest_scan(
    connection: &Connection,
) -> Result<HashMap<String, Vec<InvalidDiscoveryRecord>>, String> {
    let Some(scan_id) = load_setting(connection, "last_scan_id")? else {
        return Ok(HashMap::new());
    };
    let mut statement = connection
        .prepare(
            r#"
            SELECT code, message, context_json, created_at
            FROM app_events
            WHERE code IN (
                'invalid_json_file',
                'missing_entity_id',
                'missing_payload',
                'duplicate_entity_in_stage',
                'entity_id_changed_for_path',
                'file_metadata_unavailable',
                'file_read_failed'
            )
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .map_err(|error| format!("Failed to prepare invalid discovery event query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| format!("Failed to query invalid discovery events: {error}"))?;

    let mut by_stage: HashMap<String, Vec<InvalidDiscoveryRecord>> = HashMap::new();
    for row in rows {
        let (code, message, context_json, created_at) =
            row.map_err(|error| format!("Failed to read invalid discovery event: {error}"))?;
        let Some(context_json) = context_json else {
            continue;
        };
        let context = parse_json_value(&context_json)?;
        if context.get("scan_id").and_then(Value::as_str) != Some(scan_id.as_str()) {
            continue;
        }
        let Some(stage_id) = context.get("stage_id").and_then(Value::as_str) else {
            continue;
        };

        by_stage
            .entry(stage_id.to_string())
            .or_default()
            .push(InvalidDiscoveryRecord {
                stage_id: Some(stage_id.to_string()),
                file_name: context
                    .get("file_name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                file_path: context
                    .get("file_path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                code,
                message,
                created_at,
            });
    }

    Ok(by_stage)
}

fn load_all_stage_states_from_connection(
    connection: &Connection,
) -> Result<Vec<EntityStageStateRecord>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_instance_id,
                file_exists,
                status,
                attempts,
                max_attempts,
                last_error,
                last_http_status,
                next_retry_at,
                last_started_at,
                last_finished_at,
                created_child_path,
                discovered_at,
                last_seen_at,
                updated_at
            FROM entity_stage_states
            ORDER BY stage_id ASC, updated_at DESC, id DESC
            "#,
        )
        .map_err(|error| format!("Failed to prepare workspace stage-state query: {error}"))?;
    let rows = statement
        .query_map([], stage_state_from_row)
        .map_err(|error| format!("Failed to query workspace stage states: {error}"))?;

    let mut states = Vec::new();
    for row in rows {
        states.push(row.map_err(|error| format!("Failed to read workspace stage state: {error}"))?);
    }
    Ok(states)
}

fn stage_state_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityStageStateRecord> {
    Ok(EntityStageStateRecord {
        id: row.get(0)?,
        entity_id: row.get(1)?,
        stage_id: row.get(2)?,
        file_path: row.get(3)?,
        file_instance_id: row.get(4)?,
        file_exists: row.get::<_, i64>(5)? == 1,
        status: row.get(6)?,
        attempts: row.get::<_, i64>(7)? as u64,
        max_attempts: row.get::<_, i64>(8)? as u64,
        last_error: row.get(9)?,
        last_http_status: row.get(10)?,
        next_retry_at: row.get(11)?,
        last_started_at: row.get(12)?,
        last_finished_at: row.get(13)?,
        created_child_path: row.get(14)?,
        discovered_at: row.get(15)?,
        last_seen_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

fn build_workspace_entity_trails(
    workdir_path: &Path,
    files: &[EntityFileRecord],
    state_by_file_id: &HashMap<i64, EntityStageStateRecord>,
    stage_order: &HashMap<String, usize>,
    created_child_by_source_file: &HashMap<i64, String>,
) -> Vec<WorkspaceEntityTrail> {
    let mut files_by_entity: HashMap<String, Vec<EntityFileRecord>> = HashMap::new();
    for file in files {
        files_by_entity
            .entry(file.entity_id.clone())
            .or_default()
            .push(file.clone());
    }

    let mut trails = Vec::new();
    for (entity_id, mut entity_files) in files_by_entity {
        entity_files.sort_by(|left, right| {
            stage_order
                .get(&left.stage_id)
                .unwrap_or(&usize::MAX)
                .cmp(stage_order.get(&right.stage_id).unwrap_or(&usize::MAX))
                .then_with(|| left.updated_at.cmp(&right.updated_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        let nodes = entity_files
            .iter()
            .map(|file| {
                let absolute_path = workdir_path.join(&file.file_path);
                let parent_exists = absolute_path
                    .parent()
                    .map(|parent| parent.exists())
                    .unwrap_or(false);
                WorkspaceEntityTrailNode {
                    entity_file_id: file.id,
                    stage_id: file.stage_id.clone(),
                    file_name: file.file_name.clone(),
                    file_path: file.file_path.clone(),
                    file_exists: file.file_exists,
                    runtime_status: state_by_file_id
                        .get(&file.id)
                        .map(|state| state.status.clone()),
                    is_managed_copy: file.is_managed_copy,
                    can_open_file: file.file_exists && absolute_path.exists(),
                    can_open_folder: parent_exists,
                }
            })
            .collect::<Vec<_>>();

        let known_ids: HashSet<i64> = entity_files.iter().map(|file| file.id).collect();
        let mut edges = Vec::new();
        let mut edge_keys = HashSet::new();
        for file in &entity_files {
            if let Some(source_file_id) = file.copy_source_file_id {
                if known_ids.contains(&source_file_id) {
                    let key = (source_file_id, file.id);
                    if edge_keys.insert(key) {
                        edges.push(WorkspaceEntityTrailEdge {
                            from_entity_file_id: source_file_id,
                            to_entity_file_id: file.id,
                            relation: if file.is_managed_copy {
                                "managed_copy".to_string()
                            } else {
                                "copy_source".to_string()
                            },
                            created_child_path: created_child_by_source_file
                                .get(&source_file_id)
                                .cloned(),
                        });
                    }
                }
            }
        }

        for pair in entity_files.windows(2) {
            let from = &pair[0];
            let to = &pair[1];
            let key = (from.id, to.id);
            if edge_keys.insert(key) {
                edges.push(WorkspaceEntityTrailEdge {
                    from_entity_file_id: from.id,
                    to_entity_file_id: to.id,
                    relation: "same_entity_stage_sequence_inferred".to_string(),
                    created_child_path: created_child_by_source_file.get(&from.id).cloned(),
                });
            }
        }

        trails.push(WorkspaceEntityTrail {
            entity_id,
            file_count: nodes.len() as u64,
            stages: nodes,
            edges,
        });
    }

    trails.sort_by(|left, right| left.entity_id.cmp(&right.entity_id));
    trails
}

pub(crate) fn load_active_stages_from_connection(
    connection: &Connection,
) -> Result<Vec<StageRecord>, String> {
    let stages = load_stage_records_from_connection(connection)?;
    Ok(stages.into_iter().filter(|stage| stage.is_active).collect())
}

pub(crate) fn find_stage_by_id(
    connection: &Connection,
    stage_id: &str,
) -> Result<Option<StageRecord>, String> {
    Ok(load_stage_records_from_connection(connection)?
        .into_iter()
        .find(|stage| stage.id == stage_id))
}

pub(crate) fn find_entity_by_id(
    connection: &Connection,
    entity_id: &str,
) -> Result<Option<EntityRecord>, String> {
    connection
        .query_row(
            r#"
            SELECT
                entity_id,
                current_stage_id,
                current_status,
                latest_file_path,
                latest_file_id,
                file_count,
                validation_status,
                validation_errors_json,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entities
            WHERE entity_id = ?1
            "#,
            params![entity_id],
            entity_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load entity '{entity_id}': {error}"))
}

pub(crate) fn find_entity_file_by_path(
    connection: &Connection,
    file_path: &str,
) -> Result<Option<EntityFileRecord>, String> {
    connection
        .query_row(
            entity_files_select_sql(Some("WHERE file_path = ?1")),
            params![file_path],
            entity_file_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load entity file '{file_path}': {error}"))
}

pub(crate) fn find_entity_file_by_id(
    connection: &Connection,
    file_id: i64,
) -> Result<Option<EntityFileRecord>, String> {
    connection
        .query_row(
            entity_files_select_sql(Some("WHERE id = ?1")),
            params![file_id],
            entity_file_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load entity file id '{file_id}': {error}"))
}

pub(crate) fn find_entity_file_by_entity_stage(
    connection: &Connection,
    entity_id: &str,
    stage_id: &str,
) -> Result<Option<EntityFileRecord>, String> {
    connection
        .query_row(
            entity_files_select_sql(Some(
                "WHERE entity_id = ?1 AND stage_id = ?2 ORDER BY updated_at DESC, id DESC LIMIT 1",
            )),
            params![entity_id, stage_id],
            entity_file_from_row,
        )
        .optional()
        .map_err(|error| {
            format!("Failed to load entity file for entity '{entity_id}' on stage '{stage_id}': {error}")
        })
}

pub(crate) fn find_latest_entity_file_for_stage(
    connection: &Connection,
    entity_id: &str,
    stage_id: &str,
) -> Result<Option<EntityFileRecord>, String> {
    find_entity_file_by_entity_stage(connection, entity_id, stage_id)
}

pub(crate) fn list_eligible_runtime_tasks(
    connection: &Connection,
    now: &str,
    limit: u64,
) -> Result<Vec<RuntimeTaskRecord>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                state.id,
                state.entity_id,
                state.stage_id,
                state.status,
                state.attempts,
                state.max_attempts,
                state.file_path,
                state.file_instance_id,
                state.file_exists,
                stage.workflow_url,
                stage.retry_delay_sec,
                stage.next_stage
            FROM entity_stage_states state
            JOIN stages stage ON stage.stage_id = state.stage_id
            JOIN entity_files file ON file.id = state.file_instance_id
            WHERE stage.is_active = 1
              AND state.file_exists = 1
              AND file.file_exists = 1
              AND TRIM(stage.workflow_url) <> ''
              AND state.attempts < state.max_attempts
              AND (
                    state.status = 'pending'
                    OR (state.status = 'retry_wait' AND state.next_retry_at IS NOT NULL AND state.next_retry_at <= ?1)
                  )
            ORDER BY state.updated_at ASC, state.id ASC
            LIMIT ?2
            "#,
        )
        .map_err(|error| format!("Failed to prepare eligible task query: {error}"))?;
    let rows = statement
        .query_map(params![now, limit as i64], runtime_task_from_row)
        .map_err(|error| format!("Failed to query eligible runtime tasks: {error}"))?;

    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row.map_err(|error| format!("Failed to read eligible task row: {error}"))?);
    }
    Ok(tasks)
}

pub(crate) fn find_runtime_task(
    connection: &Connection,
    entity_id: &str,
    stage_id: &str,
) -> Result<Option<RuntimeTaskRecord>, String> {
    connection
        .query_row(
            r#"
            SELECT
                state.id,
                state.entity_id,
                state.stage_id,
                state.status,
                state.attempts,
                state.max_attempts,
                state.file_path,
                state.file_instance_id,
                state.file_exists,
                stage.workflow_url,
                stage.retry_delay_sec,
                stage.next_stage
            FROM entity_stage_states state
            JOIN stages stage ON stage.stage_id = state.stage_id
            LEFT JOIN entity_files file ON file.id = state.file_instance_id
            WHERE state.entity_id = ?1 AND state.stage_id = ?2
            "#,
            params![entity_id, stage_id],
            runtime_task_from_row,
        )
        .optional()
        .map_err(|error| {
            format!("Failed to load runtime task for entity '{entity_id}' on stage '{stage_id}': {error}")
        })
}

fn find_runtime_task_by_state_id(
    connection: &Connection,
    state_id: i64,
) -> Result<Option<RuntimeTaskRecord>, String> {
    connection
        .query_row(
            r#"
            SELECT
                state.id,
                state.entity_id,
                state.stage_id,
                state.status,
                state.attempts,
                state.max_attempts,
                state.file_path,
                state.file_instance_id,
                state.file_exists,
                stage.workflow_url,
                stage.retry_delay_sec,
                stage.next_stage
            FROM entity_stage_states state
            JOIN stages stage ON stage.stage_id = state.stage_id
            LEFT JOIN entity_files file ON file.id = state.file_instance_id
            WHERE state.id = ?1
            "#,
            params![state_id],
            runtime_task_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load runtime task state '{state_id}': {error}"))
}

pub(crate) fn claim_eligible_runtime_tasks(
    connection: &mut Connection,
    now: &str,
    limit: u64,
) -> Result<Vec<RuntimeTaskRecord>, String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start runtime claim transaction: {error}"))?;
    let candidates = list_eligible_runtime_tasks(&transaction, now, limit)?;
    let mut claimed = Vec::new();

    for candidate in candidates {
        ensure_runtime_transition(
            &candidate.status,
            &StageStatus::Queued,
            RuntimeTransitionReason::RuntimeClaim,
            Some(candidate.state_id),
            Some(&candidate.entity_id),
            Some(&candidate.stage_id),
        )?;
        let affected = transaction
            .execute(
                r#"
                UPDATE entity_stage_states
                SET status = 'queued',
                    updated_at = ?2
                WHERE id = ?1
                  AND file_exists = 1
                  AND attempts < max_attempts
                  AND (
                        status = 'pending'
                        OR (status = 'retry_wait' AND next_retry_at IS NOT NULL AND next_retry_at <= ?2)
                      )
                  AND EXISTS (
                        SELECT 1 FROM stages
                        WHERE stages.stage_id = entity_stage_states.stage_id
                          AND stages.is_active = 1
                          AND TRIM(stages.workflow_url) <> ''
                  )
                  AND EXISTS (
                        SELECT 1 FROM entity_files
                        WHERE entity_files.id = entity_stage_states.file_instance_id
                          AND entity_files.file_exists = 1
                  )
                "#,
                params![candidate.state_id, now],
            )
            .map_err(|error| {
                format!(
                    "Failed to claim runtime state '{}' for entity '{}' on stage '{}': {error}",
                    candidate.state_id, candidate.entity_id, candidate.stage_id
                )
            })?;

        if affected == 1 {
            if let Some(task) = find_runtime_task_by_state_id(&transaction, candidate.state_id)? {
                claimed.push(task);
            }
        }
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit runtime claim transaction: {error}"))?;
    Ok(claimed)
}

pub(crate) fn claim_specific_runtime_task(
    connection: &mut Connection,
    entity_id: &str,
    stage_id: &str,
    now: &str,
) -> Result<Option<RuntimeTaskRecord>, String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start runtime claim transaction: {error}"))?;
    let Some(candidate) = find_runtime_task(&transaction, entity_id, stage_id)? else {
        transaction
            .commit()
            .map_err(|error| format!("Failed to commit empty runtime claim: {error}"))?;
        return Ok(None);
    };

    if !matches!(candidate.status.as_str(), "pending" | "retry_wait") {
        transaction
            .commit()
            .map_err(|error| format!("Failed to commit skipped runtime claim: {error}"))?;
        return Ok(None);
    }

    ensure_runtime_transition(
        &candidate.status,
        &StageStatus::Queued,
        RuntimeTransitionReason::RuntimeClaim,
        Some(candidate.state_id),
        Some(&candidate.entity_id),
        Some(&candidate.stage_id),
    )?;

    let affected = transaction
        .execute(
            r#"
            UPDATE entity_stage_states
            SET status = 'queued',
                updated_at = ?4
            WHERE id = ?1
              AND entity_id = ?2
              AND stage_id = ?3
              AND file_exists = 1
              AND attempts < max_attempts
              AND status IN ('pending', 'retry_wait')
              AND EXISTS (
                    SELECT 1 FROM stages
                    WHERE stages.stage_id = entity_stage_states.stage_id
                      AND stages.is_active = 1
                      AND TRIM(stages.workflow_url) <> ''
              )
              AND EXISTS (
                    SELECT 1 FROM entity_files
                    WHERE entity_files.id = entity_stage_states.file_instance_id
                      AND entity_files.file_exists = 1
              )
            "#,
            params![candidate.state_id, entity_id, stage_id, now],
        )
        .map_err(|error| {
            format!(
                "Failed to claim runtime state '{}' for entity '{}' on stage '{}': {error}",
                candidate.state_id, entity_id, stage_id
            )
        })?;

    let task = if affected == 1 {
        find_runtime_task_by_state_id(&transaction, candidate.state_id)?
    } else {
        None
    };
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit runtime claim transaction: {error}"))?;
    Ok(task)
}

pub(crate) fn release_queued_claim(
    connection: &Connection,
    state_id: i64,
    updated_at: &str,
) -> Result<(), String> {
    let context = ensure_state_transition(
        connection,
        state_id,
        &StageStatus::Pending,
        RuntimeTransitionReason::ClaimRecovery,
    )?;
    connection
        .execute(
            r#"
            UPDATE entity_stage_states
            SET status = 'pending',
                next_retry_at = NULL,
                updated_at = ?2
            WHERE id = ?1 AND status = 'queued'
            "#,
            params![state_id, updated_at],
        )
        .map_err(|error| {
            format!(
                "Failed to release queued claim for entity '{}' on stage '{}': {error}",
                context.entity_id, context.stage_id
            )
        })?;
    update_entity_summary_from_state(connection, state_id, StageStatus::Pending, updated_at)?;
    Ok(())
}

pub(crate) fn reconcile_orphan_stage_runs_for_queued_state(
    connection: &Connection,
    state_id: i64,
    updated_at: &str,
) -> Result<u64, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT run.run_id
            FROM stage_runs run
            JOIN entity_stage_states state
              ON state.entity_id = run.entity_id
             AND state.stage_id = run.stage_id
             AND state.file_instance_id = run.entity_file_id
            WHERE state.id = ?1
              AND state.status = 'queued'
              AND run.finished_at IS NULL
            ORDER BY run.started_at ASC, run.id ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare orphan stage-run query: {error}"))?;
    let rows = statement
        .query_map(params![state_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Failed to query orphan stage-runs: {error}"))?;
    let mut run_ids = Vec::new();
    for row in rows {
        run_ids.push(row.map_err(|error| format!("Failed to read orphan stage-run row: {error}"))?);
    }
    drop(statement);

    for run_id in &run_ids {
        finish_stage_run(
            connection,
            &FinishStageRunInput {
                run_id: run_id.clone(),
                response_json: None,
                http_status: None,
                success: false,
                error_type: Some("claim_recovered_before_start".to_string()),
                error_message: Some(
                    "Queued claim was recovered before workflow request was sent.".to_string(),
                ),
                finished_at: updated_at.to_string(),
                duration_ms: 0,
            },
        )?;
    }

    Ok(run_ids.len() as u64)
}

pub(crate) fn update_stage_state_success(
    connection: &Connection,
    state_id: i64,
    http_status: Option<i64>,
    finished_at: &str,
    created_child_path: Option<&str>,
) -> Result<(), String> {
    ensure_state_transition(
        connection,
        state_id,
        &StageStatus::Done,
        RuntimeTransitionReason::RuntimeSuccess,
    )?;
    connection
        .execute(
            r#"
            UPDATE entity_stage_states
            SET status = 'done',
                last_error = NULL,
                last_http_status = ?2,
                next_retry_at = NULL,
                last_finished_at = ?3,
                created_child_path = ?4,
                updated_at = ?3
            WHERE id = ?1
            "#,
            params![state_id, http_status, finished_at, created_child_path],
        )
        .map_err(|error| format!("Failed to mark stage state '{state_id}' done: {error}"))?;
    update_entity_summary_from_state(connection, state_id, StageStatus::Done, finished_at)?;
    Ok(())
}

pub(crate) fn update_stage_state_failure(
    connection: &Connection,
    state_id: i64,
    status: StageStatus,
    error_message: &str,
    http_status: Option<i64>,
    next_retry_at: Option<&str>,
    finished_at: &str,
) -> Result<(), String> {
    let reason = match status {
        StageStatus::RetryWait => RuntimeTransitionReason::RuntimeRetryScheduled,
        StageStatus::Failed => RuntimeTransitionReason::RuntimeFailed,
        StageStatus::Blocked => RuntimeTransitionReason::RuntimeBlocked,
        _ => RuntimeTransitionReason::RuntimeFailed,
    };
    update_stage_state_failure_with_reason(
        connection,
        state_id,
        status,
        error_message,
        http_status,
        next_retry_at,
        finished_at,
        reason,
    )
}

pub(crate) fn update_stage_state_failure_with_reason(
    connection: &Connection,
    state_id: i64,
    status: StageStatus,
    error_message: &str,
    http_status: Option<i64>,
    next_retry_at: Option<&str>,
    finished_at: &str,
    reason: RuntimeTransitionReason,
) -> Result<(), String> {
    ensure_state_transition(connection, state_id, &status, reason)?;
    connection
        .execute(
            r#"
            UPDATE entity_stage_states
            SET status = ?2,
                last_error = ?3,
                last_http_status = ?4,
                next_retry_at = ?5,
                last_finished_at = ?6,
                updated_at = ?6
            WHERE id = ?1
            "#,
            params![
                state_id,
                stage_status_value(&status),
                error_message,
                http_status,
                next_retry_at,
                finished_at
            ],
        )
        .map_err(|error| {
            format!("Failed to mark stage state '{state_id}' failed/retry: {error}")
        })?;
    update_entity_summary_from_state(connection, state_id, status, finished_at)?;
    Ok(())
}

pub(crate) fn block_stage_state(
    connection: &Connection,
    state_id: i64,
    error_message: &str,
    updated_at: &str,
) -> Result<(), String> {
    update_stage_state_failure(
        connection,
        state_id,
        StageStatus::Blocked,
        error_message,
        None,
        None,
        updated_at,
    )
}

pub(crate) fn insert_stage_run(
    connection: &Connection,
    input: &NewStageRunInput,
) -> Result<(), String> {
    connection
        .execute(
            r#"
            INSERT INTO stage_runs (
                run_id,
                entity_id,
                entity_file_id,
                stage_id,
                attempt_no,
                workflow_url,
                request_json,
                success,
                started_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)
            "#,
            params![
                input.run_id,
                input.entity_id,
                input.entity_file_id,
                input.stage_id,
                input.attempt_no as i64,
                input.workflow_url,
                input.request_json,
                input.started_at
            ],
        )
        .map_err(|error| format!("Failed to insert stage run '{}': {error}", input.run_id))?;
    Ok(())
}

pub(crate) fn start_claimed_stage_run(
    connection: &mut Connection,
    state_id: i64,
    input: &NewStageRunInput,
) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start stage-run transaction: {error}"))?;
    ensure_state_transition(
        &transaction,
        state_id,
        &StageStatus::InProgress,
        RuntimeTransitionReason::RuntimeStart,
    )?;
    insert_stage_run(&transaction, input)?;
    transaction
        .execute(
            r#"
            UPDATE entity_stage_states
            SET status = 'in_progress',
                attempts = ?2,
                last_started_at = ?3,
                last_finished_at = NULL,
                next_retry_at = NULL,
                updated_at = ?3
            WHERE id = ?1 AND status = 'queued'
            "#,
            params![state_id, input.attempt_no as i64, input.started_at],
        )
        .map_err(|error| format!("Failed to mark stage state '{state_id}' in_progress: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit stage-run start transaction: {error}"))?;
    Ok(())
}

pub(crate) fn finish_stage_run(
    connection: &Connection,
    input: &FinishStageRunInput,
) -> Result<(), String> {
    connection
        .execute(
            r#"
            UPDATE stage_runs
            SET response_json = ?2,
                http_status = ?3,
                success = ?4,
                error_type = ?5,
                error_message = ?6,
                finished_at = ?7,
                duration_ms = ?8
            WHERE run_id = ?1
            "#,
            params![
                input.run_id,
                input.response_json,
                input.http_status,
                bool_to_i64(input.success),
                input.error_type,
                input.error_message,
                input.finished_at,
                input.duration_ms as i64
            ],
        )
        .map_err(|error| format!("Failed to finish stage run '{}': {error}", input.run_id))?;
    Ok(())
}

pub(crate) fn ensure_entity_stub(
    transaction: &Transaction<'_>,
    entity_id: &str,
    now: &str,
) -> Result<(), String> {
    transaction
        .execute(
            r#"
            INSERT INTO entities (
                entity_id,
                current_stage_id,
                current_status,
                latest_file_path,
                latest_file_id,
                file_count,
                validation_status,
                validation_errors_json,
                first_seen_at,
                last_seen_at,
                updated_at
            )
            VALUES (?1, NULL, 'pending', NULL, NULL, 0, 'valid', '[]', ?2, ?2, ?2)
            ON CONFLICT(entity_id) DO NOTHING
            "#,
            params![entity_id, now],
        )
        .map_err(|error| format!("Failed to ensure logical entity stub '{entity_id}': {error}"))?;
    Ok(())
}

pub(crate) fn upsert_entity_file(
    transaction: &Transaction<'_>,
    file: &PersistEntityFileInput,
) -> Result<(EntityFileWriteOutcome, i64), String> {
    let serialized_errors = serialize_json(&file.validation_errors)?;
    let status = stage_status_value(&file.status);
    let validation_status = validation_status_value(&file.validation_status);

    let existing = find_entity_file_by_path(transaction, &file.file_path)?;
    match existing {
        Some(existing)
            if existing.entity_id == file.entity_id
                && existing.stage_id == file.stage_id
                && existing.checksum == file.checksum
                && existing.file_mtime == file.file_mtime
                && existing.file_size == file.file_size
                && existing.current_stage == file.current_stage
                && existing.next_stage == file.next_stage
                && existing.status == status
                && existing.payload_json == file.payload_json
                && existing.meta_json == file.meta_json
                && existing.validation_status == file.validation_status
                && existing.validation_errors == file.validation_errors
                && existing.file_exists
                && existing.is_managed_copy == file.is_managed_copy
                && existing.copy_source_file_id == file.copy_source_file_id =>
        {
            transaction
                .execute(
                    "UPDATE entity_files SET last_seen_at = ?2 WHERE id = ?1",
                    params![existing.id, file.last_seen_at],
                )
                .map_err(|error| {
                    format!(
                        "Failed to refresh last_seen_at for entity file '{}': {error}",
                        existing.file_path
                    )
                })?;
            Ok((EntityFileWriteOutcome::Unchanged, existing.id))
        }
        Some(existing) => {
            transaction
                .execute(
                    r#"
                    UPDATE entity_files
                    SET
                        entity_id = ?2,
                        stage_id = ?3,
                        file_name = ?4,
                        checksum = ?5,
                        file_mtime = ?6,
                        file_size = ?7,
                        payload_json = ?8,
                        meta_json = ?9,
                        current_stage = ?10,
                        next_stage = ?11,
                        status = ?12,
                        validation_status = ?13,
                        validation_errors_json = ?14,
                        is_managed_copy = ?15,
                        copy_source_file_id = ?16,
                        file_exists = 1,
                        missing_since = NULL,
                        last_seen_at = ?17,
                        updated_at = ?18
                    WHERE id = ?1
                    "#,
                    params![
                        existing.id,
                        file.entity_id,
                        file.stage_id,
                        file.file_name,
                        file.checksum,
                        file.file_mtime,
                        file.file_size as i64,
                        file.payload_json,
                        file.meta_json,
                        file.current_stage,
                        file.next_stage,
                        status,
                        validation_status,
                        serialized_errors,
                        bool_to_i64(file.is_managed_copy),
                        file.copy_source_file_id,
                        file.last_seen_at,
                        file.updated_at,
                    ],
                )
                .map_err(|error| {
                    format!(
                        "Failed to update entity file '{}': {error}",
                        existing.file_path
                    )
                })?;

            let outcome = if existing.file_exists {
                EntityFileWriteOutcome::Updated
            } else {
                EntityFileWriteOutcome::Restored
            };
            Ok((outcome, existing.id))
        }
        None => {
            transaction
                .execute(
                    r#"
                    INSERT INTO entity_files (
                        entity_id,
                        stage_id,
                        file_path,
                        file_name,
                        checksum,
                        file_mtime,
                        file_size,
                        payload_json,
                        meta_json,
                        current_stage,
                        next_stage,
                        status,
                        validation_status,
                        validation_errors_json,
                        is_managed_copy,
                        copy_source_file_id,
                        file_exists,
                        missing_since,
                        first_seen_at,
                        last_seen_at,
                        updated_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 1, NULL, ?17, ?18, ?19)
                    "#,
                    params![
                        file.entity_id,
                        file.stage_id,
                        file.file_path,
                        file.file_name,
                        file.checksum,
                        file.file_mtime,
                        file.file_size as i64,
                        file.payload_json,
                        file.meta_json,
                        file.current_stage,
                        file.next_stage,
                        status,
                        validation_status,
                        serialized_errors,
                        bool_to_i64(file.is_managed_copy),
                        file.copy_source_file_id,
                        file.first_seen_at,
                        file.last_seen_at,
                        file.updated_at,
                    ],
                )
                .map_err(|error| {
                    format!("Failed to insert entity file '{}': {error}", file.file_path)
                })?;
            Ok((
                EntityFileWriteOutcome::Inserted,
                transaction.last_insert_rowid(),
            ))
        }
    }
}

pub(crate) fn upsert_entity_stage_state(
    transaction: &Transaction<'_>,
    stage_state: &PersistEntityStageStateInput,
) -> Result<(), String> {
    transaction
        .execute(
            r#"
            INSERT INTO entity_stage_states (
                entity_id,
                stage_id,
                file_path,
                file_instance_id,
                file_exists,
                status,
                attempts,
                max_attempts,
                last_error,
                last_http_status,
                next_retry_at,
                last_started_at,
                last_finished_at,
                created_child_path,
                discovered_at,
                last_seen_at,
                updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, NULL, NULL, NULL, NULL, NULL, NULL, ?8, ?9, ?10)
            ON CONFLICT(entity_id, stage_id) DO UPDATE SET
                file_path = excluded.file_path,
                file_instance_id = excluded.file_instance_id,
                file_exists = excluded.file_exists,
                max_attempts = excluded.max_attempts,
                last_seen_at = excluded.last_seen_at,
                updated_at = excluded.updated_at
            "#,
            params![
                stage_state.entity_id,
                stage_state.stage_id,
                stage_state.file_path,
                stage_state.file_instance_id,
                bool_to_i64(stage_state.file_exists),
                stage_status_value(&stage_state.status),
                stage_state.max_attempts as i64,
                stage_state.discovered_at,
                stage_state.last_seen_at,
                stage_state.updated_at,
            ],
        )
        .map_err(|error| {
            format!(
                "Failed to upsert stage state for entity '{}' on stage '{}': {error}",
                stage_state.entity_id, stage_state.stage_id
            )
        })?;
    Ok(())
}

pub(crate) fn mark_missing_files_for_active_stages(
    transaction: &Transaction<'_>,
    active_stage_ids: &HashSet<String>,
    seen_paths: &HashSet<String>,
    scan_id: &str,
    seen_at: &str,
) -> Result<u64, String> {
    let existing_files = load_entity_files_from_connection(transaction, None)?;
    let mut missing_count = 0;

    for file in existing_files {
        if !active_stage_ids.contains(&file.stage_id) {
            continue;
        }
        if seen_paths.contains(&file.file_path) || !file.file_exists {
            continue;
        }

        transaction
            .execute(
                r#"
                UPDATE entity_files
                SET file_exists = 0,
                    missing_since = COALESCE(missing_since, ?2),
                    updated_at = ?2
                WHERE id = ?1
                "#,
                params![file.id, seen_at],
            )
            .map_err(|error| {
                format!(
                    "Failed to mark file '{}' as missing: {error}",
                    file.file_path
                )
            })?;

        transaction
            .execute(
                r#"
                UPDATE entity_stage_states
                SET file_exists = 0,
                    last_seen_at = ?3,
                    updated_at = ?3
                WHERE entity_id = ?1 AND stage_id = ?2
                "#,
                params![file.entity_id, file.stage_id, seen_at],
            )
            .map_err(|error| {
                format!(
                    "Failed to mark stage state missing for entity '{}' on stage '{}': {error}",
                    file.entity_id, file.stage_id
                )
            })?;

        insert_app_event(
            transaction,
            AppEventLevel::Warning,
            "file_missing",
            &format!(
                "Tracked file '{}' is missing from the workspace.",
                file.file_path
            ),
            Some(json!({
                "scan_id": scan_id,
                "entity_id": file.entity_id,
                "stage_id": file.stage_id,
                "file_path": file.file_path,
            })),
            seen_at,
        )?;
        missing_count += 1;
    }

    Ok(missing_count)
}

pub(crate) fn recompute_entity_summaries(transaction: &Transaction<'_>) -> Result<(), String> {
    let entity_ids = load_entity_ids(transaction)?;

    for entity_id in entity_ids {
        let files = load_entity_files_from_connection(transaction, Some(&entity_id))?;
        if files.is_empty() {
            continue;
        }

        let latest_present = files
            .iter()
            .filter(|file| file.file_exists)
            .max_by(|left, right| compare_file_records(left, right));
        let latest_any = files
            .iter()
            .max_by(|left, right| compare_file_records(left, right));
        let latest = latest_present.or(latest_any).expect("files is not empty");

        let validation_status = files
            .iter()
            .map(|file| validation_rank(&file.validation_status))
            .max()
            .map(validation_status_from_rank)
            .unwrap_or(EntityValidationStatus::Valid);

        let validation_errors = latest.validation_errors.clone();
        let file_count = files.len() as u64;
        let first_seen_at = files
            .iter()
            .map(|file| file.first_seen_at.as_str())
            .min()
            .unwrap_or(latest.first_seen_at.as_str())
            .to_string();
        let last_seen_at = files
            .iter()
            .map(|file| file.last_seen_at.as_str())
            .max()
            .unwrap_or(latest.last_seen_at.as_str())
            .to_string();
        let updated_at = files
            .iter()
            .map(|file| file.updated_at.as_str())
            .max()
            .unwrap_or(latest.updated_at.as_str())
            .to_string();
        let runtime_status = transaction
            .query_row(
                r#"
                SELECT status FROM entity_stage_states
                WHERE entity_id = ?1 AND stage_id = ?2
                "#,
                params![entity_id, latest.stage_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| {
                format!(
                    "Failed to load runtime status for entity '{}' on stage '{}': {error}",
                    entity_id, latest.stage_id
                )
            })?
            .unwrap_or_else(|| latest.status.clone());

        transaction
            .execute(
                r#"
                UPDATE entities
                SET
                    current_stage_id = ?2,
                    current_status = ?3,
                    latest_file_path = ?4,
                    latest_file_id = ?5,
                    file_count = ?6,
                    validation_status = ?7,
                    validation_errors_json = ?8,
                    first_seen_at = ?9,
                    last_seen_at = ?10,
                    updated_at = ?11
                WHERE entity_id = ?1
                "#,
                params![
                    entity_id,
                    latest.stage_id,
                    runtime_status,
                    latest.file_path,
                    latest.id,
                    file_count as i64,
                    validation_status_value(&validation_status),
                    serialize_json(&validation_errors)?,
                    first_seen_at,
                    last_seen_at,
                    updated_at,
                ],
            )
            .map_err(|error| {
                format!("Failed to recompute entity summary '{entity_id}': {error}")
            })?;
    }

    Ok(())
}

pub(crate) fn insert_app_event(
    connection: &Connection,
    level: AppEventLevel,
    code: &str,
    message: &str,
    context: Option<Value>,
    created_at: &str,
) -> Result<(), String> {
    let context_json = match context {
        Some(context) => Some(serialize_json(&context)?),
        None => None,
    };

    connection
        .execute(
            r#"
            INSERT INTO app_events (level, code, message, context_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                app_event_level_value(&level),
                code,
                message,
                context_json,
                created_at
            ],
        )
        .map_err(|error| format!("Failed to insert app event '{code}': {error}"))?;

    Ok(())
}

pub(crate) fn set_setting(
    connection: &Connection,
    key: &str,
    value: &str,
    updated_at: &str,
) -> Result<(), String> {
    connection
        .execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            params![key, value, updated_at],
        )
        .map_err(|error| format!("Failed to write setting '{key}': {error}"))?;

    Ok(())
}

pub(crate) fn load_setting(connection: &Connection, key: &str) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Failed to read setting '{key}': {error}"))
}

fn ensure_schema(connection: &mut Connection) -> Result<(), String> {
    match current_schema_version(connection)? {
        0 => create_schema_v4(connection)?,
        1 => {
            migrate_v1_to_v2(connection)?;
            migrate_v2_to_v3(connection)?;
            migrate_v3_to_v4(connection)?;
        }
        2 => {
            migrate_v2_to_v3(connection)?;
            migrate_v3_to_v4(connection)?;
        }
        3 => migrate_v3_to_v4(connection)?,
        4 => {}
        version => {
            return Err(format!(
                "Unsupported SQLite schema version '{version}'. Expected 0, 1, 2, 3, or 4."
            ))
        }
    }

    ensure_query_indexes(connection)?;
    let now = Utc::now().to_rfc3339();
    set_setting(
        connection,
        "schema_version",
        &SCHEMA_VERSION.to_string(),
        &now,
    )?;
    Ok(())
}

fn ensure_query_indexes(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_stage_status ON entity_stage_states(stage_id, status);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_status_retry ON entity_stage_states(status, next_retry_at);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_entity_stage ON entity_stage_states(entity_id, stage_id);
            CREATE INDEX IF NOT EXISTS idx_entities_updated_at ON entities(updated_at);
            CREATE INDEX IF NOT EXISTS idx_entities_current_stage_status ON entities(current_stage_id, current_status);
            CREATE INDEX IF NOT EXISTS idx_stage_runs_started_at ON stage_runs(started_at);
            CREATE INDEX IF NOT EXISTS idx_stage_runs_entity_stage ON stage_runs(entity_id, stage_id);
            CREATE INDEX IF NOT EXISTS idx_app_events_level_created_at ON app_events(level, created_at);
            "#,
        )
        .map_err(|error| format!("Failed to ensure dashboard query indexes: {error}"))?;
    Ok(())
}

fn current_schema_version(connection: &Connection) -> Result<u32, String> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
        .map_err(|error| format!("Failed to read PRAGMA user_version: {error}"))
}

fn create_schema_v4(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS stages (
                stage_id TEXT PRIMARY KEY,
                input_folder TEXT NOT NULL,
                output_folder TEXT NOT NULL,
                workflow_url TEXT NOT NULL,
                max_attempts INTEGER NOT NULL CHECK (max_attempts >= 1),
                retry_delay_sec INTEGER NOT NULL CHECK (retry_delay_sec >= 0),
                next_stage TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                archived_at TEXT,
                last_seen_in_config_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS entities (
                entity_id TEXT PRIMARY KEY,
                current_stage_id TEXT,
                current_status TEXT NOT NULL,
                latest_file_path TEXT,
                latest_file_id INTEGER,
                file_count INTEGER NOT NULL DEFAULT 0,
                validation_status TEXT NOT NULL DEFAULT 'valid',
                validation_errors_json TEXT NOT NULL DEFAULT '[]',
                first_seen_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS entity_files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id TEXT NOT NULL,
                stage_id TEXT NOT NULL,
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                checksum TEXT NOT NULL,
                file_mtime TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                meta_json TEXT NOT NULL DEFAULT '{}',
                current_stage TEXT,
                next_stage TEXT,
                status TEXT NOT NULL,
                validation_status TEXT NOT NULL,
                validation_errors_json TEXT NOT NULL DEFAULT '[]',
                is_managed_copy INTEGER NOT NULL DEFAULT 0,
                copy_source_file_id INTEGER,
                file_exists INTEGER NOT NULL DEFAULT 1,
                missing_since TEXT,
                first_seen_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id),
                FOREIGN KEY (copy_source_file_id) REFERENCES entity_files(id),
                UNIQUE(entity_id, stage_id)
            );

            CREATE TABLE IF NOT EXISTS entity_stage_states (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id TEXT NOT NULL,
                stage_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_instance_id INTEGER,
                file_exists INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                max_attempts INTEGER NOT NULL,
                last_error TEXT,
                last_http_status INTEGER,
                next_retry_at TEXT,
                last_started_at TEXT,
                last_finished_at TEXT,
                created_child_path TEXT,
                discovered_at TEXT NOT NULL,
                last_seen_at TEXT,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id),
                FOREIGN KEY (file_instance_id) REFERENCES entity_files(id),
                UNIQUE(entity_id, stage_id)
            );

            CREATE TABLE IF NOT EXISTS stage_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL UNIQUE,
                entity_id TEXT NOT NULL,
                entity_file_id INTEGER,
                stage_id TEXT NOT NULL,
                attempt_no INTEGER NOT NULL,
                workflow_url TEXT NOT NULL,
                request_json TEXT NOT NULL,
                response_json TEXT,
                http_status INTEGER,
                success INTEGER NOT NULL DEFAULT 0,
                error_type TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                duration_ms INTEGER,
                error_message TEXT,
                FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                FOREIGN KEY (entity_file_id) REFERENCES entity_files(id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id)
            );

            CREATE TABLE IF NOT EXISTS app_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                level TEXT NOT NULL,
                code TEXT NOT NULL,
                message TEXT NOT NULL,
                context_json TEXT,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_stage_runs_entity_id ON stage_runs(entity_id);
            CREATE INDEX IF NOT EXISTS idx_stage_runs_run_id ON stage_runs(run_id);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_status_retry ON entity_stage_states(status, next_retry_at);

            PRAGMA user_version = 4;
            "#,
        )
        .map_err(|error| format!("Failed to create SQLite schema v4: {error}"))?;
    Ok(())
}

fn migrate_v1_to_v2(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|error| format!("Failed to disable foreign keys for v1->v2 migration: {error}"))?;

    connection
        .execute_batch(
            r#"
            ALTER TABLE stages ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1;
            ALTER TABLE stages ADD COLUMN archived_at TEXT;
            ALTER TABLE stages ADD COLUMN last_seen_in_config_at TEXT;

            ALTER TABLE entities RENAME TO entities_v1_legacy;
            ALTER TABLE entity_stage_states RENAME TO entity_stage_states_v1_legacy;

            CREATE TABLE entities (
                entity_id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                file_name TEXT NOT NULL,
                stage_id TEXT NOT NULL,
                current_stage TEXT,
                next_stage TEXT,
                status TEXT NOT NULL,
                checksum TEXT NOT NULL,
                file_mtime TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                meta_json TEXT NOT NULL DEFAULT '{}',
                validation_status TEXT NOT NULL DEFAULT 'warning',
                validation_errors_json TEXT NOT NULL DEFAULT '[]',
                discovered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            INSERT INTO entities (
                entity_id,
                file_path,
                file_name,
                stage_id,
                current_stage,
                next_stage,
                status,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                validation_status,
                validation_errors_json,
                discovered_at,
                updated_at
            )
            SELECT
                entity_id,
                'legacy/' || entity_id || '.json',
                entity_id || '.json',
                COALESCE(current_stage, 'legacy'),
                current_stage,
                next_stage,
                status,
                '',
                created_at,
                0,
                payload_json,
                meta_json,
                'warning',
                '[]',
                created_at,
                updated_at
            FROM entities_v1_legacy;

            CREATE TABLE entity_stage_states (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id TEXT NOT NULL,
                stage_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                max_attempts INTEGER NOT NULL,
                last_error TEXT,
                last_http_status INTEGER,
                next_retry_at TEXT,
                last_started_at TEXT,
                last_finished_at TEXT,
                created_child_path TEXT,
                discovered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id),
                UNIQUE(entity_id, stage_id, file_path)
            );

            INSERT INTO entity_stage_states (
                id,
                entity_id,
                stage_id,
                file_path,
                status,
                attempts,
                max_attempts,
                last_error,
                last_http_status,
                next_retry_at,
                last_started_at,
                last_finished_at,
                created_child_path,
                discovered_at,
                updated_at
            )
            SELECT
                legacy.id,
                legacy.entity_id,
                legacy.stage_id,
                COALESCE(entity.file_path, 'legacy/' || legacy.entity_id || '.json'),
                legacy.status,
                legacy.attempts,
                COALESCE(stage.max_attempts, 1),
                legacy.last_error,
                NULL,
                NULL,
                NULL,
                NULL,
                NULL,
                legacy.created_at,
                legacy.updated_at
            FROM entity_stage_states_v1_legacy legacy
            LEFT JOIN entities entity ON entity.entity_id = legacy.entity_id
            LEFT JOIN stages stage ON stage.stage_id = legacy.stage_id;

            DROP TABLE entities_v1_legacy;
            DROP TABLE entity_stage_states_v1_legacy;

            PRAGMA user_version = 2;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v1 to v2: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v2",
        "SQLite schema migrated from version 1 to version 2.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "2", &now)?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| {
            format!("Failed to re-enable foreign keys after v1->v2 migration: {error}")
        })?;
    Ok(())
}

fn migrate_v2_to_v3(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|error| format!("Failed to disable foreign keys for v2->v3 migration: {error}"))?;

    connection
        .execute_batch(
            r#"
            ALTER TABLE entities RENAME TO entities_v2_legacy;
            ALTER TABLE entity_stage_states RENAME TO entity_stage_states_v2_legacy;

            CREATE TABLE entities (
                entity_id TEXT PRIMARY KEY,
                current_stage_id TEXT,
                current_status TEXT NOT NULL,
                latest_file_path TEXT,
                latest_file_id INTEGER,
                file_count INTEGER NOT NULL DEFAULT 0,
                validation_status TEXT NOT NULL DEFAULT 'valid',
                validation_errors_json TEXT NOT NULL DEFAULT '[]',
                first_seen_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE entity_files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id TEXT NOT NULL,
                stage_id TEXT NOT NULL,
                file_path TEXT NOT NULL UNIQUE,
                file_name TEXT NOT NULL,
                checksum TEXT NOT NULL,
                file_mtime TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                meta_json TEXT NOT NULL DEFAULT '{}',
                current_stage TEXT,
                next_stage TEXT,
                status TEXT NOT NULL,
                validation_status TEXT NOT NULL,
                validation_errors_json TEXT NOT NULL DEFAULT '[]',
                is_managed_copy INTEGER NOT NULL DEFAULT 0,
                copy_source_file_id INTEGER,
                file_exists INTEGER NOT NULL DEFAULT 1,
                missing_since TEXT,
                first_seen_at TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id),
                FOREIGN KEY (copy_source_file_id) REFERENCES entity_files(id),
                UNIQUE(entity_id, stage_id)
            );

            INSERT INTO entities (
                entity_id,
                current_stage_id,
                current_status,
                latest_file_path,
                latest_file_id,
                file_count,
                validation_status,
                validation_errors_json,
                first_seen_at,
                last_seen_at,
                updated_at
            )
            SELECT
                entity_id,
                stage_id,
                status,
                file_path,
                NULL,
                1,
                validation_status,
                validation_errors_json,
                discovered_at,
                updated_at,
                updated_at
            FROM entities_v2_legacy;

            INSERT INTO entity_files (
                entity_id,
                stage_id,
                file_path,
                file_name,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                file_exists,
                missing_since,
                first_seen_at,
                last_seen_at,
                updated_at
            )
            SELECT
                entity_id,
                stage_id,
                file_path,
                file_name,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                0,
                NULL,
                1,
                NULL,
                discovered_at,
                updated_at,
                updated_at
            FROM entities_v2_legacy;

            UPDATE entities
            SET
                latest_file_id = (
                    SELECT file.id
                    FROM entity_files file
                    WHERE file.entity_id = entities.entity_id
                    ORDER BY file.last_seen_at DESC, file.updated_at DESC, file.id DESC
                    LIMIT 1
                ),
                file_count = (
                    SELECT COUNT(*)
                    FROM entity_files file
                    WHERE file.entity_id = entities.entity_id
                );

            CREATE TABLE entity_stage_states (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id TEXT NOT NULL,
                stage_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_instance_id INTEGER,
                file_exists INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                max_attempts INTEGER NOT NULL,
                last_error TEXT,
                last_http_status INTEGER,
                next_retry_at TEXT,
                last_started_at TEXT,
                last_finished_at TEXT,
                created_child_path TEXT,
                discovered_at TEXT NOT NULL,
                last_seen_at TEXT,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id),
                FOREIGN KEY (file_instance_id) REFERENCES entity_files(id),
                UNIQUE(entity_id, stage_id)
            );

            INSERT INTO entity_stage_states (
                id,
                entity_id,
                stage_id,
                file_path,
                file_instance_id,
                file_exists,
                status,
                attempts,
                max_attempts,
                last_error,
                last_http_status,
                next_retry_at,
                last_started_at,
                last_finished_at,
                created_child_path,
                discovered_at,
                last_seen_at,
                updated_at
            )
            SELECT
                legacy.id,
                legacy.entity_id,
                legacy.stage_id,
                legacy.file_path,
                file.id,
                1,
                legacy.status,
                legacy.attempts,
                legacy.max_attempts,
                legacy.last_error,
                legacy.last_http_status,
                legacy.next_retry_at,
                legacy.last_started_at,
                legacy.last_finished_at,
                legacy.created_child_path,
                legacy.discovered_at,
                legacy.updated_at,
                legacy.updated_at
            FROM entity_stage_states_v2_legacy legacy
            LEFT JOIN entity_files file
                ON file.entity_id = legacy.entity_id
               AND file.stage_id = legacy.stage_id
               AND file.file_path = legacy.file_path;

            DROP TABLE entities_v2_legacy;
            DROP TABLE entity_stage_states_v2_legacy;

            PRAGMA user_version = 3;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v2 to v3: {error}"))?;

    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start post-migration transaction: {error}"))?;
    recompute_entity_summaries(&transaction)?;
    let now = Utc::now().to_rfc3339();
    insert_app_event(
        &transaction,
        AppEventLevel::Info,
        "schema_migrated_to_v3",
        "SQLite schema migrated from version 2 to version 3.",
        None,
        &now,
    )?;
    set_setting(&transaction, "schema_version", "3", &now)?;
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit v2->v3 migration: {error}"))?;

    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| {
            format!("Failed to re-enable foreign keys after v2->v3 migration: {error}")
        })?;
    Ok(())
}

fn migrate_v3_to_v4(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|error| format!("Failed to disable foreign keys for v3->v4 migration: {error}"))?;

    connection
        .execute_batch(
            r#"
            ALTER TABLE stage_runs RENAME TO stage_runs_v3_legacy;

            CREATE TABLE stage_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL UNIQUE,
                entity_id TEXT NOT NULL,
                entity_file_id INTEGER,
                stage_id TEXT NOT NULL,
                attempt_no INTEGER NOT NULL,
                workflow_url TEXT NOT NULL,
                request_json TEXT NOT NULL,
                response_json TEXT,
                http_status INTEGER,
                success INTEGER NOT NULL DEFAULT 0,
                error_type TEXT,
                error_message TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                duration_ms INTEGER,
                FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                FOREIGN KEY (entity_file_id) REFERENCES entity_files(id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id)
            );

            INSERT INTO stage_runs (
                id,
                run_id,
                entity_id,
                entity_file_id,
                stage_id,
                attempt_no,
                workflow_url,
                request_json,
                response_json,
                http_status,
                success,
                error_type,
                error_message,
                started_at,
                finished_at,
                duration_ms
            )
            SELECT
                legacy.id,
                'legacy-' || legacy.id,
                COALESCE(legacy.entity_id, ''),
                NULL,
                legacy.stage_id,
                1,
                COALESCE(stage.workflow_url, ''),
                '{}',
                NULL,
                NULL,
                CASE WHEN legacy.status = 'done' THEN 1 ELSE 0 END,
                CASE WHEN legacy.error_message IS NULL THEN NULL ELSE 'legacy' END,
                legacy.error_message,
                legacy.started_at,
                legacy.finished_at,
                NULL
            FROM stage_runs_v3_legacy legacy
            LEFT JOIN stages stage ON stage.stage_id = legacy.stage_id;

            DROP TABLE stage_runs_v3_legacy;

            CREATE INDEX IF NOT EXISTS idx_stage_runs_entity_id ON stage_runs(entity_id);
            CREATE INDEX IF NOT EXISTS idx_stage_runs_run_id ON stage_runs(run_id);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_status_retry ON entity_stage_states(status, next_retry_at);

            PRAGMA user_version = 4;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v3 to v4: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v4",
        "SQLite schema migrated from version 3 to version 4.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "4", &now)?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| {
            format!("Failed to re-enable foreign keys after v3->v4 migration: {error}")
        })?;
    Ok(())
}

fn sync_stages(connection: &mut Connection, stages: &[StageDefinition]) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start stage sync transaction: {error}"))?;

    let incoming_ids = stages
        .iter()
        .map(|stage| stage.id.clone())
        .collect::<HashSet<_>>();
    let existing_ids = load_existing_stage_ids(&transaction)?;

    for stage in stages {
        transaction
            .execute(
                r#"
                INSERT INTO stages (
                    stage_id,
                    input_folder,
                    output_folder,
                    workflow_url,
                    max_attempts,
                    retry_delay_sec,
                    next_stage,
                    is_active,
                    archived_at,
                    last_seen_in_config_at,
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, NULL, ?8, ?8, ?8)
                ON CONFLICT(stage_id) DO UPDATE SET
                    input_folder = excluded.input_folder,
                    output_folder = excluded.output_folder,
                    workflow_url = excluded.workflow_url,
                    max_attempts = excluded.max_attempts,
                    retry_delay_sec = excluded.retry_delay_sec,
                    next_stage = excluded.next_stage,
                    is_active = 1,
                    archived_at = NULL,
                    last_seen_in_config_at = excluded.last_seen_in_config_at,
                    updated_at = excluded.updated_at
                "#,
                params![
                    stage.id,
                    stage.input_folder,
                    stage.output_folder,
                    stage.workflow_url,
                    stage.max_attempts as i64,
                    stage.retry_delay_sec as i64,
                    stage.next_stage,
                    now,
                ],
            )
            .map_err(|error| format!("Failed to upsert stage '{}': {error}", stage.id))?;
    }

    for existing_id in existing_ids {
        if incoming_ids.contains(&existing_id) {
            continue;
        }

        let was_active = transaction
            .query_row(
                "SELECT is_active FROM stages WHERE stage_id = ?1",
                params![existing_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| {
                format!(
                    "Failed to load stage lifecycle for stage '{}': {error}",
                    existing_id
                )
            })?
            == 1;

        transaction
            .execute(
                r#"
                UPDATE stages
                SET is_active = 0,
                    archived_at = COALESCE(archived_at, ?2),
                    updated_at = ?2
                WHERE stage_id = ?1
                "#,
                params![existing_id, now],
            )
            .map_err(|error| format!("Failed to deactivate stage '{}': {error}", existing_id))?;

        if was_active {
            insert_app_event(
                &transaction,
                AppEventLevel::Info,
                "stage_deactivated",
                &format!(
                    "Stage '{}' is no longer present in pipeline.yaml and was marked inactive.",
                    existing_id
                ),
                Some(json!({ "stage_id": existing_id })),
                &now,
            )?;
        }
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit stage sync transaction: {error}"))?;
    Ok(())
}

fn load_existing_stage_ids(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare("SELECT stage_id FROM stages ORDER BY stage_id")
        .map_err(|error| format!("Failed to prepare stage id query: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Failed to query stage ids: {error}"))?;

    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|error| format!("Failed to read stage id row: {error}"))?);
    }
    Ok(ids)
}

fn load_stage_records_from_connection(connection: &Connection) -> Result<Vec<StageRecord>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                stage.stage_id,
                stage.input_folder,
                stage.output_folder,
                stage.workflow_url,
                stage.max_attempts,
                stage.retry_delay_sec,
                stage.next_stage,
                stage.is_active,
                stage.archived_at,
                stage.last_seen_in_config_at,
                stage.created_at,
                stage.updated_at,
                COUNT(DISTINCT file.entity_id) AS entity_count
            FROM stages stage
            LEFT JOIN entity_files file ON file.stage_id = stage.stage_id AND file.file_exists = 1
            GROUP BY
                stage.stage_id,
                stage.input_folder,
                stage.output_folder,
                stage.workflow_url,
                stage.max_attempts,
                stage.retry_delay_sec,
                stage.next_stage,
                stage.is_active,
                stage.archived_at,
                stage.last_seen_in_config_at,
                stage.created_at,
                stage.updated_at
            ORDER BY stage.stage_id
            "#,
        )
        .map_err(|error| format!("Failed to prepare stage query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(StageRecord {
                id: row.get(0)?,
                input_folder: row.get(1)?,
                output_folder: row.get(2)?,
                workflow_url: row.get(3)?,
                max_attempts: row.get::<_, i64>(4)? as u64,
                retry_delay_sec: row.get::<_, i64>(5)? as u64,
                next_stage: row.get(6)?,
                is_active: row.get::<_, i64>(7)? == 1,
                archived_at: row.get(8)?,
                last_seen_in_config_at: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                entity_count: row.get::<_, i64>(12)? as u64,
            })
        })
        .map_err(|error| format!("Failed to query stages: {error}"))?;

    let mut stages = Vec::new();
    for row in rows {
        stages.push(row.map_err(|error| format!("Failed to read stage row: {error}"))?);
    }
    Ok(stages)
}

#[allow(dead_code)]
fn load_entities_from_connection(connection: &Connection) -> Result<Vec<EntityRecord>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                entity_id,
                current_stage_id,
                current_status,
                latest_file_path,
                latest_file_id,
                file_count,
                validation_status,
                validation_errors_json,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entities
            ORDER BY updated_at DESC, entity_id
            "#,
        )
        .map_err(|error| format!("Failed to prepare entity query: {error}"))?;
    let rows = statement
        .query_map([], entity_from_row)
        .map_err(|error| format!("Failed to query entities: {error}"))?;

    let mut entities = Vec::new();
    for row in rows {
        entities.push(row.map_err(|error| format!("Failed to read entity row: {error}"))?);
    }
    Ok(entities)
}

fn load_entity_files_from_connection(
    connection: &Connection,
    entity_id: Option<&str>,
) -> Result<Vec<EntityFileRecord>, String> {
    let mut statement = match entity_id {
        Some(_) => connection
            .prepare(entity_files_select_sql(Some(
                "WHERE entity_id = ?1 ORDER BY file_exists DESC, last_seen_at DESC, updated_at DESC, id DESC",
            )))
            .map_err(|error| format!("Failed to prepare entity-files query: {error}"))?,
        None => connection
            .prepare(entity_files_select_sql(Some(
                "ORDER BY stage_id, file_exists DESC, last_seen_at DESC, updated_at DESC, id DESC",
            )))
            .map_err(|error| format!("Failed to prepare entity-files query: {error}"))?,
    };

    let rows = match entity_id {
        Some(entity_id) => statement
            .query_map(params![entity_id], entity_file_from_row)
            .map_err(|error| format!("Failed to query entity files for '{entity_id}': {error}"))?,
        None => statement
            .query_map([], entity_file_from_row)
            .map_err(|error| format!("Failed to query entity files: {error}"))?,
    };

    let mut files = Vec::new();
    for row in rows {
        files.push(row.map_err(|error| format!("Failed to read entity-file row: {error}"))?);
    }
    Ok(files)
}

fn entity_files_select_sql(filter: Option<&str>) -> &'static str {
    match filter {
        Some(
            "WHERE file_path = ?1",
        ) => {
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_name,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                file_exists,
                missing_since,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entity_files
            WHERE file_path = ?1
            "#
        }
        Some(
            "WHERE id = ?1",
        ) => {
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_name,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                file_exists,
                missing_since,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entity_files
            WHERE id = ?1
            "#
        }
        Some(
            "WHERE entity_id = ?1 AND stage_id = ?2 ORDER BY updated_at DESC, id DESC LIMIT 1",
        ) => {
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_name,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                file_exists,
                missing_since,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entity_files
            WHERE entity_id = ?1 AND stage_id = ?2
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#
        }
        Some(
            "WHERE entity_id = ?1 ORDER BY file_exists DESC, last_seen_at DESC, updated_at DESC, id DESC LIMIT 1",
        ) => {
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_name,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                file_exists,
                missing_since,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entity_files
            WHERE entity_id = ?1
            ORDER BY file_exists DESC, last_seen_at DESC, updated_at DESC, id DESC
            LIMIT 1
            "#
        }
        Some(
            "WHERE entity_id = ?1 ORDER BY file_exists DESC, last_seen_at DESC, updated_at DESC, id DESC",
        ) => {
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_name,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                file_exists,
                missing_since,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entity_files
            WHERE entity_id = ?1
            ORDER BY file_exists DESC, last_seen_at DESC, updated_at DESC, id DESC
            "#
        }
        Some(
            "ORDER BY stage_id, file_exists DESC, last_seen_at DESC, updated_at DESC, id DESC",
        ) => {
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_name,
                checksum,
                file_mtime,
                file_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                file_exists,
                missing_since,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entity_files
            ORDER BY stage_id, file_exists DESC, last_seen_at DESC, updated_at DESC, id DESC
            "#
        }
        _ => unreachable!("unexpected entity-files SQL variant"),
    }
}

fn load_stage_states_for_entity(
    connection: &Connection,
    entity_id: &str,
) -> Result<Vec<EntityStageStateRecord>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_instance_id,
                file_exists,
                status,
                attempts,
                max_attempts,
                last_error,
                last_http_status,
                next_retry_at,
                last_started_at,
                last_finished_at,
                created_child_path,
                discovered_at,
                last_seen_at,
                updated_at
            FROM entity_stage_states
            WHERE entity_id = ?1
            ORDER BY updated_at DESC, id DESC
            "#,
        )
        .map_err(|error| format!("Failed to prepare stage-state query: {error}"))?;
    let rows = statement
        .query_map(params![entity_id], |row| {
            Ok(EntityStageStateRecord {
                id: row.get(0)?,
                entity_id: row.get(1)?,
                stage_id: row.get(2)?,
                file_path: row.get(3)?,
                file_instance_id: row.get(4)?,
                file_exists: row.get::<_, i64>(5)? == 1,
                status: row.get(6)?,
                attempts: row.get::<_, i64>(7)? as u64,
                max_attempts: row.get::<_, i64>(8)? as u64,
                last_error: row.get(9)?,
                last_http_status: row.get(10)?,
                next_retry_at: row.get(11)?,
                last_started_at: row.get(12)?,
                last_finished_at: row.get(13)?,
                created_child_path: row.get(14)?,
                discovered_at: row.get(15)?,
                last_seen_at: row.get(16)?,
                updated_at: row.get(17)?,
            })
        })
        .map_err(|error| format!("Failed to query stage states for '{entity_id}': {error}"))?;

    let mut states = Vec::new();
    for row in rows {
        states.push(row.map_err(|error| format!("Failed to read stage-state row: {error}"))?);
    }
    Ok(states)
}

fn load_app_events_from_connection(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<AppEventRecord>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT id, level, code, message, context_json, created_at
            FROM app_events
            ORDER BY created_at DESC, id DESC
            LIMIT ?1
            "#,
        )
        .map_err(|error| format!("Failed to prepare app event query: {error}"))?;
    let rows = statement
        .query_map(params![limit as i64], |row| {
            let context_json: Option<String> = row.get(4)?;
            let context = context_json
                .as_deref()
                .map(parse_json_value)
                .transpose()
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                    )
                })?;

            Ok(AppEventRecord {
                id: row.get(0)?,
                level: parse_app_event_level(&row.get::<_, String>(1)?)?,
                code: row.get(2)?,
                message: row.get(3)?,
                context,
                created_at: row.get(5)?,
            })
        })
        .map_err(|error| format!("Failed to query app events: {error}"))?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row.map_err(|error| format!("Failed to read app event row: {error}"))?);
    }
    Ok(events)
}

fn load_status_counts(connection: &Connection) -> Result<Vec<StatusCount>, String> {
    let mut statement = connection
        .prepare("SELECT current_status, COUNT(*) FROM entities GROUP BY current_status ORDER BY current_status")
        .map_err(|error| format!("Failed to prepare status count query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(StatusCount {
                status: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
            })
        })
        .map_err(|error| format!("Failed to query entity status counts: {error}"))?;

    let mut counts = Vec::new();
    for row in rows {
        counts.push(row.map_err(|error| format!("Failed to read status count row: {error}"))?);
    }
    Ok(counts)
}

fn load_execution_status_counts(connection: &Connection) -> Result<Vec<StatusCount>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT status, COUNT(*)
            FROM entity_stage_states
            GROUP BY status
            ORDER BY status
            "#,
        )
        .map_err(|error| format!("Failed to prepare execution status count query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(StatusCount {
                status: row.get(0)?,
                count: row.get::<_, i64>(1)? as u64,
            })
        })
        .map_err(|error| format!("Failed to query execution status counts: {error}"))?;

    let mut counts = Vec::new();
    for row in rows {
        counts.push(
            row.map_err(|error| format!("Failed to read execution status count row: {error}"))?,
        );
    }
    Ok(counts)
}

fn load_entity_ids(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare("SELECT entity_id FROM entities ORDER BY entity_id")
        .map_err(|error| format!("Failed to prepare entity id query: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Failed to query entity ids: {error}"))?;

    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|error| format!("Failed to read entity id row: {error}"))?);
    }
    Ok(ids)
}

fn build_json_preview(file: Option<&EntityFileRecord>) -> Result<String, String> {
    let Some(file) = file else {
        return Ok("{}".to_string());
    };
    let payload = parse_json_value(&file.payload_json)?;
    let meta = parse_json_value(&file.meta_json)?;
    serialize_json_pretty(&json!({
        "id": file.entity_id,
        "current_stage": file.current_stage,
        "next_stage": file.next_stage,
        "status": file.status,
        "payload": payload,
        "meta": meta,
    }))
}

fn build_full_file_json(file: &EntityFileRecord) -> Result<String, String> {
    build_json_preview(Some(file))
}

fn build_entity_timeline(
    connection: &Connection,
    stage_states: &[EntityStageStateRecord],
) -> Result<Vec<EntityTimelineItem>, String> {
    let stages = load_stage_records_from_connection(connection)?;
    let ordered_stage_ids = order_stage_ids_for_timeline(&stages);
    let mut states_by_stage = HashMap::<String, &EntityStageStateRecord>::new();
    for state in stage_states {
        states_by_stage.insert(state.stage_id.clone(), state);
    }

    let mut timeline = Vec::new();
    let mut emitted = HashSet::new();
    for stage_id in ordered_stage_ids {
        if let Some(state) = states_by_stage.get(&stage_id) {
            timeline.push(timeline_item_from_state(state));
            emitted.insert(stage_id);
        }
    }
    let mut historical = stage_states
        .iter()
        .filter(|state| !emitted.contains(&state.stage_id))
        .collect::<Vec<_>>();
    historical.sort_by(|left, right| {
        left.stage_id
            .cmp(&right.stage_id)
            .then_with(|| left.updated_at.cmp(&right.updated_at))
    });
    for state in historical {
        timeline.push(timeline_item_from_state(state));
    }
    Ok(timeline)
}

fn order_stage_ids_for_timeline(stages: &[StageRecord]) -> Vec<String> {
    let stage_ids = stages
        .iter()
        .map(|stage| stage.id.clone())
        .collect::<HashSet<_>>();
    let targeted = stages
        .iter()
        .filter_map(|stage| stage.next_stage.clone())
        .collect::<HashSet<_>>();
    let mut roots = stages
        .iter()
        .filter(|stage| !targeted.contains(&stage.id))
        .map(|stage| stage.id.clone())
        .collect::<Vec<_>>();
    roots.sort();

    let next_by_stage = stages
        .iter()
        .filter_map(|stage| {
            stage
                .next_stage
                .as_ref()
                .map(|next| (stage.id.clone(), next.clone()))
        })
        .collect::<HashMap<_, _>>();
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();

    for root in roots {
        let mut current = Some(root);
        while let Some(stage_id) = current {
            if !seen.insert(stage_id.clone()) {
                break;
            }
            ordered.push(stage_id.clone());
            current = next_by_stage
                .get(&stage_id)
                .filter(|next| stage_ids.contains(*next))
                .cloned();
        }
    }

    let mut remaining = stages
        .iter()
        .map(|stage| stage.id.clone())
        .filter(|stage_id| !seen.contains(stage_id))
        .collect::<Vec<_>>();
    remaining.sort();
    ordered.extend(remaining);
    ordered
}

fn timeline_item_from_state(state: &EntityStageStateRecord) -> EntityTimelineItem {
    EntityTimelineItem {
        stage_id: state.stage_id.clone(),
        status: state.status.clone(),
        attempts: state.attempts,
        max_attempts: state.max_attempts,
        file_path: Some(state.file_path.clone()),
        file_exists: state.file_exists,
        last_error: state.last_error.clone(),
        last_http_status: state.last_http_status,
        next_retry_at: state.next_retry_at.clone(),
        last_started_at: state.last_started_at.clone(),
        last_finished_at: state.last_finished_at.clone(),
        created_child_path: state.created_child_path.clone(),
        updated_at: state.updated_at.clone(),
    }
}

fn ensure_state_transition(
    connection: &Connection,
    state_id: i64,
    to_status: &StageStatus,
    reason: RuntimeTransitionReason,
) -> Result<StageStateTransitionContext, String> {
    let context = connection
        .query_row(
            "SELECT status, entity_id, stage_id FROM entity_stage_states WHERE id = ?1",
            params![state_id],
            |row| {
                Ok(StageStateTransitionContext {
                    status: row.get(0)?,
                    entity_id: row.get(1)?,
                    stage_id: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Failed to load stage state '{state_id}': {error}"))?
        .ok_or_else(|| format!("Stage state '{state_id}' does not exist."))?;

    ensure_runtime_transition(
        &context.status,
        to_status,
        reason,
        Some(state_id),
        Some(&context.entity_id),
        Some(&context.stage_id),
    )?;
    Ok(context)
}

pub(crate) fn find_stage_state_identity(
    connection: &Connection,
    entity_id: &str,
    stage_id: &str,
) -> Result<Option<StageStateIdentity>, String> {
    connection
        .query_row(
            "SELECT id, status FROM entity_stage_states WHERE entity_id = ?1 AND stage_id = ?2",
            params![entity_id, stage_id],
            |row| {
                Ok(StageStateIdentity {
                    id: row.get(0)?,
                    status: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|error| {
            format!("Failed to load stage state for entity '{entity_id}' on stage '{stage_id}': {error}")
        })
}

fn ensure_runtime_transition(
    from_status: &str,
    to_status: &StageStatus,
    reason: RuntimeTransitionReason,
    state_id: Option<i64>,
    entity_id: Option<&str>,
    stage_id: Option<&str>,
) -> Result<(), String> {
    let from = parse_runtime_status(from_status).ok_or_else(|| {
        format!(
            "Unknown runtime status '{}' for state '{}'.",
            from_status,
            state_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )
    })?;
    validate_transition(&from, to_status, reason).map_err(|mut error| {
        error.state_id = state_id;
        error.entity_id = entity_id.map(ToOwned::to_owned);
        error.stage_id = stage_id.map(ToOwned::to_owned);
        error.message = format!(
            "{} state_id={:?}, entity_id={:?}, stage_id={:?}",
            error.message, error.state_id, error.entity_id, error.stage_id
        );
        error.to_string()
    })
}

fn update_entity_summary_from_state(
    connection: &Connection,
    state_id: i64,
    status: StageStatus,
    updated_at: &str,
) -> Result<(), String> {
    connection
        .execute(
            r#"
            UPDATE entities
            SET current_stage_id = (
                    SELECT stage_id FROM entity_stage_states WHERE id = ?1
                ),
                current_status = ?2,
                last_seen_at = ?3,
                updated_at = ?3
            WHERE entity_id = (
                SELECT entity_id FROM entity_stage_states WHERE id = ?1
            )
            "#,
            params![state_id, stage_status_value(&status), updated_at],
        )
        .map_err(|error| {
            format!("Failed to update logical entity summary from state '{state_id}': {error}")
        })?;
    Ok(())
}

fn runtime_task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuntimeTaskRecord> {
    Ok(RuntimeTaskRecord {
        state_id: row.get(0)?,
        entity_id: row.get(1)?,
        stage_id: row.get(2)?,
        status: row.get(3)?,
        attempts: row.get::<_, i64>(4)? as u64,
        max_attempts: row.get::<_, i64>(5)? as u64,
        file_path: row.get(6)?,
        file_instance_id: row.get::<_, Option<i64>>(7)?.unwrap_or_default(),
        file_exists: row.get::<_, i64>(8)? == 1,
        workflow_url: row.get(9)?,
        retry_delay_sec: row.get::<_, i64>(10)? as u64,
        next_stage: row.get(11)?,
    })
}

fn load_stage_runs_from_connection(
    connection: &Connection,
    entity_id: Option<&str>,
    limit: u32,
) -> Result<Vec<StageRunRecord>, String> {
    let sql = match entity_id {
        Some(_) => {
            r#"
            SELECT id, run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url,
                   request_json, response_json, http_status, success, error_type, error_message,
                   started_at, finished_at, duration_ms
            FROM stage_runs
            WHERE entity_id = ?1
            ORDER BY started_at DESC, id DESC
            LIMIT ?2
            "#
        }
        None => {
            r#"
            SELECT id, run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url,
                   request_json, response_json, http_status, success, error_type, error_message,
                   started_at, finished_at, duration_ms
            FROM stage_runs
            ORDER BY started_at DESC, id DESC
            LIMIT ?1
            "#
        }
    };
    let mut statement = connection
        .prepare(sql)
        .map_err(|error| format!("Failed to prepare stage runs query: {error}"))?;
    let rows = match entity_id {
        Some(entity_id) => statement
            .query_map(params![entity_id, limit as i64], stage_run_from_row)
            .map_err(|error| format!("Failed to query stage runs for '{entity_id}': {error}"))?,
        None => statement
            .query_map(params![limit as i64], stage_run_from_row)
            .map_err(|error| format!("Failed to query stage runs: {error}"))?,
    };

    let mut runs = Vec::new();
    for row in rows {
        runs.push(row.map_err(|error| format!("Failed to read stage run row: {error}"))?);
    }
    Ok(runs)
}

fn stage_run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StageRunRecord> {
    Ok(StageRunRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        entity_id: row.get(2)?,
        entity_file_id: row.get(3)?,
        stage_id: row.get(4)?,
        attempt_no: row.get::<_, i64>(5)? as u64,
        workflow_url: row.get(6)?,
        request_json: row.get(7)?,
        response_json: row.get(8)?,
        http_status: row.get(9)?,
        success: row.get::<_, i64>(10)? == 1,
        error_type: row.get(11)?,
        error_message: row.get(12)?,
        started_at: row.get(13)?,
        finished_at: row.get(14)?,
        duration_ms: row.get::<_, Option<i64>>(15)?.map(|value| value as u64),
    })
}

fn compare_file_records(left: &EntityFileRecord, right: &EntityFileRecord) -> std::cmp::Ordering {
    left.file_exists
        .cmp(&right.file_exists)
        .then_with(|| left.last_seen_at.cmp(&right.last_seen_at))
        .then_with(|| left.updated_at.cmp(&right.updated_at))
        .then_with(|| left.id.cmp(&right.id))
}

fn validation_rank(status: &EntityValidationStatus) -> u8 {
    match status {
        EntityValidationStatus::Valid => 0,
        EntityValidationStatus::Warning => 1,
        EntityValidationStatus::Invalid => 2,
    }
}

fn validation_status_from_rank(rank: u8) -> EntityValidationStatus {
    match rank {
        2 => EntityValidationStatus::Invalid,
        1 => EntityValidationStatus::Warning,
        _ => EntityValidationStatus::Valid,
    }
}

fn query_count<P>(connection: &Connection, sql: &str, params: P) -> Result<u64, String>
where
    P: rusqlite::Params,
{
    connection
        .query_row(sql, params, |row| row.get::<_, i64>(0))
        .map(|value| value as u64)
        .map_err(|error| format!("Failed to execute count query '{sql}': {error}"))
}

fn entity_table_sort_expression(sort_by: Option<&str>) -> &'static str {
    match sort_by {
        Some("entity_id") => "entity.entity_id",
        Some("current_stage") => "entity.current_stage_id",
        Some("status") => "COALESCE(state.status, entity.current_status)",
        Some("last_seen_at") => "entity.last_seen_at",
        Some("attempts") => "COALESCE(state.attempts, 0)",
        Some("last_error") => "COALESCE(state.last_error, '')",
        Some("updated_at") | None => "entity.updated_at",
        Some(_) => "entity.updated_at",
    }
}

fn entity_table_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityTableRow> {
    let payload_json: Option<String> = row.get(5)?;
    Ok(EntityTableRow {
        entity_id: row.get(0)?,
        display_name: entity_display_name_from_payload(payload_json.as_deref()),
        current_stage_id: row.get(1)?,
        current_status: row.get(2)?,
        latest_file_path: row.get(3)?,
        latest_file_id: row.get(4)?,
        file_count: row.get::<_, i64>(6)? as u64,
        attempts: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
        max_attempts: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
        last_error: row.get(9)?,
        last_http_status: row.get(10)?,
        next_retry_at: row.get(11)?,
        last_started_at: row.get(12)?,
        last_finished_at: row.get(13)?,
        validation_status: parse_validation_status(&row.get::<_, String>(14)?)?,
        updated_at: row.get(15)?,
        last_seen_at: row.get(16)?,
    })
}

fn entity_display_name_from_payload(payload_json: Option<&str>) -> Option<String> {
    let payload = payload_json?;
    let value = serde_json::from_str::<Value>(payload).ok()?;
    let name = value.get("entity_name")?.as_str()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn load_available_entity_statuses(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT status FROM (
                SELECT DISTINCT status FROM entity_stage_states
                UNION
                SELECT DISTINCT current_status AS status FROM entities
            )
            WHERE status IS NOT NULL AND TRIM(status) <> ''
            ORDER BY status ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare available-status query: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Failed to query available statuses: {error}"))?;
    let mut statuses = Vec::new();
    for row in rows {
        statuses.push(row.map_err(|error| format!("Failed to read status row: {error}"))?);
    }
    Ok(statuses)
}

fn entity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityRecord> {
    let validation_status = parse_validation_status(&row.get::<_, String>(6)?)?;
    let validation_errors_json: String = row.get(7)?;
    let validation_errors = parse_json::<Vec<ConfigValidationIssue>>(&validation_errors_json)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        })?;

    Ok(EntityRecord {
        entity_id: row.get(0)?,
        current_stage_id: row.get(1)?,
        current_status: row.get(2)?,
        latest_file_path: row.get(3)?,
        latest_file_id: row.get(4)?,
        file_count: row.get::<_, i64>(5)? as u64,
        validation_status,
        validation_errors,
        first_seen_at: row.get(8)?,
        last_seen_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn entity_file_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityFileRecord> {
    let validation_status = parse_validation_status(&row.get::<_, String>(13)?)?;
    let validation_errors_json: String = row.get(14)?;
    let validation_errors = parse_json::<Vec<ConfigValidationIssue>>(&validation_errors_json)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                14,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        })?;

    Ok(EntityFileRecord {
        id: row.get(0)?,
        entity_id: row.get(1)?,
        stage_id: row.get(2)?,
        file_path: row.get(3)?,
        file_name: row.get(4)?,
        checksum: row.get(5)?,
        file_mtime: row.get(6)?,
        file_size: row.get::<_, i64>(7)? as u64,
        payload_json: row.get(8)?,
        meta_json: row.get(9)?,
        current_stage: row.get(10)?,
        next_stage: row.get(11)?,
        status: row.get(12)?,
        validation_status,
        validation_errors,
        is_managed_copy: row.get::<_, i64>(15)? == 1,
        copy_source_file_id: row.get(16)?,
        file_exists: row.get::<_, i64>(17)? == 1,
        missing_since: row.get(18)?,
        first_seen_at: row.get(19)?,
        last_seen_at: row.get(20)?,
        updated_at: row.get(21)?,
    })
}

fn app_event_level_value(level: &AppEventLevel) -> &'static str {
    match level {
        AppEventLevel::Info => "info",
        AppEventLevel::Warning => "warning",
        AppEventLevel::Error => "error",
    }
}

fn parse_app_event_level(value: &str) -> rusqlite::Result<AppEventLevel> {
    match value {
        "info" => Ok(AppEventLevel::Info),
        "warning" => Ok(AppEventLevel::Warning),
        "error" => Ok(AppEventLevel::Error),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unknown app event level '{value}'"),
            )),
        )),
    }
}

fn validation_status_value(status: &EntityValidationStatus) -> &'static str {
    match status {
        EntityValidationStatus::Valid => "valid",
        EntityValidationStatus::Warning => "warning",
        EntityValidationStatus::Invalid => "invalid",
    }
}

fn parse_validation_status(value: &str) -> rusqlite::Result<EntityValidationStatus> {
    match value {
        "valid" => Ok(EntityValidationStatus::Valid),
        "warning" => Ok(EntityValidationStatus::Warning),
        "invalid" => Ok(EntityValidationStatus::Invalid),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unknown entity validation status '{value}'"),
            )),
        )),
    }
}

fn stage_status_value(status: &StageStatus) -> &'static str {
    runtime_status_value(status)
}

fn serialize_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("Failed to serialize JSON: {error}"))
}

fn serialize_json_pretty<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map_err(|error| format!("Failed to serialize pretty JSON: {error}"))
}

fn parse_json<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, String> {
    serde_json::from_str(value).map_err(|error| format!("Failed to parse JSON: {error}"))
}

fn parse_json_value(value: &str) -> Result<Value, String> {
    parse_json::<Value>(value)
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

pub(crate) fn system_time_to_rfc3339(value: std::time::SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339()
}

#[cfg(test)]
fn load_table_names(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|error| format!("Failed to prepare table list query: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Failed to query table list: {error}"))?;

    let mut names = Vec::new();
    for row in rows {
        names.push(row.map_err(|error| format!("Failed to read table name: {error}"))?);
    }

    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::scan_workspace;
    use crate::domain::{PipelineConfig, ProjectConfig, RuntimeConfig};
    use std::fs;

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

    fn stage(id: &str, next_stage: Option<&str>) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: format!("stages/{id}"),
            output_folder: format!("stages/{id}-out"),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: next_stage.map(ToOwned::to_owned),
        }
    }

    fn create_v1_schema(connection: &Connection) {
        connection
            .execute_batch(
                r#"
                PRAGMA foreign_keys = ON;

                CREATE TABLE settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE stages (
                    stage_id TEXT PRIMARY KEY,
                    input_folder TEXT NOT NULL,
                    output_folder TEXT NOT NULL,
                    workflow_url TEXT NOT NULL,
                    max_attempts INTEGER NOT NULL CHECK (max_attempts >= 1),
                    retry_delay_sec INTEGER NOT NULL CHECK (retry_delay_sec >= 0),
                    next_stage TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE entities (
                    entity_id TEXT PRIMARY KEY,
                    current_stage TEXT,
                    next_stage TEXT,
                    status TEXT NOT NULL,
                    payload_json TEXT NOT NULL DEFAULT '{}',
                    meta_json TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE entity_stage_states (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    entity_id TEXT NOT NULL,
                    stage_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    attempts INTEGER NOT NULL DEFAULT 0,
                    last_error TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                    FOREIGN KEY (stage_id) REFERENCES stages(stage_id)
                );

                CREATE TABLE stage_runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    entity_id TEXT,
                    stage_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    started_at TEXT NOT NULL,
                    finished_at TEXT,
                    error_message TEXT,
                    FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                    FOREIGN KEY (stage_id) REFERENCES stages(stage_id)
                );

                CREATE TABLE app_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    level TEXT NOT NULL,
                    code TEXT NOT NULL,
                    message TEXT NOT NULL,
                    context_json TEXT,
                    created_at TEXT NOT NULL
                );

                PRAGMA user_version = 1;
                "#,
            )
            .expect("create v1 schema");
    }

    fn create_v2_schema(connection: &Connection) {
        connection
            .execute_batch(
                r#"
                PRAGMA foreign_keys = ON;

                CREATE TABLE settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE stages (
                    stage_id TEXT PRIMARY KEY,
                    input_folder TEXT NOT NULL,
                    output_folder TEXT NOT NULL,
                    workflow_url TEXT NOT NULL,
                    max_attempts INTEGER NOT NULL CHECK (max_attempts >= 1),
                    retry_delay_sec INTEGER NOT NULL CHECK (retry_delay_sec >= 0),
                    next_stage TEXT,
                    is_active INTEGER NOT NULL DEFAULT 1,
                    archived_at TEXT,
                    last_seen_in_config_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE entities (
                    entity_id TEXT PRIMARY KEY,
                    file_path TEXT NOT NULL,
                    file_name TEXT NOT NULL,
                    stage_id TEXT NOT NULL,
                    current_stage TEXT,
                    next_stage TEXT,
                    status TEXT NOT NULL,
                    checksum TEXT NOT NULL,
                    file_mtime TEXT NOT NULL,
                    file_size INTEGER NOT NULL,
                    payload_json TEXT NOT NULL DEFAULT '{}',
                    meta_json TEXT NOT NULL DEFAULT '{}',
                    validation_status TEXT NOT NULL DEFAULT 'valid',
                    validation_errors_json TEXT NOT NULL DEFAULT '[]',
                    discovered_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE entity_stage_states (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    entity_id TEXT NOT NULL,
                    stage_id TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    status TEXT NOT NULL,
                    attempts INTEGER NOT NULL DEFAULT 0,
                    max_attempts INTEGER NOT NULL,
                    last_error TEXT,
                    last_http_status INTEGER,
                    next_retry_at TEXT,
                    last_started_at TEXT,
                    last_finished_at TEXT,
                    created_child_path TEXT,
                    discovered_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(entity_id, stage_id, file_path)
                );

                CREATE TABLE stage_runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    entity_id TEXT,
                    stage_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    started_at TEXT NOT NULL,
                    finished_at TEXT,
                    error_message TEXT
                );

                CREATE TABLE app_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    level TEXT NOT NULL,
                    code TEXT NOT NULL,
                    message TEXT NOT NULL,
                    context_json TEXT,
                    created_at TEXT NOT NULL
                );

                PRAGMA user_version = 2;
                "#,
            )
            .expect("create v2 schema");
    }

    #[test]
    fn bootstrap_creates_database_file_and_required_tables_at_v4() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config(vec![stage("ingest", Some("normalize"))]);

        let result = bootstrap_database(&database_path, &config).expect("bootstrap");
        let connection = Connection::open(&database_path).expect("open db");
        let table_names = load_table_names(&connection).expect("table names");

        assert!(database_path.exists());
        assert_eq!(result.schema_version, 4);
        assert!(table_names.contains(&"entity_files".to_string()));
        assert!(table_names.contains(&"entities".to_string()));
        assert!(table_names.contains(&"entity_stage_states".to_string()));
    }

    #[test]
    fn existing_v2_database_is_migrated_to_v4() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let connection = Connection::open(&database_path).expect("open db");
        create_v2_schema(&connection);
        let now = Utc::now().to_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO stages (
                    stage_id, input_folder, output_folder, workflow_url, max_attempts, retry_delay_sec,
                    next_stage, is_active, archived_at, last_seen_in_config_at, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, 3, 10, ?5, 1, NULL, ?6, ?6, ?6)
                "#,
                params!["ingest", "stages/ingest", "stages/out", "http://localhost/workflow", "normalize", &now],
            )
            .expect("seed stage");
        connection
            .execute(
                r#"
                INSERT INTO entities (
                    entity_id, file_path, file_name, stage_id, current_stage, next_stage, status,
                    checksum, file_mtime, file_size, payload_json, meta_json, validation_status,
                    validation_errors_json, discovered_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?4, ?5, 'pending', 'abc', ?6, 12, '{}', '{}', 'valid', '[]', ?6, ?6)
                "#,
                params!["entity-1", "stages/ingest/entity-1.json", "entity-1.json", "ingest", "normalize", &now],
            )
            .expect("seed entity");
        connection
            .execute(
                r#"
                INSERT INTO entity_stage_states (
                    entity_id, stage_id, file_path, status, attempts, max_attempts, last_error,
                    last_http_status, next_retry_at, last_started_at, last_finished_at, created_child_path,
                    discovered_at, updated_at
                )
                VALUES (?1, ?2, ?3, 'pending', 0, 3, NULL, NULL, NULL, NULL, NULL, NULL, ?4, ?4)
                "#,
                params!["entity-1", "ingest", "stages/ingest/entity-1.json", &now],
            )
            .expect("seed stage state");
        drop(connection);

        let result = bootstrap_database(
            &database_path,
            &test_config(vec![stage("ingest", Some("normalize"))]),
        )
        .expect("bootstrap");
        let connection = Connection::open(&database_path).expect("open migrated db");
        let entity = find_entity_by_id(&connection, "entity-1")
            .expect("load entity")
            .expect("entity exists");
        let files =
            load_entity_files_from_connection(&connection, Some("entity-1")).expect("load files");
        let events = load_app_events_from_connection(&connection, 20).expect("events");

        assert_eq!(result.schema_version, 4);
        assert_eq!(entity.file_count, 1);
        assert_eq!(files.len(), 1);
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v3"));
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v4"));
    }

    #[test]
    fn v1_database_can_bootstrap_through_v4() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let connection = Connection::open(&database_path).expect("open db");
        create_v1_schema(&connection);
        drop(connection);

        let result = bootstrap_database(&database_path, &test_config(vec![stage("ingest", None)]))
            .expect("bootstrap");

        assert_eq!(result.schema_version, 4);
    }

    #[test]
    fn removed_stage_is_marked_inactive_not_deleted() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");

        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("ingest", Some("normalize")),
                stage("normalize", None),
            ]),
        )
        .expect("bootstrap one");
        let result =
            bootstrap_database(&database_path, &test_config(vec![stage("normalize", None)]))
                .expect("bootstrap two");
        let connection = Connection::open(&database_path).expect("open db");
        let stages = load_stage_records_from_connection(&connection).expect("stages");
        let ingest = stages
            .into_iter()
            .find(|stage| stage.id == "ingest")
            .expect("ingest stage");

        assert_eq!(result.active_stage_count, 1);
        assert!(!ingest.is_active);
        assert!(ingest.archived_at.is_some());
    }

    #[test]
    fn database_transition_wrapper_rejects_invalid_status_regression() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir.join("stages").join("ingest").join("entity-1.json");
        bootstrap_database(&database_path, &test_config(vec![stage("ingest", None)]))
            .expect("bootstrap");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("parent");
        fs::write(
            &source_path,
            r#"{"id":"entity-1","payload":{"ok":true},"status":"pending"}"#,
        )
        .expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");
        let connection = Connection::open(&database_path).expect("open db");
        let state = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists")
            .stage_states
            .into_iter()
            .find(|state| state.stage_id == "ingest")
            .expect("state");

        let result = update_stage_state_success(
            &connection,
            state.id,
            Some(200),
            &Utc::now().to_rfc3339(),
            None,
        );
        let after = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists")
            .stage_states
            .into_iter()
            .find(|state| state.stage_id == "ingest")
            .expect("state");

        assert!(result.is_err());
        assert!(result
            .err()
            .expect("error")
            .contains("Invalid runtime transition"));
        assert_eq!(after.status, "pending");
    }

    #[test]
    fn entity_table_query_filters_sorts_and_paginates_in_sql() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("incoming", Some("normalize")),
                stage("normalize", None),
            ]),
        )
        .expect("bootstrap");
        let incoming = workdir.join("stages/incoming");
        fs::create_dir_all(&incoming).expect("incoming");
        fs::write(
            incoming.join("entity-a.json"),
            r#"{"id":"entity-a","payload":{"entity_name":"alpha","value":1},"status":"pending"}"#,
        )
        .expect("entity a");
        fs::write(
            incoming.join("entity-b.json"),
            r#"{"id":"entity-b","payload":{"entity_name":"керамика","value":2},"status":"pending"}"#,
        )
        .expect("entity b");
        scan_workspace(&workdir, &database_path).expect("scan");
        let connection = Connection::open(&database_path).expect("open db");
        connection
            .execute(
                r#"
                UPDATE entity_stage_states
                SET status = 'failed', attempts = 2, last_error = 'network failed', last_http_status = 500
                WHERE entity_id = 'entity-b'
                "#,
                [],
            )
            .expect("update state");
        connection
            .execute(
                "UPDATE entities SET current_status = 'failed' WHERE entity_id = 'entity-b'",
                [],
            )
            .expect("update entity");
        drop(connection);

        let filtered = list_entity_table_page(
            &database_path,
            &EntityListQuery {
                search: Some("керамика".to_string()),
                status: Some("failed".to_string()),
                sort_by: Some("attempts".to_string()),
                sort_direction: Some("desc".to_string()),
                page: Some(1),
                page_size: Some(10),
                ..EntityListQuery::default()
            },
        )
        .expect("filtered page");
        let paged = list_entity_table_page(
            &database_path,
            &EntityListQuery {
                sort_by: Some("entity_id".to_string()),
                sort_direction: Some("asc".to_string()),
                page: Some(2),
                page_size: Some(1),
                ..EntityListQuery::default()
            },
        )
        .expect("paged");

        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.entities[0].entity_id, "entity-b");
        assert_eq!(
            filtered.entities[0].display_name.as_deref(),
            Some("керамика")
        );
        assert_eq!(filtered.entities[0].attempts, Some(2));
        assert_eq!(
            filtered.entities[0].last_error.as_deref(),
            Some("network failed")
        );
        assert_eq!(paged.total, 2);
        assert_eq!(paged.page, 2);
        assert_eq!(paged.entities.len(), 1);
        assert_eq!(paged.entities[0].entity_id, "entity-b");
    }

    #[test]
    fn entity_detail_includes_runs_timeline_selected_json_and_allowed_actions() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("incoming", Some("normalize")),
                stage("normalize", None),
            ]),
        )
        .expect("bootstrap");
        let source_path = workdir.join("stages/incoming/entity-1.json");
        fs::create_dir_all(source_path.parent().expect("parent")).expect("parent");
        fs::write(
            &source_path,
            r#"{"id":"entity-1","payload":{"value":1},"meta":{"source":"test"},"status":"pending"}"#,
        )
        .expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");
        let connection = Connection::open(&database_path).expect("open db");
        let file = load_entity_files_from_connection(&connection, Some("entity-1"))
            .expect("files")
            .remove(0);
        connection
            .execute(
                r#"
                INSERT INTO stage_runs (
                    run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url,
                    request_json, response_json, http_status, success, error_type, error_message,
                    started_at, finished_at, duration_ms
                )
                VALUES ('run-1', 'entity-1', ?1, 'incoming', 1, 'http://localhost',
                        '{"request":true}', '{"response":true}', 200, 1, NULL, NULL, ?2, ?2, 4)
                "#,
                params![file.id, Utc::now().to_rfc3339()],
            )
            .expect("run");
        drop(connection);

        let detail = get_entity_detail_with_selection(&database_path, "entity-1", Some(file.id))
            .expect("detail")
            .expect("exists");

        assert_eq!(detail.files.len(), 1);
        assert_eq!(detail.stage_states.len(), 1);
        assert_eq!(detail.stage_runs.len(), 1);
        assert_eq!(detail.timeline[0].stage_id, "incoming");
        assert!(detail
            .selected_file_json
            .as_deref()
            .expect("selected json")
            .contains("\"payload\""));
        let actions = detail
            .allowed_actions
            .iter()
            .find(|action| action.stage_id == "incoming")
            .expect("actions");
        assert!(actions.can_retry_now);
        assert!(actions.can_skip);
        let file_actions = detail
            .file_allowed_actions
            .iter()
            .find(|action| action.entity_file_id == file.id)
            .expect("file actions");
        assert!(file_actions.can_edit_business_json);
        assert!(file_actions.can_open_file);
    }

    #[test]
    fn workspace_explorer_fresh_workdir_returns_stage_tree_with_zero_counters() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("incoming", Some("done")), stage("done", None)]),
        )
        .expect("bootstrap");

        let explorer = get_workspace_explorer(&workdir, &database_path).expect("explorer");

        assert_eq!(explorer.workdir_path, path_string(&workdir));
        assert_eq!(explorer.totals.stages_total, 2);
        assert_eq!(explorer.totals.registered_files_total, 0);
        assert_eq!(explorer.stages.len(), 2);
        assert!(explorer.stages.iter().all(|stage| stage.files.is_empty()));
        assert!(explorer
            .stages
            .iter()
            .all(|stage| stage.counters.registered_files == 0));
    }

    #[test]
    fn workspace_explorer_links_present_missing_invalid_inactive_and_terminal_data() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let mut terminal = stage("terminal", None);
        terminal.output_folder = String::new();
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("incoming", Some("normalize")),
                stage("normalize", Some("terminal")),
                terminal.clone(),
            ]),
        )
        .expect("bootstrap");

        let incoming_path = workdir.join("stages/incoming/entity-1.json");
        fs::create_dir_all(incoming_path.parent().expect("parent")).expect("incoming parent");
        fs::write(
            &incoming_path,
            r#"{"id":"entity-1","current_stage":"incoming","next_stage":"normalize","payload":{"value":1},"status":"pending"}"#,
        )
        .expect("source");
        let normalize_path = workdir.join("stages/normalize/entity-1.json");
        fs::create_dir_all(normalize_path.parent().expect("parent")).expect("normalize parent");
        fs::write(
            &normalize_path,
            r#"{"id":"entity-1","current_stage":"normalize","next_stage":"terminal","payload":{"value":2},"meta":{"beehive":{"copy_source_stage":"incoming"}},"status":"pending"}"#,
        )
        .expect("managed target");
        scan_workspace(&workdir, &database_path).expect("initial scan");
        let connection = Connection::open(&database_path).expect("open db");
        let source_file_id: i64 = connection
            .query_row(
                "SELECT id FROM entity_files WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                [],
                |row| row.get(0),
            )
            .expect("source file id");
        connection
            .execute(
                r#"
                UPDATE entity_files
                SET is_managed_copy = 1, copy_source_file_id = ?1
                WHERE entity_id = 'entity-1' AND stage_id = 'normalize'
                "#,
                params![source_file_id],
            )
            .expect("mark managed");
        drop(connection);
        fs::remove_file(&incoming_path).expect("remove source");
        scan_workspace(&workdir, &database_path).expect("missing scan");
        let invalid_path = workdir.join("stages/normalize/bad.json");
        fs::write(&invalid_path, "{not-json").expect("invalid");
        scan_workspace(&workdir, &database_path).expect("invalid scan");
        let connection = Connection::open(&database_path).expect("open db");
        connection
            .execute(
                r#"
                UPDATE entity_files
                SET is_managed_copy = 1, copy_source_file_id = ?1
                WHERE entity_id = 'entity-1' AND stage_id = 'normalize'
                "#,
                params![source_file_id],
            )
            .expect("restore managed marker");
        drop(connection);
        bootstrap_database(&database_path, &test_config(vec![terminal])).expect("archive stages");

        let explorer = get_workspace_explorer(&workdir, &database_path).expect("explorer");
        let incoming = explorer
            .stages
            .iter()
            .find(|stage| stage.stage_id == "incoming")
            .expect("incoming stage");
        let normalize = explorer
            .stages
            .iter()
            .find(|stage| stage.stage_id == "normalize")
            .expect("normalize stage");
        let terminal_stage = explorer
            .stages
            .iter()
            .find(|stage| stage.stage_id == "terminal")
            .expect("terminal stage");
        let trail = explorer
            .entity_trails
            .iter()
            .find(|trail| trail.entity_id == "entity-1")
            .expect("entity trail");

        assert!(!incoming.is_active);
        assert!(!normalize.is_active);
        assert_eq!(terminal_stage.output_folder, None);
        assert!(incoming.files.iter().any(|file| {
            file.entity_id == "entity-1"
                && file.stage_id == "incoming"
                && file.runtime_status.as_deref() == Some("pending")
                && !file.file_exists
                && file.missing_since.is_some()
        }));
        assert!(normalize.files.iter().any(|file| {
            file.entity_id == "entity-1"
                && file.stage_id == "normalize"
                && file.is_managed_copy
                && file.copy_source_file_id.is_some()
                && file.copy_source_entity_id.as_deref() == Some("entity-1")
        }));
        assert_eq!(normalize.invalid_files.len(), 1);
        assert_eq!(normalize.invalid_files[0].code, "invalid_json_file");
        assert!(incoming.counters.missing_files >= 1);
        assert!(normalize.counters.managed_copies >= 1);
        assert!(trail.file_count >= 2);
        assert!(trail.stages.len() >= 2);
        assert!(trail
            .edges
            .iter()
            .any(|edge| edge.relation == "managed_copy"));
    }

    #[test]
    fn workspace_explorer_read_model_does_not_mutate_sqlite_state() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(&database_path, &test_config(vec![stage("incoming", None)]))
            .expect("bootstrap");
        let source_path = workdir.join("stages/incoming/entity-1.json");
        fs::create_dir_all(source_path.parent().expect("parent")).expect("parent");
        fs::write(
            &source_path,
            r#"{"id":"entity-1","payload":{"ok":true},"status":"pending"}"#,
        )
        .expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");
        let connection = Connection::open(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'done', attempts = 1 WHERE entity_id = 'entity-1'",
                [],
            )
            .expect("mark done");
        let before_status: String = connection
            .query_row(
                "SELECT status FROM entity_stage_states WHERE entity_id = 'entity-1'",
                [],
                |row| row.get(0),
            )
            .expect("before status");
        let before_runs =
            query_count(&connection, "SELECT COUNT(*) FROM stage_runs", []).expect("before runs");
        drop(connection);

        let explorer = get_workspace_explorer(&workdir, &database_path).expect("explorer");
        let connection = Connection::open(&database_path).expect("open db after");
        let after_status: String = connection
            .query_row(
                "SELECT status FROM entity_stage_states WHERE entity_id = 'entity-1'",
                [],
                |row| row.get(0),
            )
            .expect("after status");
        let after_runs =
            query_count(&connection, "SELECT COUNT(*) FROM stage_runs", []).expect("after runs");

        assert_eq!(before_status, "done");
        assert_eq!(after_status, "done");
        assert_eq!(before_runs, after_runs);
        assert!(explorer.stages.iter().any(|stage| {
            stage.files.iter().any(|file| {
                file.entity_id == "entity-1" && file.runtime_status.as_deref() == Some("done")
            })
        }));
    }

    #[test]
    fn manual_reset_and_skip_use_state_machine_and_write_events() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(&database_path, &test_config(vec![stage("incoming", None)]))
            .expect("bootstrap");
        let source_path = workdir.join("stages/incoming/entity-1.json");
        fs::create_dir_all(source_path.parent().expect("parent")).expect("parent");
        fs::write(&source_path, r#"{"id":"entity-1","payload":{"ok":true}}"#).expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");
        let connection = Connection::open(&database_path).expect("open db");
        connection
            .execute(
                r#"
                UPDATE entity_stage_states
                SET status = 'failed', attempts = 2, last_error = 'bad', last_http_status = 500
                WHERE entity_id = 'entity-1' AND stage_id = 'incoming'
                "#,
                [],
            )
            .expect("failed state");
        drop(connection);

        reset_entity_stage_to_pending(&database_path, "entity-1", "incoming", Some("retry later"))
            .expect("reset");
        let after_reset = get_entity_detail(&database_path, "entity-1")
            .expect("detail")
            .expect("exists")
            .stage_states
            .remove(0);
        assert_eq!(after_reset.status, "pending");
        assert_eq!(after_reset.attempts, 0);
        assert!(after_reset.last_error.is_none());
        assert!(after_reset.last_http_status.is_none());

        skip_entity_stage(&database_path, "entity-1", "incoming", Some("not needed"))
            .expect("skip");
        let after_skip = get_entity_detail(&database_path, "entity-1")
            .expect("detail")
            .expect("exists")
            .stage_states
            .remove(0);
        let events = list_app_events(&database_path, 20).expect("events");

        assert_eq!(after_skip.status, "skipped");
        assert!(events
            .iter()
            .any(|event| event.code == "manual_reset_to_pending"));
        assert!(events.iter().any(|event| event.code == "manual_skip"));
    }

    #[test]
    fn skip_rejects_active_in_progress_state() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(&database_path, &test_config(vec![stage("incoming", None)]))
            .expect("bootstrap");
        let source_path = workdir.join("stages/incoming/entity-1.json");
        fs::create_dir_all(source_path.parent().expect("parent")).expect("parent");
        fs::write(&source_path, r#"{"id":"entity-1","payload":{"ok":true}}"#).expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");
        let connection = Connection::open(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'in_progress' WHERE entity_id = 'entity-1'",
                [],
            )
            .expect("in progress");
        drop(connection);

        let error = skip_entity_stage(&database_path, "entity-1", "incoming", None)
            .expect_err("skip should reject in_progress");

        assert!(error.contains("Invalid runtime transition"));
    }
}

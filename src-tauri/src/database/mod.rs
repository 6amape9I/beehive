use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Duration, Utc};
use rusqlite::types::Value as SqlValue;
use rusqlite::{
    params, params_from_iter, Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction,
    TransactionBehavior,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::domain::{
    AppEventLevel, AppEventRecord, BulkResetEntityStagesPayload, ConfigValidationIssue,
    DatabaseState, EntityDetailPayload, EntityFileRecord, EntityFilters, EntityListQuery,
    EntityRecord, EntityStageStateRecord, EntityTableRow, EntityTimelineItem,
    EntityValidationStatus, InvalidDiscoveryRecord, OutputRegistrationReport,
    OutputRegistrationReportItem, PipelineConfig, ResourceClass, RuntimeSummary, SchedulingPolicy,
    StageDefinition, StageRecord, StageRunRecord, StageStatus, StatusCount, StorageProvider,
    UpdateEntityRequest, WorkerLeaseRecord, WorkerPoolRuntimeSummary, WorkerStateAnomalyCount,
    WorkerStateAnomalyRecord, WorkerSummary, WorkspaceEntityTrail, WorkspaceEntityTrailEdge,
    WorkspaceEntityTrailNode, WorkspaceExplorerResult, WorkspaceExplorerTotals, WorkspaceFileNode,
    WorkspaceStageTree, WorkspaceStageTreeCounters,
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

const SCHEMA_VERSION: u32 = 13;
const DEFAULT_SQLITE_BUSY_TIMEOUT_MS: u64 = 30_000;
const SQLITE_WRITE_RETRY_BACKOFF_MS: [u64; 5] = [50, 100, 200, 400, 800];
const DEFAULT_APP_EVENTS_MAX_ROWS: u64 = 100_000;
const DEFAULT_APP_EVENTS_RETENTION_DAYS: u64 = 14;
const DEFAULT_WORKER_LEASES_MAX_ROWS: u64 = 100_000;
const DEFAULT_WORKER_LEASES_RETENTION_DAYS: u64 = 14;
const DEFAULT_RUNTIME_RETENTION_DELETE_BATCH: u64 = 1_000;
const RUNTIME_RETENTION_WRITE_GATE_TIMEOUT_MS: u64 = 1_000;
const DEFAULT_S3_REGISTRATION_WRITE_BATCH_SIZE: u64 = 1;

static SQLITE_WRITE_GATES: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
static SQLITE_WRITE_GATE_OWNERS: OnceLock<Mutex<HashMap<String, SqliteWriteGateOwner>>> =
    OnceLock::new();

#[derive(Clone)]
struct SqliteWriteGateOwner {
    context: String,
    thread_name: Option<String>,
    started_at: Instant,
}

struct SqliteWriteGateOwnerGuard {
    key: String,
}

impl Drop for SqliteWriteGateOwnerGuard {
    fn drop(&mut self) {
        if let Some(owners) = SQLITE_WRITE_GATE_OWNERS.get() {
            owners
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&self.key);
        }
    }
}

pub(crate) struct PersistEntityFileInput {
    pub entity_id: String,
    pub stage_id: String,
    pub file_path: String,
    pub file_name: String,
    pub artifact_id: Option<String>,
    pub relation_to_source: Option<String>,
    pub storage_provider: StorageProvider,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub version_id: Option<String>,
    pub etag: Option<String>,
    pub checksum_sha256: Option<String>,
    pub checksum: String,
    pub file_mtime: String,
    pub file_size: u64,
    pub artifact_size: Option<u64>,
    pub payload_json: String,
    pub meta_json: String,
    pub current_stage: Option<String>,
    pub next_stage: Option<String>,
    pub status: StageStatus,
    pub validation_status: EntityValidationStatus,
    pub validation_errors: Vec<ConfigValidationIssue>,
    pub is_managed_copy: bool,
    pub copy_source_file_id: Option<i64>,
    pub producer_run_id: Option<String>,
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
    pub resource_class: ResourceClass,
    pub lease_id: Option<String>,
    pub worker_id: Option<String>,
}

pub(crate) struct WorkerLeaseClaimInput {
    pub resource_class: ResourceClass,
    pub limit: u64,
    pub worker_id: String,
    pub lease_ttl_sec: u64,
    pub scheduling_policy: SchedulingPolicy,
    pub now: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RuntimeRetentionPruneSummary {
    pub app_events_deleted: u64,
    pub worker_leases_deleted: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkerPoolControlState {
    pub desired_concurrency: u32,
    pub is_started: bool,
    pub is_paused: bool,
    pub pause_reason: Option<String>,
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

pub(crate) struct RegisterS3ArtifactPointerInput {
    pub entity_id: String,
    pub artifact_id: String,
    pub relation_to_source: Option<String>,
    pub stage_id: String,
    pub bucket: String,
    pub key: String,
    pub version_id: Option<String>,
    pub etag: Option<String>,
    pub checksum_sha256: Option<String>,
    pub size: Option<u64>,
    pub last_modified: Option<String>,
    pub source_file_id: Option<i64>,
    pub producer_run_id: Option<String>,
    pub status: StageStatus,
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
    sync_storage_settings(&connection, config)?;

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

pub(crate) fn verify_worker_runtime_database(path: &Path) -> Result<u32, String> {
    if !path.exists() {
        return Err(format!(
            "workspace_not_bootstrapped_for_workers: SQLite database '{}' does not exist; open or bootstrap the workspace before starting workers.",
            path.display()
        ));
    }
    let connection = open_readonly_connection(path)?;
    let schema_version = current_schema_version(&connection)?;
    if schema_version != SCHEMA_VERSION {
        return Err(format!(
            "workspace_not_bootstrapped_for_workers: SQLite schema version '{}' is not current '{}'; open or bootstrap the workspace before starting workers.",
            schema_version, SCHEMA_VERSION
        ));
    }
    Ok(schema_version)
}

fn sync_storage_settings(connection: &Connection, config: &PipelineConfig) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    set_setting(connection, "project_name", &config.project.name, &now)?;
    let provider = config
        .storage
        .as_ref()
        .map(|storage| storage.provider.as_str())
        .unwrap_or("local");
    let s3_config = config
        .storage
        .as_ref()
        .and_then(|storage| storage.s3_config());
    set_setting(connection, "storage_provider", provider, &now)?;
    set_setting(
        connection,
        "storage_bucket",
        s3_config
            .as_ref()
            .map(|storage| storage.bucket.as_str())
            .unwrap_or(""),
        &now,
    )?;
    set_setting(
        connection,
        "storage_workspace_prefix",
        s3_config
            .as_ref()
            .map(|storage| storage.workspace_prefix.as_str())
            .unwrap_or(""),
        &now,
    )?;
    Ok(())
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
    configure_sqlite_connection(&connection, true)?;
    Ok(connection)
}

fn configure_sqlite_connection(connection: &Connection, writable: bool) -> Result<(), String> {
    let busy_timeout_ms = sqlite_busy_timeout_ms_from_env();
    connection
        .execute_batch(&format!(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = {busy_timeout_ms};
            "#
        ))
        .map_err(|error| format!("Failed to configure SQLite connection pragmas: {error}"))?;
    if writable {
        connection
            .execute_batch(
                r#"
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
                "#,
            )
            .map_err(|error| format!("Failed to configure SQLite WAL pragmas: {error}"))?;
    }
    Ok(())
}

fn sqlite_busy_timeout_ms_from_env() -> u64 {
    parse_sqlite_busy_timeout_ms(
        std::env::var("BEEHIVE_SQLITE_BUSY_TIMEOUT_MS")
            .ok()
            .as_deref(),
    )
}

pub(crate) fn parse_sqlite_busy_timeout_ms(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SQLITE_BUSY_TIMEOUT_MS)
}

pub(crate) fn sqlite_error_is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked,
                ..
            },
            _
        )
    )
}

pub(crate) fn sqlite_message_is_busy(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("database is locked")
        || normalized.contains("database is busy")
        || normalized.contains("sqlite_busy")
        || normalized.contains("sqlite_locked")
}

pub(crate) fn retry_sqlite_busy_operation<T, F>(mut operation: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
{
    retry_sqlite_busy_operation_with_sleep(&mut operation, |delay| thread::sleep(delay))
}

pub(crate) fn retry_sqlite_busy_operation_with_sleep<T, F, S>(
    operation: &mut F,
    mut sleep: S,
) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
    S: FnMut(StdDuration),
{
    let mut attempt = 0;
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(message)
                if sqlite_message_is_busy(&message)
                    && attempt < SQLITE_WRITE_RETRY_BACKOFF_MS.len() =>
            {
                let delay = StdDuration::from_millis(SQLITE_WRITE_RETRY_BACKOFF_MS[attempt]);
                attempt += 1;
                sleep(delay);
            }
            Err(message) => return Err(message),
        }
    }
}

fn sqlite_write_gate_for_key(key: String) -> Result<Arc<Mutex<()>>, String> {
    let gates = SQLITE_WRITE_GATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut gates = gates
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(gates
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

fn sqlite_write_gate_key_for_path(path: &Path) -> Result<String, String> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(canonical.to_string_lossy().to_string());
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                format!("Failed to resolve current directory for SQLite gate: {error}")
            })?
            .join(path)
    };
    Ok(absolute.to_string_lossy().to_string())
}

fn sqlite_write_gate_key_for_connection(connection: &Connection) -> Result<String, String> {
    let mut statement = connection
        .prepare("PRAGMA database_list")
        .map_err(|error| format!("Failed to inspect SQLite database path: {error}"))?;
    let mut rows = statement
        .query([])
        .map_err(|error| format!("Failed to query SQLite database path: {error}"))?;
    while let Some(row) = rows
        .next()
        .map_err(|error| format!("Failed to read SQLite database path: {error}"))?
    {
        let name: String = row
            .get(1)
            .map_err(|error| format!("Failed to read SQLite database name: {error}"))?;
        let file: String = row
            .get(2)
            .map_err(|error| format!("Failed to read SQLite database file: {error}"))?;
        if name == "main" && !file.trim().is_empty() {
            return sqlite_write_gate_key_for_path(Path::new(&file));
        }
    }
    Ok(format!("sqlite-memory:{:p}", connection))
}

fn set_sqlite_write_gate_owner(key: &str, context: &str) -> SqliteWriteGateOwnerGuard {
    let current_thread = thread::current();
    let owner = SqliteWriteGateOwner {
        context: context.to_string(),
        thread_name: current_thread.name().map(ToOwned::to_owned),
        started_at: Instant::now(),
    };
    SQLITE_WRITE_GATE_OWNERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key.to_string(), owner);
    SqliteWriteGateOwnerGuard {
        key: key.to_string(),
    }
}

fn sqlite_write_gate_owner_message(key: &str) -> Option<String> {
    SQLITE_WRITE_GATE_OWNERS.get().and_then(|owners| {
        owners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
            .map(|owner| {
                let thread_name = owner.thread_name.unwrap_or_else(|| "unnamed".to_string());
                format!(
                    " Current holder: context='{}', thread='{}', held_for_ms={}.",
                    owner.context,
                    thread_name,
                    owner.started_at.elapsed().as_millis()
                )
            })
    })
}

fn with_sqlite_write_gate<T, F>(
    gate: &Arc<Mutex<()>>,
    gate_key: &str,
    context: &str,
    operation: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let _guard = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let _owner = set_sqlite_write_gate_owner(gate_key, context);
    operation()
}

fn with_sqlite_write_gate_timeout<T, F>(
    gate: &Arc<Mutex<()>>,
    gate_key: &str,
    context: &str,
    timeout: StdDuration,
    operation: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let started_at = Instant::now();
    loop {
        match gate.try_lock() {
            Ok(_guard) => {
                let _owner = set_sqlite_write_gate_owner(gate_key, context);
                return operation();
            }
            Err(TryLockError::Poisoned(poisoned)) => {
                let _guard = poisoned.into_inner();
                let _owner = set_sqlite_write_gate_owner(gate_key, context);
                return operation();
            }
            Err(TryLockError::WouldBlock) => {
                if started_at.elapsed() >= timeout {
                    return Err(format!(
                        "Timed out waiting {}ms for SQLite worker write gate while running '{}'.{}",
                        timeout.as_millis(),
                        context,
                        sqlite_write_gate_owner_message(gate_key).unwrap_or_default()
                    ));
                }
                thread::sleep(StdDuration::from_millis(25));
            }
        }
    }
}

fn immediate_transaction<'a>(
    connection: &'a mut Connection,
    context: &str,
) -> Result<Transaction<'a>, String> {
    connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            if sqlite_error_is_busy(&error) {
                format!("Failed to start {context} transaction: database is locked/busy: {error}")
            } else {
                format!("Failed to start {context} transaction: {error}")
            }
        })
}

pub(crate) fn with_immediate_write_transaction<T, F>(
    path: &Path,
    context: &str,
    mut operation: F,
) -> Result<T, String>
where
    F: FnMut(&Transaction<'_>) -> Result<T, String>,
{
    let gate_key = sqlite_write_gate_key_for_path(path)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    retry_sqlite_busy_operation(|| {
        with_sqlite_write_gate(&gate, &gate_key, context, || {
            let mut connection = open_connection(path)?;
            let transaction = immediate_transaction(&mut connection, context)?;
            let value = operation(&transaction)?;
            transaction
                .commit()
                .map_err(|error| format!("Failed to commit {context} transaction: {error}"))?;
            Ok(value)
        })
    })
}

pub(crate) fn with_immediate_write_transaction_timeout<T, F>(
    path: &Path,
    context: &str,
    gate_timeout: StdDuration,
    mut operation: F,
) -> Result<T, String>
where
    F: FnMut(&Transaction<'_>) -> Result<T, String>,
{
    let gate_key = sqlite_write_gate_key_for_path(path)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    with_sqlite_write_gate_timeout(&gate, &gate_key, context, gate_timeout, || {
        let mut connection = open_connection(path)?;
        let transaction = immediate_transaction(&mut connection, context)?;
        let value = operation(&transaction)?;
        transaction
            .commit()
            .map_err(|error| format!("Failed to commit {context} transaction: {error}"))?;
        Ok(value)
    })
}

fn env_flag_enabled(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "0" | "false" | "no" | "off" => false,
            "1" | "true" | "yes" | "on" => true,
            _ => default,
        },
        Err(_) => default,
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
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
    configure_sqlite_connection(&connection, false)?;
    Ok(connection)
}

pub fn get_runtime_summary(path: &Path) -> Result<RuntimeSummary, String> {
    let connection = open_readonly_connection(path)?;
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
    let connection = open_readonly_connection(path)?;
    load_stage_records_from_connection(&connection)
}

#[allow(dead_code)]
pub fn list_entities(path: &Path, filters: &EntityFilters) -> Result<Vec<EntityRecord>, String> {
    let connection = open_readonly_connection(path)?;
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
    let connection = open_readonly_connection(path)?;
    let page_size = query.limit.or(query.page_size).unwrap_or(50).clamp(1, 200);
    let offset = query
        .offset
        .unwrap_or_else(|| (query.page.unwrap_or(1).max(1) - 1) * page_size);
    let page = (offset / page_size) + 1;

    let mut where_clauses = Vec::new();
    let mut values: Vec<SqlValue> = Vec::new();

    if let Some(search) = query
        .search
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        where_clauses.push(
            "(LOWER(entity.entity_id) LIKE ? OR LOWER(COALESCE(entity.display_name, '')) LIKE ? OR LOWER(COALESCE(entity.operator_note, '')) LIKE ? OR LOWER(COALESCE(entity.latest_file_path, '')) LIKE ? OR LOWER(COALESCE(latest_file.file_name, '')) LIKE ? OR LOWER(COALESCE(latest_file.payload_json, '')) LIKE ? OR entity.entity_id LIKE ? OR COALESCE(entity.display_name, '') LIKE ? OR COALESCE(entity.operator_note, '') LIKE ? OR COALESCE(entity.latest_file_path, '') LIKE ? OR COALESCE(latest_file.file_name, '') LIKE ? OR COALESCE(latest_file.payload_json, '') LIKE ?)"
                .to_string(),
        );
        let pattern = format!("%{}%", search.to_lowercase());
        let raw_pattern = format!("%{}%", search);
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern.clone()));
        values.push(SqlValue::Text(pattern));
        values.push(SqlValue::Text(raw_pattern.clone()));
        values.push(SqlValue::Text(raw_pattern.clone()));
        values.push(SqlValue::Text(raw_pattern.clone()));
        values.push(SqlValue::Text(raw_pattern.clone()));
        values.push(SqlValue::Text(raw_pattern.clone()));
        values.push(SqlValue::Text(raw_pattern));
    }
    if !query.include_archived.unwrap_or(false) {
        where_clauses.push("entity.is_archived = 0".to_string());
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
            entity.display_name,
            entity.operator_note,
            entity.is_archived,
            entity.archived_at,
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
    let connection = open_readonly_connection(path)?;
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
    let connection = open_readonly_connection(path)?;
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
    let connection = open_readonly_connection(path)?;
    load_app_events_from_connection(&connection, limit)
}

pub fn list_stage_runs(
    path: &Path,
    entity_id: Option<&str>,
) -> Result<Vec<StageRunRecord>, String> {
    let connection = open_readonly_connection(path)?;
    load_stage_runs_from_connection(&connection, entity_id, 100)
}

pub fn get_stage_state_status(
    path: &Path,
    entity_id: &str,
    stage_id: &str,
) -> Result<Option<String>, String> {
    let connection = open_readonly_connection(path)?;
    find_stage_state_identity(&connection, entity_id, stage_id).map(|state| state.map(|s| s.status))
}

pub fn get_stage_state(
    path: &Path,
    entity_id: &str,
    stage_id: &str,
) -> Result<Option<EntityStageStateRecord>, String> {
    let connection = open_readonly_connection(path)?;
    connection
        .query_row(
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
            WHERE entity_id = ?1 AND stage_id = ?2
            "#,
            params![entity_id, stage_id],
            stage_state_from_row,
        )
        .optional()
        .map_err(|error| {
            format!("Failed to load stage state for entity '{entity_id}' on stage '{stage_id}': {error}")
        })
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
    if active_worker_lease_exists(&connection, state.id)? {
        return Err(
            "active_worker_lease_exists: This state is currently leased by a worker. Stop/reconcile the worker lease first."
                .to_string(),
        );
    }

    if state.status == "in_progress" {
        return Err(
            "state_in_progress_cannot_reset: Use Reconcile stuck worker states first.".to_string(),
        );
    }
    if state.status == "queued" {
        return Err(
            "state_queued_cannot_reset: Use Reconcile stuck worker states first.".to_string(),
        );
    }
    if !matches!(state.status.as_str(), "failed" | "blocked" | "retry_wait") {
        return Err(format!(
            "state_not_resettable: Entity '{entity_id}' on stage '{stage_id}' has status '{}' and cannot be reset to pending.",
            state.status
        ));
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
        "entity_stage_state_manual_reset",
        &format!("Manual reset moved entity '{entity_id}' on stage '{stage_id}' to pending."),
        Some(json!({
            "action": "reset_to_pending",
            "actor": "operator",
            "source": "manual",
            "entity_id": entity_id,
            "stage_id": stage_id,
            "reason": operator_comment,
            "old_status": state.status,
            "new_status": "pending",
        })),
        &now,
    )
}

pub fn reset_failed_blocked_entity_stages_to_pending(
    path: &Path,
    operator_comment: Option<&str>,
) -> Result<BulkResetEntityStagesPayload, String> {
    retry_sqlite_busy_operation(|| {
        reset_failed_blocked_entity_stages_to_pending_once(path, operator_comment)
    })
}

fn reset_failed_blocked_entity_stages_to_pending_once(
    path: &Path,
    operator_comment: Option<&str>,
) -> Result<BulkResetEntityStagesPayload, String> {
    let mut connection = open_connection(path)?;
    let now = Utc::now().to_rfc3339();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            if sqlite_error_is_busy(&error) {
                format!("Failed to start bulk reset transaction: database is locked/busy: {error}")
            } else {
                format!("Failed to start bulk reset transaction: {error}")
            }
        })?;
    let failed_before = query_count(
        &transaction,
        "SELECT COUNT(*) FROM entity_stage_states WHERE status = 'failed'",
        [],
    )?;
    let blocked_before = query_count(
        &transaction,
        "SELECT COUNT(*) FROM entity_stage_states WHERE status = 'blocked'",
        [],
    )?;
    let skipped_active_lease_count = query_count(
        &transaction,
        r#"
        SELECT COUNT(*)
        FROM entity_stage_states state
        WHERE state.status IN ('failed', 'blocked')
          AND EXISTS (
                SELECT 1
                FROM worker_leases lease
                WHERE lease.state_id = state.id
                  AND lease.status = 'active'
          )
        "#,
        [],
    )?;
    let mut statement = transaction
        .prepare(
            r#"
            SELECT id, entity_id, stage_id, status
            FROM entity_stage_states state
            WHERE state.status IN ('failed', 'blocked')
              AND NOT EXISTS (
                    SELECT 1
                    FROM worker_leases lease
                    WHERE lease.state_id = state.id
                      AND lease.status = 'active'
              )
            ORDER BY updated_at ASC, id ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare bulk reset candidate query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| format!("Failed to query bulk reset candidates: {error}"))?;
    let mut candidates = Vec::new();
    for row in rows {
        candidates
            .push(row.map_err(|error| format!("Failed to read bulk reset candidate: {error}"))?);
    }
    drop(statement);

    for (state_id, entity_id, stage_id, status) in &candidates {
        ensure_runtime_transition(
            status,
            &StageStatus::Pending,
            RuntimeTransitionReason::ManualReset,
            Some(*state_id),
            Some(entity_id),
            Some(stage_id),
        )?;
        transaction
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
                params![state_id, now],
            )
            .map_err(|error| {
                format!(
                    "Failed to bulk reset entity '{entity_id}' on stage '{stage_id}' to pending: {error}"
                )
            })?;
        update_entity_summary_from_state(&transaction, *state_id, StageStatus::Pending, &now)?;
    }

    if !candidates.is_empty() {
        insert_app_event(
            &transaction,
            AppEventLevel::Info,
            "entity_stage_states_bulk_reset",
            &format!(
                "Bulk reset moved {} failed/blocked entity stage state(s) to pending.",
                candidates.len()
            ),
            Some(json!({
                "action": "bulk_reset_failed_blocked_to_pending",
                "actor": "operator",
                "source": "manual",
                "reason": operator_comment,
                "reset_count": candidates.len() as u64,
                "failed_before": failed_before,
                "blocked_before": blocked_before,
                "skipped_active_lease_count": skipped_active_lease_count,
            })),
            &now,
        )?;
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit bulk reset transaction: {error}"))?;
    Ok(BulkResetEntityStagesPayload {
        reset_count: candidates.len() as u64,
        failed_before,
        blocked_before,
        skipped_active_lease_count,
    })
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
        let is_local_file = file.storage_provider == StorageProvider::Local;
        let absolute_path = workdir_path.join(&file.file_path);
        let parent_exists = is_local_file
            && absolute_path
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
                storage_provider: file.storage_provider.clone(),
                bucket: file.bucket.clone(),
                key: file.key.clone(),
                artifact_id: file.artifact_id.clone(),
                relation_to_source: file.relation_to_source.clone(),
                producer_run_id: file.producer_run_id.clone(),
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
                can_open_file: is_local_file && file.file_exists && absolute_path.exists(),
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
            let is_s3_stage = stage
                .input_uri
                .as_deref()
                .is_some_and(|uri| uri.starts_with("s3://"));
            let uses_local_llm = stage.resource_class.uses_local_llm();
            let folder_path = workdir_path.join(&stage.input_folder);
            WorkspaceStageTree {
                stage_id: stage.id,
                input_folder: stage.input_folder,
                input_uri: stage.input_uri.clone(),
                storage_provider: if is_s3_stage {
                    StorageProvider::S3
                } else {
                    StorageProvider::Local
                },
                output_folder: non_empty_string(stage.output_folder),
                workflow_url: non_empty_string(stage.workflow_url),
                max_attempts: stage.max_attempts,
                retry_delay_sec: stage.retry_delay_sec,
                next_stage: stage.next_stage,
                save_path_aliases: stage.save_path_aliases,
                resource_class: stage.resource_class,
                uses_local_llm,
                allow_empty_outputs: stage.allow_empty_outputs,
                allow_multiple_outputs: stage.allow_multiple_outputs,
                is_active: stage.is_active,
                archived_at: stage.archived_at,
                folder_path: stage
                    .input_uri
                    .clone()
                    .unwrap_or_else(|| path_string(&folder_path)),
                folder_exists: is_s3_stage || folder_path.exists(),
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

pub fn get_workspace_stage_overview(
    workdir_path: &Path,
    database_path: &Path,
) -> Result<WorkspaceExplorerResult, String> {
    let connection = open_readonly_connection(database_path)?;
    let generated_at = Utc::now().to_rfc3339();
    let stages = load_stage_records_from_connection(&connection)?;
    let last_scan_at = load_setting(&connection, "last_scan_completed_at")?;
    let mut counters_by_stage = load_workspace_stage_file_counters(&connection)?;
    for (stage_id, state_counters) in load_workspace_stage_state_counters(&connection)? {
        let counters = counters_by_stage.entry(stage_id).or_default();
        counters.pending += state_counters.pending;
        counters.queued += state_counters.queued;
        counters.in_progress += state_counters.in_progress;
        counters.retry_wait += state_counters.retry_wait;
        counters.done += state_counters.done;
        counters.failed += state_counters.failed;
        counters.blocked += state_counters.blocked;
        counters.skipped += state_counters.skipped;
    }

    let mut totals = WorkspaceExplorerTotals {
        stages_total: stages.len() as u64,
        active_stages_total: stages.iter().filter(|stage| stage.is_active).count() as u64,
        inactive_stages_total: stages.iter().filter(|stage| !stage.is_active).count() as u64,
        entities_total: query_count(&connection, "SELECT COUNT(*) FROM entities", [])?,
        ..WorkspaceExplorerTotals::default()
    };

    let stage_trees = stages
        .into_iter()
        .map(|stage| {
            let counters = counters_by_stage.remove(&stage.id).unwrap_or_default();
            totals.registered_files_total += counters.registered_files;
            totals.present_files_total += counters.present_files;
            totals.missing_files_total += counters.missing_files;
            totals.invalid_files_total += counters.invalid_files;
            totals.managed_copies_total += counters.managed_copies;

            let is_s3_stage = stage
                .input_uri
                .as_deref()
                .is_some_and(|uri| uri.starts_with("s3://"));
            let uses_local_llm = stage.resource_class.uses_local_llm();
            let folder_path = workdir_path.join(&stage.input_folder);
            WorkspaceStageTree {
                stage_id: stage.id,
                input_folder: stage.input_folder,
                input_uri: stage.input_uri.clone(),
                storage_provider: if is_s3_stage {
                    StorageProvider::S3
                } else {
                    StorageProvider::Local
                },
                output_folder: non_empty_string(stage.output_folder),
                workflow_url: non_empty_string(stage.workflow_url),
                max_attempts: stage.max_attempts,
                retry_delay_sec: stage.retry_delay_sec,
                next_stage: stage.next_stage,
                save_path_aliases: stage.save_path_aliases,
                resource_class: stage.resource_class,
                uses_local_llm,
                allow_empty_outputs: stage.allow_empty_outputs,
                allow_multiple_outputs: stage.allow_multiple_outputs,
                is_active: stage.is_active,
                archived_at: stage.archived_at,
                folder_path: stage
                    .input_uri
                    .clone()
                    .unwrap_or_else(|| path_string(&folder_path)),
                folder_exists: is_s3_stage || folder_path.exists(),
                files: Vec::new(),
                invalid_files: Vec::new(),
                counters,
            }
        })
        .collect();

    Ok(WorkspaceExplorerResult {
        generated_at,
        workdir_path: path_string(workdir_path),
        last_scan_at,
        stages: stage_trees,
        entity_trails: Vec::new(),
        totals,
        errors: Vec::new(),
    })
}

fn load_workspace_stage_file_counters(
    connection: &Connection,
) -> Result<HashMap<String, WorkspaceStageTreeCounters>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                stage_id,
                COUNT(*) AS registered_files,
                COALESCE(SUM(CASE WHEN file_exists = 1 THEN 1 ELSE 0 END), 0) AS present_files,
                COALESCE(SUM(CASE WHEN file_exists = 0 THEN 1 ELSE 0 END), 0) AS missing_files,
                COALESCE(SUM(CASE WHEN is_managed_copy = 1 THEN 1 ELSE 0 END), 0) AS managed_copies
            FROM entity_files
            GROUP BY stage_id
            "#,
        )
        .map_err(|error| format!("Failed to prepare workspace stage file counters: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                WorkspaceStageTreeCounters {
                    registered_files: row.get::<_, i64>(1)? as u64,
                    present_files: row.get::<_, i64>(2)? as u64,
                    missing_files: row.get::<_, i64>(3)? as u64,
                    managed_copies: row.get::<_, i64>(4)? as u64,
                    ..WorkspaceStageTreeCounters::default()
                },
            ))
        })
        .map_err(|error| format!("Failed to query workspace stage file counters: {error}"))?;

    let mut counters = HashMap::new();
    for row in rows {
        let (stage_id, stage_counters) =
            row.map_err(|error| format!("Failed to read workspace stage file counters: {error}"))?;
        counters.insert(stage_id, stage_counters);
    }
    Ok(counters)
}

fn load_workspace_stage_state_counters(
    connection: &Connection,
) -> Result<HashMap<String, WorkspaceStageTreeCounters>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT stage_id, status, COUNT(*)
            FROM entity_stage_states
            GROUP BY stage_id, status
            "#,
        )
        .map_err(|error| format!("Failed to prepare workspace stage state counters: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
            ))
        })
        .map_err(|error| format!("Failed to query workspace stage state counters: {error}"))?;

    let mut counters = HashMap::<String, WorkspaceStageTreeCounters>::new();
    for row in rows {
        let (stage_id, status, count) =
            row.map_err(|error| format!("Failed to read workspace stage state counters: {error}"))?;
        let stage_counters = counters.entry(stage_id).or_default();
        match status.as_str() {
            "pending" => stage_counters.pending += count,
            "queued" => stage_counters.queued += count,
            "in_progress" => stage_counters.in_progress += count,
            "retry_wait" => stage_counters.retry_wait += count,
            "done" => stage_counters.done += count,
            "failed" => stage_counters.failed += count,
            "blocked" => stage_counters.blocked += count,
            "skipped" => stage_counters.skipped += count,
            _ => {}
        }
    }
    Ok(counters)
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
                let is_local_file = file.storage_provider == StorageProvider::Local;
                let absolute_path = workdir_path.join(&file.file_path);
                let parent_exists = is_local_file
                    && absolute_path
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
                    can_open_file: is_local_file && file.file_exists && absolute_path.exists(),
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
                display_name,
                operator_note,
                is_archived,
                archived_at,
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

pub fn update_entity_metadata(
    path: &Path,
    entity_id: &str,
    input: &UpdateEntityRequest,
) -> Result<Option<EntityRecord>, String> {
    let connection = open_connection(path)?;
    if find_entity_by_id(&connection, entity_id)?.is_none() {
        return Ok(None);
    }

    let now = Utc::now().to_rfc3339();
    if input.display_name.is_some() {
        connection
            .execute(
                "UPDATE entities SET display_name = ?2, updated_at = ?3 WHERE entity_id = ?1",
                params![
                    entity_id,
                    input.display_name.clone().and_then(non_empty_string),
                    now
                ],
            )
            .map_err(|error| {
                format!("Failed to update display_name for entity '{entity_id}': {error}")
            })?;
    }
    if input.operator_note.is_some() {
        connection
            .execute(
                "UPDATE entities SET operator_note = ?2, updated_at = ?3 WHERE entity_id = ?1",
                params![
                    entity_id,
                    input.operator_note.clone().and_then(non_empty_string),
                    now
                ],
            )
            .map_err(|error| {
                format!("Failed to update operator_note for entity '{entity_id}': {error}")
            })?;
    }

    find_entity_by_id(&connection, entity_id)
}

pub fn archive_entity(path: &Path, entity_id: &str) -> Result<Option<EntityRecord>, String> {
    set_entity_archived(path, entity_id, true)
}

pub fn restore_entity(path: &Path, entity_id: &str) -> Result<Option<EntityRecord>, String> {
    set_entity_archived(path, entity_id, false)
}

fn set_entity_archived(
    path: &Path,
    entity_id: &str,
    archived: bool,
) -> Result<Option<EntityRecord>, String> {
    let connection = open_connection(path)?;
    if find_entity_by_id(&connection, entity_id)?.is_none() {
        return Ok(None);
    }
    let now = Utc::now().to_rfc3339();
    connection
        .execute(
            r#"
            UPDATE entities
            SET is_archived = ?2,
                archived_at = ?3,
                updated_at = ?4
            WHERE entity_id = ?1
            "#,
            params![
                entity_id,
                bool_to_i64(archived),
                if archived { Some(now.clone()) } else { None },
                now,
            ],
        )
        .map_err(|error| {
            format!("Failed to update archive state for entity '{entity_id}': {error}")
        })?;
    find_entity_by_id(&connection, entity_id)
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
                stage.next_stage,
                stage.resource_class
            FROM entity_stage_states state INDEXED BY idx_entity_stage_states_worker_claim
            JOIN stages stage ON stage.stage_id = state.stage_id
            JOIN entity_files file ON file.id = state.file_instance_id
            JOIN entities entity ON entity.entity_id = state.entity_id
            WHERE stage.is_active = 1
              AND entity.is_archived = 0
              AND state.file_exists = 1
              AND file.file_exists = 1
              AND TRIM(stage.workflow_url) <> ''
              AND state.attempts < state.max_attempts
              AND NOT EXISTS (
                    SELECT 1 FROM worker_leases lease
                    WHERE lease.state_id = state.id AND lease.status = 'active'
              )
              AND (
                    state.status = 'pending'
                    OR (state.status = 'retry_wait' AND state.next_retry_at IS NOT NULL AND state.next_retry_at <= ?1)
                  )
            ORDER BY
                CASE WHEN state.status = 'pending' THEN 0 ELSE 1 END ASC,
                state.updated_at ASC,
                state.id ASC
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
                stage.next_stage,
                stage.resource_class
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
                stage.next_stage,
                stage.resource_class
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
    let gate_key = sqlite_write_gate_key_for_connection(connection)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    retry_sqlite_busy_operation(|| {
        with_sqlite_write_gate(&gate, &gate_key, "runtime task claim", || {
            claim_eligible_runtime_tasks_once(connection, now, limit)
        })
    })
}

fn claim_eligible_runtime_tasks_once(
    connection: &mut Connection,
    now: &str,
    limit: u64,
) -> Result<Vec<RuntimeTaskRecord>, String> {
    let transaction = immediate_transaction(connection, "runtime claim")?;
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
                  AND NOT EXISTS (
                        SELECT 1 FROM worker_leases lease
                        WHERE lease.state_id = entity_stage_states.id AND lease.status = 'active'
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
                  AND EXISTS (
                        SELECT 1 FROM entities
                        WHERE entities.entity_id = entity_stage_states.entity_id
                          AND entities.is_archived = 0
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
    let gate_key = sqlite_write_gate_key_for_connection(connection)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    retry_sqlite_busy_operation(|| {
        with_sqlite_write_gate(&gate, &gate_key, "specific runtime task claim", || {
            claim_specific_runtime_task_once(connection, entity_id, stage_id, now)
        })
    })
}

fn claim_specific_runtime_task_once(
    connection: &mut Connection,
    entity_id: &str,
    stage_id: &str,
    now: &str,
) -> Result<Option<RuntimeTaskRecord>, String> {
    let transaction = immediate_transaction(connection, "runtime claim")?;
    let Some(candidate) = find_runtime_task(&transaction, entity_id, stage_id)? else {
        transaction
            .commit()
            .map_err(|error| format!("Failed to commit empty runtime claim: {error}"))?;
        return Ok(None);
    };

    if active_worker_lease_exists(&transaction, candidate.state_id)? {
        return Err(format!(
            "Cannot manually claim entity '{}' on stage '{}': active worker lease exists.",
            entity_id, stage_id
        ));
    }
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
              AND NOT EXISTS (
                    SELECT 1 FROM worker_leases lease
                    WHERE lease.state_id = entity_stage_states.id AND lease.status = 'active'
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

pub(crate) fn claim_worker_runtime_tasks(
    connection: &mut Connection,
    input: &WorkerLeaseClaimInput,
) -> Result<Vec<RuntimeTaskRecord>, String> {
    let gate_key = sqlite_write_gate_key_for_connection(connection)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    retry_sqlite_busy_operation(|| {
        with_sqlite_write_gate(&gate, &gate_key, "worker lease claim", || {
            claim_worker_runtime_tasks_once(connection, input)
        })
    })
}

fn claim_worker_runtime_tasks_once(
    connection: &mut Connection,
    input: &WorkerLeaseClaimInput,
) -> Result<Vec<RuntimeTaskRecord>, String> {
    if input.limit == 0 {
        return Ok(Vec::new());
    }
    let now = input.now.to_rfc3339();
    let lease_until = (input.now + Duration::seconds(input.lease_ttl_sec as i64)).to_rfc3339();
    let transaction = immediate_transaction(connection, "worker lease claim")?;
    let control = worker_pool_control_state(&transaction, input.resource_class)?;
    if !control.is_started || control.is_paused || control.desired_concurrency == 0 {
        transaction
            .commit()
            .map_err(|error| format!("Failed to commit idle worker claim transaction: {error}"))?;
        return Ok(Vec::new());
    }
    let active_leases = active_worker_lease_count(&transaction, input.resource_class)?;
    if active_leases >= control.desired_concurrency as u64 {
        transaction.commit().map_err(|error| {
            format!("Failed to commit capped worker claim transaction: {error}")
        })?;
        return Ok(Vec::new());
    }
    let available_capacity = (control.desired_concurrency as u64 - active_leases).min(input.limit);
    let candidates = list_eligible_worker_runtime_tasks(
        &transaction,
        &now,
        input.resource_class,
        input.scheduling_policy,
        available_capacity,
    )?;
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
                    updated_at = ?3
                WHERE id = ?1
                  AND file_exists = 1
                  AND attempts < max_attempts
                  AND (
                        status = 'pending'
                        OR (status = 'retry_wait' AND next_retry_at IS NOT NULL AND next_retry_at <= ?3)
                      )
                  AND NOT EXISTS (
                        SELECT 1 FROM worker_leases lease
                        WHERE lease.state_id = entity_stage_states.id AND lease.status = 'active'
                  )
                  AND EXISTS (
                        SELECT 1 FROM stages
                        WHERE stages.stage_id = entity_stage_states.stage_id
                          AND stages.is_active = 1
                          AND stages.resource_class = ?2
                          AND TRIM(stages.workflow_url) <> ''
                  )
                  AND EXISTS (
                        SELECT 1 FROM entity_files
                        WHERE entity_files.id = entity_stage_states.file_instance_id
                          AND entity_files.file_exists = 1
                  )
                  AND EXISTS (
                        SELECT 1 FROM entities
                        WHERE entities.entity_id = entity_stage_states.entity_id
                          AND entities.is_archived = 0
                  )
                "#,
                params![candidate.state_id, input.resource_class.as_str(), now],
            )
            .map_err(|error| {
                format!(
                    "Failed to claim worker lease state '{}' for entity '{}' on stage '{}': {error}",
                    candidate.state_id, candidate.entity_id, candidate.stage_id
                )
            })?;

        if affected == 1 {
            let lease_id = Uuid::new_v4().to_string();
            transaction
                .execute(
                    r#"
                    INSERT INTO worker_leases (
                        lease_id,
                        state_id,
                        entity_id,
                        entity_file_id,
                        stage_id,
                        resource_class,
                        worker_id,
                        status,
                        leased_at,
                        lease_until,
                        heartbeat_at,
                        created_at,
                        updated_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?9, ?8, ?8, ?8)
                    "#,
                    params![
                        lease_id,
                        candidate.state_id,
                        candidate.entity_id,
                        candidate.file_instance_id,
                        candidate.stage_id,
                        input.resource_class.as_str(),
                        input.worker_id,
                        now,
                        lease_until
                    ],
                )
                .map_err(|error| {
                    format!(
                        "Failed to insert worker lease for state '{}' on stage '{}': {error}",
                        candidate.state_id, candidate.stage_id
                    )
                })?;
            if let Some(mut task) = find_runtime_task_by_state_id(&transaction, candidate.state_id)?
            {
                task.lease_id = Some(lease_id);
                task.worker_id = Some(input.worker_id.clone());
                claimed.push(task);
            }
        }
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit worker lease claim transaction: {error}"))?;
    Ok(claimed)
}

pub fn set_worker_pool_paused(
    path: &Path,
    resource_class: ResourceClass,
    is_paused: bool,
    reason: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(path)?;
    set_worker_pool_paused_with_connection(&connection, resource_class, is_paused, reason)
}

pub fn set_all_worker_pools_paused(
    path: &Path,
    is_paused: bool,
    reason: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(path)?;
    for resource_class in [ResourceClass::Default, ResourceClass::LocalLlm] {
        set_worker_pool_paused_with_connection(&connection, resource_class, is_paused, reason)?;
    }
    Ok(())
}

pub(crate) fn set_worker_pool_paused_with_connection(
    connection: &Connection,
    resource_class: ResourceClass,
    is_paused: bool,
    reason: Option<&str>,
) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let normalized_reason = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    retry_sqlite_busy_operation(|| {
        connection
            .execute(
                r#"
            INSERT INTO worker_pool_controls (
                resource_class,
                desired_concurrency,
                is_started,
                is_paused,
                pause_reason,
                updated_at
            )
            VALUES (?1, 0, 0, ?2, ?3, ?4)
            ON CONFLICT(resource_class) DO UPDATE SET
                is_paused = excluded.is_paused,
                pause_reason = excluded.pause_reason,
                updated_at = excluded.updated_at
            "#,
                params![
                    resource_class.as_str(),
                    if is_paused { 1_i64 } else { 0_i64 },
                    if is_paused {
                        normalized_reason.as_deref()
                    } else {
                        None
                    },
                    now
                ],
            )
            .map_err(|error| {
                format!(
                    "Failed to update worker pool '{}' pause state: {error}",
                    resource_class.as_str()
                )
            })
            .map(|_| ())
    })?;
    Ok(())
}

pub fn set_all_worker_pools_started(
    path: &Path,
    default_desired_concurrency: u32,
    local_llm_desired_concurrency: u32,
) -> Result<(), String> {
    let connection = open_connection(path)?;
    set_worker_pool_runtime_state_with_connection(
        &connection,
        ResourceClass::Default,
        true,
        default_desired_concurrency,
    )?;
    set_worker_pool_runtime_state_with_connection(
        &connection,
        ResourceClass::LocalLlm,
        true,
        local_llm_desired_concurrency,
    )?;
    Ok(())
}

pub fn set_all_worker_pools_stopped(path: &Path) -> Result<(), String> {
    let connection = open_connection(path)?;
    for resource_class in [ResourceClass::Default, ResourceClass::LocalLlm] {
        let current = worker_pool_control_state(&connection, resource_class)?;
        set_worker_pool_runtime_state_with_connection(
            &connection,
            resource_class,
            false,
            current.desired_concurrency,
        )?;
    }
    Ok(())
}

pub fn update_worker_pool_desired_concurrency(
    path: &Path,
    resource_class: ResourceClass,
    desired_concurrency: u32,
) -> Result<(), String> {
    let connection = open_connection(path)?;
    let current = worker_pool_control_state(&connection, resource_class)?;
    set_worker_pool_runtime_state_with_connection(
        &connection,
        resource_class,
        current.is_started,
        desired_concurrency,
    )
}

pub(crate) fn set_worker_pool_runtime_state_with_connection(
    connection: &Connection,
    resource_class: ResourceClass,
    is_started: bool,
    desired_concurrency: u32,
) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    retry_sqlite_busy_operation(|| {
        connection
            .execute(
                r#"
                INSERT INTO worker_pool_controls (
                    resource_class,
                    desired_concurrency,
                    is_started,
                    is_paused,
                    pause_reason,
                    updated_at
                )
                VALUES (?1, ?2, ?3, 0, NULL, ?4)
                ON CONFLICT(resource_class) DO UPDATE SET
                    desired_concurrency = excluded.desired_concurrency,
                    is_started = excluded.is_started,
                    updated_at = excluded.updated_at
                "#,
                params![
                    resource_class.as_str(),
                    desired_concurrency as i64,
                    if is_started { 1_i64 } else { 0_i64 },
                    now,
                ],
            )
            .map_err(|error| {
                format!(
                    "Failed to update worker pool '{}' runtime state: {error}",
                    resource_class.as_str()
                )
            })
            .map(|_| ())
    })
}

pub fn worker_pool_is_paused(path: &Path, resource_class: ResourceClass) -> Result<bool, String> {
    let connection = open_connection(path)?;
    worker_pool_is_paused_with_connection(&connection, resource_class)
}

pub(crate) fn worker_pool_is_paused_with_connection(
    connection: &Connection,
    resource_class: ResourceClass,
) -> Result<bool, String> {
    worker_pool_control_state(connection, resource_class).map(|state| state.is_paused)
}

pub(crate) fn worker_pool_control_state(
    connection: &Connection,
    resource_class: ResourceClass,
) -> Result<WorkerPoolControlState, String> {
    connection
        .query_row(
            r#"
            SELECT desired_concurrency, is_started, is_paused, pause_reason
            FROM worker_pool_controls
            WHERE resource_class = ?1
            "#,
            params![resource_class.as_str()],
            |row| {
                Ok(WorkerPoolControlState {
                    desired_concurrency: row.get::<_, i64>(0)?.max(0) as u32,
                    is_started: row.get::<_, i64>(1)? != 0,
                    is_paused: row.get::<_, i64>(2)? != 0,
                    pause_reason: row.get::<_, Option<String>>(3)?,
                })
            },
        )
        .optional()
        .map(|value| {
            value.unwrap_or(WorkerPoolControlState {
                desired_concurrency: 0,
                is_started: false,
                is_paused: false,
                pause_reason: None,
            })
        })
        .map_err(|error| {
            format!(
                "Failed to read worker pool '{}' control state: {error}",
                resource_class.as_str()
            )
        })
}

fn list_eligible_worker_runtime_tasks(
    connection: &Connection,
    now: &str,
    resource_class: ResourceClass,
    scheduling_policy: SchedulingPolicy,
    limit: u64,
) -> Result<Vec<RuntimeTaskRecord>, String> {
    let ordering = match scheduling_policy {
        SchedulingPolicy::DepthFirst => {
            "CASE WHEN state.status = 'pending' THEN 0 ELSE 1 END ASC,
             CASE WHEN file.producer_run_id IS NOT NULL THEN 0 ELSE 1 END ASC,
             state.updated_at DESC,
             state.id DESC"
        }
        SchedulingPolicy::Fifo => {
            "CASE WHEN state.status = 'pending' THEN 0 ELSE 1 END ASC,
             state.updated_at ASC,
             state.id ASC"
        }
    };
    let mut statement = connection
        .prepare(&format!(
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
                stage.next_stage,
                stage.resource_class
            FROM entity_stage_states state INDEXED BY idx_entity_stage_states_worker_claim
            JOIN stages stage ON stage.stage_id = state.stage_id
            JOIN entity_files file ON file.id = state.file_instance_id
            JOIN entities entity ON entity.entity_id = state.entity_id
            WHERE stage.is_active = 1
              AND stage.resource_class = ?2
              AND entity.is_archived = 0
              AND state.file_exists = 1
              AND file.file_exists = 1
              AND TRIM(stage.workflow_url) <> ''
              AND state.attempts < state.max_attempts
              AND NOT EXISTS (
                    SELECT 1 FROM worker_leases lease
                    WHERE lease.state_id = state.id AND lease.status = 'active'
              )
                  AND (
                    state.status = 'pending'
                    OR (state.status = 'retry_wait' AND state.next_retry_at IS NOT NULL AND state.next_retry_at <= ?1)
                  )
            ORDER BY {ordering}
            LIMIT ?3
            "#
        ))
        .map_err(|error| format!("Failed to prepare eligible worker task query: {error}"))?;
    let rows = statement
        .query_map(
            params![now, resource_class.as_str(), limit as i64],
            runtime_task_from_row,
        )
        .map_err(|error| format!("Failed to query eligible worker tasks: {error}"))?;

    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(
            row.map_err(|error| format!("Failed to read eligible worker task row: {error}"))?,
        );
    }
    Ok(tasks)
}

fn active_worker_lease_count(
    connection: &Connection,
    resource_class: ResourceClass,
) -> Result<u64, String> {
    query_count(
        connection,
        "SELECT COUNT(*) FROM worker_leases WHERE resource_class = ?1 AND status = 'active'",
        params![resource_class.as_str()],
    )
}

pub(crate) fn active_worker_lease_exists(
    connection: &Connection,
    state_id: i64,
) -> Result<bool, String> {
    query_count(
        connection,
        "SELECT COUNT(*) FROM worker_leases WHERE state_id = ?1 AND status = 'active'",
        params![state_id],
    )
    .map(|count| count > 0)
}

pub(crate) fn attach_worker_lease_run(
    connection: &Connection,
    lease_id: &str,
    run_id: &str,
    updated_at: &str,
) -> Result<(), String> {
    let affected = connection
        .execute(
            r#"
            UPDATE worker_leases
            SET run_id = ?2,
                updated_at = ?3
            WHERE lease_id = ?1 AND status = 'active'
            "#,
            params![lease_id, run_id, updated_at],
        )
        .map_err(|error| {
            format!("Failed to attach run '{run_id}' to worker lease '{lease_id}': {error}")
        })?;
    if affected != 1 {
        return Err(format!(
            "Active worker lease '{lease_id}' was not found while attaching run '{run_id}'."
        ));
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn heartbeat_worker_lease(
    path: &Path,
    lease_id: &str,
    worker_id: &str,
    lease_ttl_sec: u64,
) -> Result<(), String> {
    let gate_key = sqlite_write_gate_key_for_path(path)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    retry_sqlite_busy_operation(|| {
        with_sqlite_write_gate(&gate, &gate_key, "worker heartbeat", || {
            let connection = open_connection(path)?;
            let now = Utc::now();
            heartbeat_worker_lease_with_connection(
                &connection,
                lease_id,
                worker_id,
                lease_ttl_sec,
                &now,
            )
        })
    })
}

pub(crate) fn heartbeat_worker_lease_with_gate_timeout(
    path: &Path,
    lease_id: &str,
    worker_id: &str,
    lease_ttl_sec: u64,
    gate_timeout: StdDuration,
) -> Result<(), String> {
    let gate_key = sqlite_write_gate_key_for_path(path)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    with_sqlite_write_gate_timeout(&gate, &gate_key, "worker heartbeat", gate_timeout, || {
        let connection = open_connection(path)?;
        let now = Utc::now();
        heartbeat_worker_lease_with_connection(
            &connection,
            lease_id,
            worker_id,
            lease_ttl_sec,
            &now,
        )
    })
}

pub(crate) fn heartbeat_worker_lease_with_connection(
    connection: &Connection,
    lease_id: &str,
    worker_id: &str,
    lease_ttl_sec: u64,
    now: &DateTime<Utc>,
) -> Result<(), String> {
    let now_text = now.to_rfc3339();
    let lease_until = (*now + Duration::seconds(lease_ttl_sec as i64)).to_rfc3339();
    let affected = connection
        .execute(
            r#"
            UPDATE worker_leases
            SET heartbeat_at = ?3,
                lease_until = ?4,
                updated_at = ?3
            WHERE lease_id = ?1
              AND worker_id = ?2
              AND status = 'active'
            "#,
            params![lease_id, worker_id, now_text, lease_until],
        )
        .map_err(|error| format!("Failed to heartbeat worker lease '{lease_id}': {error}"))?;
    if affected != 1 {
        return Err(format!(
            "Active worker lease '{lease_id}' owned by worker '{worker_id}' was not found."
        ));
    }
    Ok(())
}

pub(crate) fn finish_worker_lease(
    connection: &Connection,
    lease_id: &str,
    status: &str,
    release_reason: &str,
    released_at: &str,
) -> Result<(), String> {
    if !matches!(status, "done" | "failed" | "released") {
        return Err(format!(
            "Unsupported worker lease finish status '{status}'."
        ));
    }
    let affected = connection
        .execute(
            r#"
            UPDATE worker_leases
            SET status = ?2,
                released_at = ?3,
                release_reason = ?4,
                updated_at = ?3
            WHERE lease_id = ?1 AND status = 'active'
            "#,
            params![lease_id, status, released_at, release_reason],
        )
        .map_err(|error| format!("Failed to finish worker lease '{lease_id}': {error}"))?;
    if affected != 1 {
        return Err(format!("Active worker lease '{lease_id}' was not found."));
    }
    Ok(())
}

pub fn recover_expired_worker_leases(path: &Path) -> Result<u64, String> {
    with_immediate_write_transaction(path, "worker lease recovery", |connection| {
        let now = Utc::now().to_rfc3339();
        recover_expired_worker_leases_with_connection(connection, &now)
    })
}

pub(crate) fn recover_expired_worker_leases_with_connection(
    connection: &Connection,
    now: &str,
) -> Result<u64, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                lease_id,
                state_id,
                entity_id,
                entity_file_id,
                stage_id,
                resource_class,
                worker_id,
                status,
                run_id,
                leased_at,
                lease_until,
                heartbeat_at,
                released_at,
                release_reason,
                updated_at
            FROM worker_leases
            WHERE status = 'active' AND lease_until < ?1
            ORDER BY lease_until ASC, id ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare expired worker lease query: {error}"))?;
    let rows = statement
        .query_map(params![now], worker_lease_from_row)
        .map_err(|error| format!("Failed to query expired worker leases: {error}"))?;
    let mut leases = Vec::new();
    for row in rows {
        leases.push(
            row.map_err(|error| format!("Failed to read expired worker lease row: {error}"))?,
        );
    }
    drop(statement);

    let mut recovered = 0_u64;
    for lease in leases {
        if lease_run_finished(connection, lease.run_id.as_deref())? {
            continue;
        }
        let affected = connection
            .execute(
                r#"
                UPDATE worker_leases
                SET status = 'expired',
                    released_at = ?2,
                    release_reason = 'lease_expired',
                    updated_at = ?2
                WHERE lease_id = ?1 AND status = 'active'
                "#,
                params![lease.lease_id, now],
            )
            .map_err(|error| {
                format!(
                    "Failed to mark worker lease '{}' expired: {error}",
                    lease.lease_id
                )
            })?;
        if affected != 1 {
            continue;
        }

        let state = find_stage_state_by_id(connection, lease.state_id)?;
        if let Some(state) = state {
            match state.status.as_str() {
                "queued" => {
                    release_queued_claim(connection, lease.state_id, now)?;
                }
                "in_progress" => {
                    let next_status = if state.attempts < state.max_attempts {
                        StageStatus::RetryWait
                    } else {
                        StageStatus::Failed
                    };
                    let next_retry_at = if matches!(next_status, StageStatus::RetryWait) {
                        Some(now)
                    } else {
                        None
                    };
                    update_stage_state_failure_with_reason(
                        connection,
                        lease.state_id,
                        next_status,
                        "Worker lease expired before task completion.",
                        None,
                        next_retry_at,
                        now,
                        RuntimeTransitionReason::StuckReconciliation,
                    )?;
                }
                _ => {}
            }
        }
        insert_app_event(
            connection,
            AppEventLevel::Warning,
            "worker_lease_expired",
            &format!(
                "Worker lease '{}' expired for entity '{}' on stage '{}'.",
                lease.lease_id, lease.entity_id, lease.stage_id
            ),
            Some(json!({
                "lease_id": lease.lease_id,
                "state_id": lease.state_id,
                "entity_id": lease.entity_id,
                "stage_id": lease.stage_id,
                "resource_class": lease.resource_class.as_str(),
                "worker_id": lease.worker_id,
            })),
            now,
        )?;
        recovered += 1;
    }
    Ok(recovered)
}

#[allow(dead_code)]
pub fn recover_orphaned_worker_process_leases(
    path: &Path,
    workspace_id: &str,
    current_process_id: u32,
) -> Result<u64, String> {
    let current_worker_marker = format!("-{current_process_id}-{workspace_id}-");
    with_immediate_write_transaction(path, "worker process lease recovery", |connection| {
        let now = Utc::now().to_rfc3339();
        recover_orphaned_worker_process_leases_with_connection(
            connection,
            &current_worker_marker,
            &now,
        )
    })
}

pub(crate) fn repair_worker_leases_with_gate_timeout(
    path: &Path,
    workspace_id: &str,
    current_process_id: u32,
    worker_lease_sec: u64,
    gate_timeout: StdDuration,
) -> Result<u64, String> {
    let current_worker_marker = format!("-{current_process_id}-{workspace_id}-");
    with_immediate_write_transaction_timeout(
        path,
        "operator worker repair",
        gate_timeout,
        |connection| {
            let now = Utc::now().to_rfc3339();
            let mut repaired = recover_orphaned_worker_process_leases_with_connection(
                connection,
                &current_worker_marker,
                &now,
            )?;
            repaired += recover_expired_worker_leases_with_connection(connection, &now)?;
            repaired += reconcile_active_finished_worker_leases(connection, &now)?;
            repaired +=
                reconcile_in_progress_without_active_lease(connection, worker_lease_sec, &now)?;
            repaired += reconcile_queued_without_active_lease(connection, &now)?;
            for resource_class in [ResourceClass::Default, ResourceClass::LocalLlm] {
                set_worker_pool_paused_with_connection(connection, resource_class, false, None)?;
            }
            Ok(repaired)
        },
    )
}

fn recover_orphaned_worker_process_leases_with_connection(
    connection: &Connection,
    current_worker_marker: &str,
    now: &str,
) -> Result<u64, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                lease_id,
                state_id,
                entity_id,
                entity_file_id,
                stage_id,
                resource_class,
                worker_id,
                status,
                run_id,
                leased_at,
                lease_until,
                heartbeat_at,
                released_at,
                release_reason,
                updated_at
            FROM worker_leases
            WHERE status = 'active'
              AND instr(worker_id, ?1) = 0
            ORDER BY leased_at ASC, id ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare orphaned worker lease query: {error}"))?;
    let rows = statement
        .query_map(params![current_worker_marker], worker_lease_from_row)
        .map_err(|error| format!("Failed to query orphaned worker leases: {error}"))?;
    let mut leases = Vec::new();
    for row in rows {
        leases.push(
            row.map_err(|error| format!("Failed to read orphaned worker lease row: {error}"))?,
        );
    }
    drop(statement);

    let mut recovered = 0_u64;
    for lease in leases {
        if lease_run_finished(connection, lease.run_id.as_deref())? {
            continue;
        }
        let affected = connection
            .execute(
                r#"
                UPDATE worker_leases
                SET status = 'expired',
                    released_at = ?2,
                    release_reason = 'worker_process_restarted',
                    updated_at = ?2
                WHERE lease_id = ?1 AND status = 'active'
                "#,
                params![lease.lease_id, now],
            )
            .map_err(|error| {
                format!(
                    "Failed to mark orphaned worker lease '{}' expired: {error}",
                    lease.lease_id
                )
            })?;
        if affected != 1 {
            continue;
        }

        let state = find_stage_state_by_id(connection, lease.state_id)?;
        if let Some(state) = state {
            match state.status.as_str() {
                "queued" => {
                    release_queued_claim(connection, lease.state_id, now)?;
                }
                "in_progress" => {
                    let next_status = if state.attempts < state.max_attempts {
                        StageStatus::RetryWait
                    } else {
                        StageStatus::Failed
                    };
                    let next_retry_at = if matches!(next_status, StageStatus::RetryWait) {
                        Some(now)
                    } else {
                        None
                    };
                    update_stage_state_failure_with_reason(
                        connection,
                        lease.state_id,
                        next_status,
                        "Worker process restarted before task completion.",
                        None,
                        next_retry_at,
                        now,
                        RuntimeTransitionReason::StuckReconciliation,
                    )?;
                }
                _ => {}
            }
        }
        insert_app_event(
            connection,
            AppEventLevel::Warning,
            "worker_lease_orphaned_process_recovered",
            &format!(
                "Worker lease '{}' belonged to a previous worker process and was recovered for entity '{}' on stage '{}'.",
                lease.lease_id, lease.entity_id, lease.stage_id
            ),
            Some(json!({
                "lease_id": lease.lease_id,
                "state_id": lease.state_id,
                "entity_id": lease.entity_id,
                "stage_id": lease.stage_id,
                "resource_class": lease.resource_class.as_str(),
                "worker_id": lease.worker_id,
                "current_worker_marker": current_worker_marker,
            })),
            now,
        )?;
        recovered += 1;
    }
    Ok(recovered)
}

pub fn release_worker_lease(path: &Path, lease_id: &str, reason: &str) -> Result<bool, String> {
    with_immediate_write_transaction(path, "manual worker lease release", |connection| {
        release_worker_lease_with_connection(connection, lease_id, reason, &Utc::now().to_rfc3339())
    })
}

pub(crate) fn release_worker_lease_with_connection(
    connection: &Connection,
    lease_id: &str,
    reason: &str,
    now: &str,
) -> Result<bool, String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err("Manual worker lease release requires a reason.".to_string());
    }
    let lease = find_worker_lease_by_id(connection, lease_id)?
        .ok_or_else(|| format!("Worker lease '{lease_id}' was not found."))?;
    if lease.status != "active" {
        return Err(format!(
            "Worker lease '{lease_id}' is '{}' and cannot be manually released.",
            lease.status
        ));
    }

    let run_finished = lease_run_finished(connection, lease.run_id.as_deref())?;
    let expired = lease.lease_until.as_str() < now;
    let state_status = find_stage_state_by_id(connection, lease.state_id)?
        .map(|state| state.status)
        .unwrap_or_else(|| "missing".to_string());
    let state_is_running = matches!(state_status.as_str(), "queued" | "in_progress");
    if !run_finished && !(expired && !state_is_running) {
        return Err(format!(
            "Worker lease '{lease_id}' is still active. Use recover expired leases for expired unfinished work, or wait for the worker heartbeat to expire."
        ));
    }

    let affected = connection
        .execute(
            r#"
            UPDATE worker_leases
            SET status = 'released',
                released_at = ?2,
                release_reason = ?3,
                updated_at = ?2
            WHERE lease_id = ?1 AND status = 'active'
            "#,
            params![lease_id, now, reason],
        )
        .map_err(|error| format!("Failed to release worker lease '{lease_id}': {error}"))?;
    if affected != 1 {
        return Ok(false);
    }

    insert_app_event(
        connection,
        AppEventLevel::Warning,
        "worker_lease_manually_released",
        &format!(
            "Worker lease '{}' was manually released for entity '{}' on stage '{}'.",
            lease.lease_id, lease.entity_id, lease.stage_id
        ),
        Some(json!({
            "lease_id": lease.lease_id,
            "state_id": lease.state_id,
            "entity_id": lease.entity_id,
            "stage_id": lease.stage_id,
            "resource_class": lease.resource_class.as_str(),
            "worker_id": lease.worker_id,
            "run_finished": run_finished,
            "expired": expired,
            "state_status": state_status,
            "reason": reason,
        })),
        now,
    )?;
    Ok(true)
}

pub fn get_worker_summary(path: &Path, config: &PipelineConfig) -> Result<WorkerSummary, String> {
    let connection = open_readonly_connection(path)?;
    let now = Utc::now().to_rfc3339();
    let default_pool = worker_pool_summary(
        &connection,
        ResourceClass::Default,
        config.runtime.worker_pools.default.concurrency,
        &now,
    )?;
    let local_llm_pool = worker_pool_summary(
        &connection,
        ResourceClass::LocalLlm,
        config.runtime.worker_pools.local_llm.concurrency,
        &now,
    )?;
    let active_leases_total = default_pool.active_leases + local_llm_pool.active_leases;
    let expired_leases_total = default_pool.expired_leases + local_llm_pool.expired_leases;
    let paused_all = default_pool.is_paused && local_llm_pool.is_paused;
    let recent_leases = list_recent_worker_leases(&connection, 20)?;
    let all_anomalies = list_worker_state_anomalies(
        &connection,
        &now,
        config.runtime.worker_heartbeat_sec,
        config.runtime.worker_lease_sec,
        None,
    )?;
    let worker_state_anomaly_counts = worker_state_anomaly_counts(&all_anomalies);
    let recent_worker_state_anomalies = all_anomalies.into_iter().take(20).collect();
    let last_recovery_at = connection
        .query_row(
            "SELECT MAX(created_at) FROM app_events WHERE code = 'worker_lease_expired'",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|error| format!("Failed to query last worker lease recovery time: {error}"))?;
    Ok(WorkerSummary {
        worker_lease_sec: config.runtime.worker_lease_sec,
        worker_heartbeat_sec: config.runtime.worker_heartbeat_sec,
        scheduling_policy: config.runtime.scheduling_policy,
        workers_enabled: false,
        broad_runs_disabled: false,
        runtime_status: "stopped".to_string(),
        paused_all,
        pools: vec![default_pool, local_llm_pool],
        active_leases_total,
        expired_leases_total,
        last_recovery_at,
        recent_leases,
        worker_state_anomaly_counts,
        recent_worker_state_anomalies,
    })
}

pub fn reconcile_stuck_worker_states(path: &Path, worker_lease_sec: u64) -> Result<u64, String> {
    with_immediate_write_transaction(path, "worker state reconciliation", |connection| {
        let now = Utc::now().to_rfc3339();
        let mut reconciled = recover_expired_worker_leases_with_connection(connection, &now)?;
        reconciled += reconcile_active_finished_worker_leases(connection, &now)?;
        reconciled +=
            reconcile_in_progress_without_active_lease(connection, worker_lease_sec, &now)?;
        reconciled += reconcile_queued_without_active_lease(connection, &now)?;
        Ok(reconciled)
    })
}

pub(crate) fn finish_worker_task_internal_error(
    path: &Path,
    task: &RuntimeTaskRecord,
    message: &str,
) -> Result<(), String> {
    with_immediate_write_transaction(path, "worker internal error reconciliation", |connection| {
        finish_worker_task_internal_error_with_connection(connection, task, message)
    })
}

fn finish_worker_task_internal_error_with_connection(
    connection: &Connection,
    task: &RuntimeTaskRecord,
    message: &str,
) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let lease = match task.lease_id.as_deref() {
        Some(lease_id) => match find_worker_lease_by_id(&connection, lease_id)? {
            Some(lease) if lease.status == "active" => Some(lease),
            _ => find_active_worker_lease_by_state_id(&connection, task.state_id)?,
        },
        None => find_active_worker_lease_by_state_id(&connection, task.state_id)?,
    };
    let Some(lease) = lease else {
        return Ok(());
    };
    if lease.status != "active" {
        return Ok(());
    }
    let lease_id = lease.lease_id.clone();

    let run_id = match lease.run_id.clone() {
        Some(run_id) => Some(run_id),
        None => find_latest_unfinished_run_id_for_state(&connection, task.state_id)?,
    };
    if let Some(run_id) = run_id.as_deref() {
        finish_unfinished_stage_run(&connection, run_id, "worker_internal_error", message, &now)?;
    }

    if let Some(state) = find_stage_state_by_id(&connection, task.state_id)? {
        if matches!(state.status.as_str(), "queued" | "in_progress") {
            let next_status = if state.attempts < state.max_attempts {
                StageStatus::RetryWait
            } else {
                StageStatus::Failed
            };
            let next_retry_at = if matches!(next_status, StageStatus::RetryWait) {
                Some(now.as_str())
            } else {
                None
            };
            update_stage_state_failure_with_reason(
                &connection,
                task.state_id,
                next_status,
                message,
                None,
                next_retry_at,
                &now,
                RuntimeTransitionReason::StuckReconciliation,
            )?;
        }
    }

    finish_worker_lease(
        &connection,
        &lease_id,
        "failed",
        "worker_internal_error",
        &now,
    )?;
    insert_app_event(
        &connection,
        AppEventLevel::Error,
        "worker_internal_error_reconciled",
        &format!(
            "Worker task internal error was reconciled for entity '{}' on stage '{}': {}",
            task.entity_id, task.stage_id, message
        ),
        Some(json!({
            "lease_id": lease_id,
            "state_id": task.state_id,
            "entity_id": task.entity_id,
            "stage_id": task.stage_id,
            "resource_class": task.resource_class.as_str(),
            "message": message,
        })),
        &now,
    )?;
    Ok(())
}

fn worker_state_anomaly_counts(
    anomalies: &[WorkerStateAnomalyRecord],
) -> Vec<WorkerStateAnomalyCount> {
    let mut counts = HashMap::<String, u64>::new();
    for anomaly in anomalies {
        *counts.entry(anomaly.diagnosis.clone()).or_insert(0) += 1;
    }
    let mut counts = counts
        .into_iter()
        .map(|(diagnosis, count)| WorkerStateAnomalyCount { diagnosis, count })
        .collect::<Vec<_>>();
    counts.sort_by(|left, right| left.diagnosis.cmp(&right.diagnosis));
    counts
}

fn reconcile_active_finished_worker_leases(
    connection: &Connection,
    now: &str,
) -> Result<u64, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                lease_id,
                state_id,
                entity_id,
                entity_file_id,
                stage_id,
                resource_class,
                worker_id,
                status,
                run_id,
                leased_at,
                lease_until,
                heartbeat_at,
                released_at,
                release_reason,
                updated_at
            FROM worker_leases
            WHERE status = 'active'
              AND run_id IS NOT NULL
              AND EXISTS (
                    SELECT 1
                    FROM stage_runs run
                    WHERE run.run_id = worker_leases.run_id
                      AND run.finished_at IS NOT NULL
              )
            ORDER BY updated_at ASC, id ASC
            "#,
        )
        .map_err(|error| {
            format!("Failed to prepare active finished worker lease query: {error}")
        })?;
    let rows = statement
        .query_map([], worker_lease_from_row)
        .map_err(|error| format!("Failed to query active finished worker leases: {error}"))?;
    let mut leases = Vec::new();
    for row in rows {
        leases.push(
            row.map_err(|error| format!("Failed to read active finished lease row: {error}"))?,
        );
    }
    drop(statement);

    let mut reconciled = 0_u64;
    for lease in leases {
        let Some(run_id) = lease.run_id.as_deref() else {
            continue;
        };
        let Some(run) = find_stage_run_by_run_id(connection, run_id)? else {
            continue;
        };
        if run.finished_at.is_none() {
            continue;
        }
        if let Some(state) = find_stage_state_by_id(connection, lease.state_id)? {
            reconcile_state_from_finished_run(connection, &state, &run, now)?;
        }
        finish_worker_lease(
            connection,
            &lease.lease_id,
            if run.success { "done" } else { "failed" },
            "finished_run_reconciled",
            now,
        )?;
        insert_app_event(
            connection,
            AppEventLevel::Warning,
            "active_lease_with_finished_run_reconciled",
            &format!(
                "Active worker lease '{}' had a finished run and was reconciled.",
                lease.lease_id
            ),
            Some(json!({
                "lease_id": lease.lease_id,
                "state_id": lease.state_id,
                "entity_id": lease.entity_id,
                "stage_id": lease.stage_id,
                "resource_class": lease.resource_class.as_str(),
                "worker_id": lease.worker_id,
                "run_id": run.run_id,
                "run_success": run.success,
            })),
            now,
        )?;
        reconciled += 1;
    }
    Ok(reconciled)
}

fn reconcile_state_from_finished_run(
    connection: &Connection,
    state: &EntityStageStateRecord,
    run: &StageRunRecord,
    now: &str,
) -> Result<(), String> {
    if run.success {
        if state.status != "done" {
            update_stage_state_success_with_reason(
                connection,
                state.id,
                run.http_status,
                run.finished_at.as_deref().unwrap_or(now),
                state.created_child_path.as_deref(),
                RuntimeTransitionReason::StuckReconciliation,
            )?;
        }
        return Ok(());
    }

    let message = run
        .error_message
        .as_deref()
        .unwrap_or("Finished worker run failed before the active lease was closed.");
    if failure_type_is_blocking(run.error_type.as_deref()) {
        if state.status != "blocked" {
            block_stage_state(
                connection,
                state.id,
                message,
                run.finished_at.as_deref().unwrap_or(now),
            )?;
        }
        return Ok(());
    }

    if matches!(state.status.as_str(), "queued" | "in_progress") {
        reconcile_state_to_retry_or_failed(
            connection,
            state,
            message,
            run.http_status,
            run.finished_at.as_deref().unwrap_or(now),
        )?;
    }
    Ok(())
}

fn reconcile_in_progress_without_active_lease(
    connection: &Connection,
    worker_lease_sec: u64,
    now: &str,
) -> Result<u64, String> {
    let now_dt = DateTime::parse_from_rfc3339(now)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let recent_cutoff = (now_dt - Duration::seconds(worker_lease_sec.max(1) as i64)).to_rfc3339();
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
            FROM entity_stage_states state
            WHERE status = 'in_progress'
              AND NOT EXISTS (
                    SELECT 1
                    FROM worker_leases lease
                    WHERE lease.state_id = state.id
                      AND lease.status = 'active'
              )
              AND NOT EXISTS (
                    SELECT 1
                    FROM stage_runs run
                    WHERE run.entity_id = state.entity_id
                      AND run.stage_id = state.stage_id
                      AND COALESCE(run.entity_file_id, state.file_instance_id) = state.file_instance_id
                      AND run.finished_at IS NULL
                      AND run.started_at > ?1
              )
            ORDER BY updated_at ASC, id ASC
            "#,
        )
        .map_err(|error| {
            format!("Failed to prepare in-progress-without-lease reconciliation query: {error}")
        })?;
    let rows = statement
        .query_map(params![recent_cutoff], stage_state_from_row)
        .map_err(|error| format!("Failed to query in-progress-without-lease states: {error}"))?;
    let mut states = Vec::new();
    for row in rows {
        states.push(row.map_err(|error| {
            format!("Failed to read in-progress-without-lease state row: {error}")
        })?);
    }
    drop(statement);

    let mut reconciled = 0_u64;
    for state in states {
        finish_unfinished_stage_runs_for_state(
            connection,
            state.id,
            "in_progress_without_active_lease_reconciled",
            "In-progress state had no active worker lease and was reconciled.",
            now,
        )?;
        reconcile_state_to_retry_or_failed(
            connection,
            &state,
            "In-progress state had no active worker lease.",
            None,
            now,
        )?;
        insert_app_event(
            connection,
            AppEventLevel::Warning,
            "in_progress_without_active_lease_reconciled",
            &format!(
                "In-progress state without active lease was reconciled for entity '{}' on stage '{}'.",
                state.entity_id, state.stage_id
            ),
            Some(json!({
                "state_id": state.id,
                "entity_id": state.entity_id,
                "stage_id": state.stage_id,
            })),
            now,
        )?;
        reconciled += 1;
    }
    Ok(reconciled)
}

fn reconcile_queued_without_active_lease(
    connection: &Connection,
    now: &str,
) -> Result<u64, String> {
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
            FROM entity_stage_states state
            WHERE status = 'queued'
              AND NOT EXISTS (
                    SELECT 1
                    FROM worker_leases lease
                    WHERE lease.state_id = state.id
                      AND lease.status = 'active'
              )
              AND NOT EXISTS (
                    SELECT 1
                    FROM stage_runs run
                    WHERE run.entity_id = state.entity_id
                      AND run.stage_id = state.stage_id
                      AND COALESCE(run.entity_file_id, state.file_instance_id) = state.file_instance_id
                      AND run.finished_at IS NULL
              )
            ORDER BY updated_at ASC, id ASC
            "#,
        )
        .map_err(|error| {
            format!("Failed to prepare queued-without-lease reconciliation query: {error}")
        })?;
    let rows = statement
        .query_map([], stage_state_from_row)
        .map_err(|error| format!("Failed to query queued-without-lease states: {error}"))?;
    let mut states = Vec::new();
    for row in rows {
        states.push(
            row.map_err(|error| format!("Failed to read queued-without-lease state row: {error}"))?,
        );
    }
    drop(statement);

    let mut reconciled = 0_u64;
    for state in states {
        release_queued_claim(connection, state.id, now)?;
        insert_app_event(
            connection,
            AppEventLevel::Warning,
            "queued_without_active_lease_reconciled",
            &format!(
                "Queued state without active lease was moved back to pending for entity '{}' on stage '{}'.",
                state.entity_id, state.stage_id
            ),
            Some(json!({
                "state_id": state.id,
                "entity_id": state.entity_id,
                "stage_id": state.stage_id,
            })),
            now,
        )?;
        reconciled += 1;
    }
    Ok(reconciled)
}

fn reconcile_state_to_retry_or_failed(
    connection: &Connection,
    state: &EntityStageStateRecord,
    message: &str,
    http_status: Option<i64>,
    now: &str,
) -> Result<(), String> {
    let next_status = if state.attempts < state.max_attempts {
        StageStatus::RetryWait
    } else {
        StageStatus::Failed
    };
    let next_retry_at = if matches!(next_status, StageStatus::RetryWait) {
        Some(now)
    } else {
        None
    };
    update_stage_state_failure_with_reason(
        connection,
        state.id,
        next_status,
        message,
        http_status,
        next_retry_at,
        now,
        RuntimeTransitionReason::StuckReconciliation,
    )
}

fn finish_unfinished_stage_runs_for_state(
    connection: &Connection,
    state_id: i64,
    error_type: &str,
    message: &str,
    now: &str,
) -> Result<u64, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT run.run_id
            FROM stage_runs run
            JOIN entity_stage_states state
              ON state.entity_id = run.entity_id
             AND state.stage_id = run.stage_id
             AND COALESCE(run.entity_file_id, state.file_instance_id) = state.file_instance_id
            WHERE state.id = ?1
              AND run.finished_at IS NULL
            ORDER BY run.started_at ASC, run.id ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare unfinished stage-run query: {error}"))?;
    let rows = statement
        .query_map(params![state_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Failed to query unfinished stage-runs: {error}"))?;
    let mut run_ids = Vec::new();
    for row in rows {
        run_ids.push(row.map_err(|error| format!("Failed to read unfinished run id: {error}"))?);
    }
    drop(statement);
    for run_id in &run_ids {
        finish_unfinished_stage_run(connection, run_id, error_type, message, now)?;
    }
    Ok(run_ids.len() as u64)
}

fn find_latest_unfinished_run_id_for_state(
    connection: &Connection,
    state_id: i64,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            r#"
            SELECT run.run_id
            FROM stage_runs run
            JOIN entity_stage_states state
              ON state.entity_id = run.entity_id
             AND state.stage_id = run.stage_id
             AND COALESCE(run.entity_file_id, state.file_instance_id) = state.file_instance_id
            WHERE state.id = ?1
              AND run.finished_at IS NULL
            ORDER BY run.started_at DESC, run.id DESC
            LIMIT 1
            "#,
            params![state_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| {
            format!("Failed to find latest unfinished run for state '{state_id}': {error}")
        })
}

fn finish_unfinished_stage_run(
    connection: &Connection,
    run_id: &str,
    error_type: &str,
    message: &str,
    now: &str,
) -> Result<(), String> {
    let Some(run) = find_stage_run_by_run_id(connection, run_id)? else {
        return Ok(());
    };
    if run.finished_at.is_some() {
        return Ok(());
    }
    finish_stage_run(
        connection,
        &FinishStageRunInput {
            run_id: run_id.to_string(),
            response_json: None,
            http_status: None,
            success: false,
            error_type: Some(error_type.to_string()),
            error_message: Some(message.to_string()),
            finished_at: now.to_string(),
            duration_ms: 0,
        },
    )
}

fn find_stage_run_by_run_id(
    connection: &Connection,
    run_id: &str,
) -> Result<Option<StageRunRecord>, String> {
    connection
        .query_row(
            r#"
            SELECT id, run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url,
                   request_json, response_json, http_status, success, error_type, error_message,
                   started_at, finished_at, duration_ms
            FROM stage_runs
            WHERE run_id = ?1
            "#,
            params![run_id],
            stage_run_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load stage run '{run_id}': {error}"))
}

fn failure_type_is_blocking(error_type: Option<&str>) -> bool {
    matches!(
        error_type.unwrap_or_default(),
        "contract" | "invalid_json" | "manifest_invalid" | "manifest_blocked" | "copy_blocked"
    )
}

fn list_worker_state_anomalies(
    connection: &Connection,
    now: &str,
    worker_heartbeat_sec: u64,
    worker_lease_sec: u64,
    limit: Option<usize>,
) -> Result<Vec<WorkerStateAnomalyRecord>, String> {
    let now_dt = DateTime::parse_from_rfc3339(now)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let heartbeat_cutoff =
        (now_dt - Duration::seconds((worker_heartbeat_sec.max(1) * 2) as i64)).to_rfc3339();
    let recent_in_progress_cutoff =
        (now_dt - Duration::seconds(worker_lease_sec.max(1) as i64)).to_rfc3339();
    let mut anomalies = Vec::new();
    append_worker_lease_anomalies(connection, now, &heartbeat_cutoff, &mut anomalies)?;
    append_worker_state_anomalies(connection, &recent_in_progress_cutoff, &mut anomalies)?;
    anomalies.sort_by(|left, right| {
        left.diagnosis
            .cmp(&right.diagnosis)
            .then_with(|| left.stage_id.cmp(&right.stage_id))
            .then_with(|| left.entity_id.cmp(&right.entity_id))
            .then_with(|| left.state_id.cmp(&right.state_id))
    });
    if let Some(limit) = limit {
        anomalies.truncate(limit);
    }
    Ok(anomalies)
}

fn append_worker_lease_anomalies(
    connection: &Connection,
    now: &str,
    heartbeat_cutoff: &str,
    anomalies: &mut Vec<WorkerStateAnomalyRecord>,
) -> Result<(), String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                lease.state_id,
                lease.lease_id,
                lease.entity_id,
                lease.stage_id,
                lease.resource_class,
                state.status,
                lease.status,
                lease.worker_id,
                lease.run_id,
                lease.lease_until,
                lease.heartbeat_at,
                state.last_started_at,
                state.last_finished_at,
                CASE
                    WHEN state.id IS NULL THEN 'active_lease_for_missing_state'
                    WHEN run.finished_at IS NOT NULL THEN 'active_lease_with_finished_run'
                    WHEN lease.lease_until < ?1 THEN 'active_lease_expired'
                    WHEN lease.heartbeat_at < ?2 THEN 'active_lease_without_recent_heartbeat'
                    WHEN state.status NOT IN ('queued', 'in_progress') THEN 'active_lease_for_state_not_running'
                    ELSE 'active_lease_unknown'
                END AS diagnosis
            FROM worker_leases lease
            LEFT JOIN entity_stage_states state ON state.id = lease.state_id
            LEFT JOIN stage_runs run ON run.run_id = lease.run_id
            WHERE lease.status = 'active'
              AND (
                    state.id IS NULL
                 OR run.finished_at IS NOT NULL
                 OR lease.lease_until < ?1
                 OR lease.heartbeat_at < ?2
                 OR state.status NOT IN ('queued', 'in_progress')
              )
            "#,
        )
        .map_err(|error| format!("Failed to prepare worker lease anomaly query: {error}"))?;
    let rows = statement
        .query_map(params![now, heartbeat_cutoff], |row| {
            worker_state_anomaly_from_row(row, "Run reconcile stuck worker states.")
        })
        .map_err(|error| format!("Failed to query worker lease anomalies: {error}"))?;
    for row in rows {
        anomalies.push(
            row.map_err(|error| format!("Failed to read worker lease anomaly row: {error}"))?,
        );
    }
    Ok(())
}

fn append_worker_state_anomalies(
    connection: &Connection,
    recent_in_progress_cutoff: &str,
    anomalies: &mut Vec<WorkerStateAnomalyRecord>,
) -> Result<(), String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                state.id,
                NULL AS lease_id,
                state.entity_id,
                state.stage_id,
                stage.resource_class,
                state.status,
                NULL AS lease_status,
                NULL AS worker_id,
                run.run_id,
                NULL AS lease_until,
                NULL AS heartbeat_at,
                state.last_started_at,
                state.last_finished_at,
                CASE
                    WHEN state.status = 'in_progress'
                     AND run.run_id IS NOT NULL
                     AND run.started_at > ?1
                    THEN 'recent_unleased_in_progress'
                    WHEN state.status = 'in_progress' THEN 'in_progress_without_active_lease'
                    ELSE 'queued_without_active_lease'
                END AS diagnosis
            FROM entity_stage_states state
            JOIN stages stage ON stage.stage_id = state.stage_id
            JOIN entities entity ON entity.entity_id = state.entity_id
            LEFT JOIN stage_runs run ON run.run_id = (
                SELECT latest.run_id
                FROM stage_runs latest
                WHERE latest.entity_id = state.entity_id
                  AND latest.stage_id = state.stage_id
                  AND COALESCE(latest.entity_file_id, state.file_instance_id) = state.file_instance_id
                  AND latest.finished_at IS NULL
                ORDER BY latest.started_at DESC, latest.id DESC
                LIMIT 1
            )
            WHERE state.status IN ('queued', 'in_progress')
              AND stage.is_active = 1
              AND entity.is_archived = 0
              AND NOT EXISTS (
                    SELECT 1
                    FROM worker_leases active
                    WHERE active.state_id = state.id
                      AND active.status = 'active'
              )
            "#,
        )
        .map_err(|error| format!("Failed to prepare worker state anomaly query: {error}"))?;
    let rows = statement
        .query_map(params![recent_in_progress_cutoff], |row| {
            let diagnosis = row.get::<_, String>(13)?;
            let recommended_action = if diagnosis == "recent_unleased_in_progress" {
                "Wait for the recent unfinished run or inspect the worker before reconciling."
            } else {
                "Run reconcile stuck worker states."
            };
            worker_state_anomaly_from_row_with_diagnosis(row, diagnosis, recommended_action)
        })
        .map_err(|error| format!("Failed to query worker state anomalies: {error}"))?;
    for row in rows {
        anomalies.push(
            row.map_err(|error| format!("Failed to read worker state anomaly row: {error}"))?,
        );
    }
    Ok(())
}

fn worker_state_anomaly_from_row(
    row: &rusqlite::Row<'_>,
    recommended_action: &str,
) -> rusqlite::Result<WorkerStateAnomalyRecord> {
    let diagnosis = row.get::<_, String>(13)?;
    worker_state_anomaly_from_row_with_diagnosis(row, diagnosis, recommended_action)
}

fn worker_state_anomaly_from_row_with_diagnosis(
    row: &rusqlite::Row<'_>,
    diagnosis: String,
    recommended_action: &str,
) -> rusqlite::Result<WorkerStateAnomalyRecord> {
    let resource_class_text: String = row.get(4)?;
    Ok(WorkerStateAnomalyRecord {
        state_id: row.get(0)?,
        lease_id: row.get(1)?,
        entity_id: row.get(2)?,
        stage_id: row.get(3)?,
        resource_class: ResourceClass::from_str(&resource_class_text).unwrap_or_default(),
        state_status: row.get(5)?,
        lease_status: row.get(6)?,
        worker_id: row.get(7)?,
        run_id: row.get(8)?,
        lease_until: row.get(9)?,
        heartbeat_at: row.get(10)?,
        last_started_at: row.get(11)?,
        last_finished_at: row.get(12)?,
        diagnosis,
        recommended_action: recommended_action.to_string(),
    })
}

fn find_stage_state_by_id(
    connection: &Connection,
    state_id: i64,
) -> Result<Option<EntityStageStateRecord>, String> {
    connection
        .query_row(
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
            WHERE id = ?1
            "#,
            params![state_id],
            stage_state_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load stage state id '{state_id}': {error}"))
}

fn lease_run_finished(connection: &Connection, run_id: Option<&str>) -> Result<bool, String> {
    let Some(run_id) = run_id else {
        return Ok(false);
    };
    query_count(
        connection,
        "SELECT COUNT(*) FROM stage_runs WHERE run_id = ?1 AND finished_at IS NOT NULL",
        params![run_id],
    )
    .map(|count| count > 0)
}

fn lease_count(
    connection: &Connection,
    resource_class: ResourceClass,
    status: &str,
) -> Result<u64, String> {
    query_count(
        connection,
        r#"
        SELECT COUNT(*)
        FROM worker_leases lease
        JOIN entity_stage_states state ON state.id = lease.state_id
        JOIN stages stage ON stage.stage_id = lease.stage_id
        JOIN entity_files file ON file.id = lease.entity_file_id
        JOIN entities entity ON entity.entity_id = lease.entity_id
        WHERE lease.resource_class = ?1
          AND lease.status = ?2
          AND stage.resource_class = ?1
          AND stage.is_active = 1
          AND entity.is_archived = 0
          AND state.file_exists = 1
          AND file.file_exists = 1
        "#,
        params![resource_class.as_str(), status],
    )
}

fn worker_pool_summary(
    connection: &Connection,
    resource_class: ResourceClass,
    configured_concurrency: u32,
    now: &str,
) -> Result<WorkerPoolRuntimeSummary, String> {
    let active_leases = lease_count(connection, resource_class, "active")?;
    let expired_leases = lease_count(connection, resource_class, "expired")?;
    let control = worker_pool_control_state(connection, resource_class)?;
    let effective_concurrency = configured_concurrency.min(control.desired_concurrency);
    Ok(WorkerPoolRuntimeSummary {
        resource_class,
        configured_concurrency,
        env_concurrency_limit: None,
        requested_desired_concurrency: None,
        desired_concurrency: control.desired_concurrency,
        effective_concurrency,
        is_started: control.is_started,
        active_leases,
        expired_leases,
        pending_count: queue_count(connection, resource_class, "pending", now)?,
        retry_wait_due_count: retry_wait_count(connection, resource_class, true, now)?,
        retry_wait_not_due_count: retry_wait_count(connection, resource_class, false, now)?,
        queued_count: queue_count(connection, resource_class, "queued", now)?,
        in_progress_count: queue_count(connection, resource_class, "in_progress", now)?,
        blocked_count: queue_count(connection, resource_class, "blocked", now)?,
        failed_count: queue_count(connection, resource_class, "failed", now)?,
        is_paused: control.is_paused,
        pause_reason: control.pause_reason,
        oldest_pending_age_sec: oldest_pending_age_sec(connection, resource_class, now)?,
        average_duration_ms: average_duration_ms(connection, resource_class)?,
        last_error: last_worker_pool_error(connection, resource_class)?,
    })
}

fn queue_count(
    connection: &Connection,
    resource_class: ResourceClass,
    status: &str,
    now: &str,
) -> Result<u64, String> {
    query_count(
        connection,
        r#"
        SELECT COUNT(*)
        FROM entity_stage_states state
        JOIN stages stage ON stage.stage_id = state.stage_id
        JOIN entity_files file ON file.id = state.file_instance_id
        JOIN entities entity ON entity.entity_id = state.entity_id
        WHERE stage.resource_class = ?1
          AND stage.is_active = 1
          AND entity.is_archived = 0
          AND state.file_exists = 1
          AND file.file_exists = 1
          AND TRIM(stage.workflow_url) <> ''
          AND state.status = ?2
          AND (?2 != 'retry_wait' OR state.next_retry_at IS NULL OR state.next_retry_at <= ?3)
        "#,
        params![resource_class.as_str(), status, now],
    )
}

fn retry_wait_count(
    connection: &Connection,
    resource_class: ResourceClass,
    due: bool,
    now: &str,
) -> Result<u64, String> {
    let due_clause = if due {
        "state.next_retry_at IS NOT NULL AND state.next_retry_at <= ?2"
    } else {
        "state.next_retry_at IS NULL OR state.next_retry_at > ?2"
    };
    query_count(
        connection,
        &format!(
            r#"
            SELECT COUNT(*)
            FROM entity_stage_states state
            JOIN stages stage ON stage.stage_id = state.stage_id
            JOIN entity_files file ON file.id = state.file_instance_id
            JOIN entities entity ON entity.entity_id = state.entity_id
            WHERE stage.resource_class = ?1
              AND stage.is_active = 1
              AND entity.is_archived = 0
              AND state.file_exists = 1
              AND file.file_exists = 1
              AND TRIM(stage.workflow_url) <> ''
              AND state.status = 'retry_wait'
              AND ({due_clause})
            "#
        ),
        params![resource_class.as_str(), now],
    )
}

fn oldest_pending_age_sec(
    connection: &Connection,
    resource_class: ResourceClass,
    now: &str,
) -> Result<Option<u64>, String> {
    connection
        .query_row(
            r#"
            SELECT CAST((julianday(?2) - julianday(MIN(state.updated_at))) * 86400 AS INTEGER)
            FROM entity_stage_states state
            JOIN stages stage ON stage.stage_id = state.stage_id
            JOIN entity_files file ON file.id = state.file_instance_id
            JOIN entities entity ON entity.entity_id = state.entity_id
            WHERE stage.resource_class = ?1
              AND stage.is_active = 1
              AND entity.is_archived = 0
              AND state.file_exists = 1
              AND file.file_exists = 1
              AND TRIM(stage.workflow_url) <> ''
              AND (
                    state.status = 'pending'
                    OR (
                        state.status = 'retry_wait'
                        AND state.next_retry_at IS NOT NULL
                        AND state.next_retry_at <= ?2
                    )
                  )
            "#,
            params![resource_class.as_str(), now],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map(|value| value.and_then(|seconds| u64::try_from(seconds.max(0)).ok()))
        .map_err(|error| {
            format!(
                "Failed to query oldest pending age for worker pool '{}': {error}",
                resource_class.as_str()
            )
        })
}

fn average_duration_ms(
    connection: &Connection,
    resource_class: ResourceClass,
) -> Result<Option<u64>, String> {
    connection
        .query_row(
            r#"
            SELECT AVG(run.duration_ms)
            FROM stage_runs run
            JOIN stages stage ON stage.stage_id = run.stage_id
            WHERE stage.resource_class = ?1
              AND run.duration_ms IS NOT NULL
              AND run.finished_at IS NOT NULL
            "#,
            params![resource_class.as_str()],
            |row| row.get::<_, Option<f64>>(0),
        )
        .map(|value| value.map(|duration| duration.round().max(0.0) as u64))
        .map_err(|error| {
            format!(
                "Failed to query average duration for worker pool '{}': {error}",
                resource_class.as_str()
            )
        })
}

fn last_worker_pool_error(
    connection: &Connection,
    resource_class: ResourceClass,
) -> Result<Option<String>, String> {
    connection
        .query_row(
            r#"
            SELECT state.last_error
            FROM entity_stage_states state
            JOIN stages stage ON stage.stage_id = state.stage_id
            JOIN entity_files file ON file.id = state.file_instance_id
            JOIN entities entity ON entity.entity_id = state.entity_id
            WHERE stage.resource_class = ?1
              AND stage.is_active = 1
              AND entity.is_archived = 0
              AND state.file_exists = 1
              AND file.file_exists = 1
              AND TRIM(COALESCE(state.last_error, '')) <> ''
            ORDER BY state.updated_at DESC, state.id DESC
            LIMIT 1
            "#,
            params![resource_class.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| {
            format!(
                "Failed to query last error for worker pool '{}': {error}",
                resource_class.as_str()
            )
        })
}

fn list_recent_worker_leases(
    connection: &Connection,
    limit: u64,
) -> Result<Vec<WorkerLeaseRecord>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                lease_id,
                state_id,
                entity_id,
                entity_file_id,
                stage_id,
                resource_class,
                worker_id,
                status,
                run_id,
                leased_at,
                lease_until,
                heartbeat_at,
                released_at,
                release_reason,
                updated_at
            FROM worker_leases
            ORDER BY updated_at DESC, id DESC
            LIMIT ?1
            "#,
        )
        .map_err(|error| format!("Failed to prepare recent worker leases query: {error}"))?;
    let rows = statement
        .query_map(params![limit as i64], worker_lease_from_row)
        .map_err(|error| format!("Failed to query recent worker leases: {error}"))?;
    let mut leases = Vec::new();
    for row in rows {
        leases.push(row.map_err(|error| format!("Failed to read worker lease row: {error}"))?);
    }
    Ok(leases)
}

fn find_worker_lease_by_id(
    connection: &Connection,
    lease_id: &str,
) -> Result<Option<WorkerLeaseRecord>, String> {
    connection
        .query_row(
            r#"
            SELECT
                lease_id,
                state_id,
                entity_id,
                entity_file_id,
                stage_id,
                resource_class,
                worker_id,
                status,
                run_id,
                leased_at,
                lease_until,
                heartbeat_at,
                released_at,
                release_reason,
                updated_at
            FROM worker_leases
            WHERE lease_id = ?1
            "#,
            params![lease_id],
            worker_lease_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load worker lease '{lease_id}': {error}"))
}

fn find_active_worker_lease_by_state_id(
    connection: &Connection,
    state_id: i64,
) -> Result<Option<WorkerLeaseRecord>, String> {
    connection
        .query_row(
            r#"
            SELECT
                lease_id,
                state_id,
                entity_id,
                entity_file_id,
                stage_id,
                resource_class,
                worker_id,
                status,
                run_id,
                leased_at,
                lease_until,
                heartbeat_at,
                released_at,
                release_reason,
                updated_at
            FROM worker_leases
            WHERE state_id = ?1
              AND status = 'active'
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#,
            params![state_id],
            worker_lease_from_row,
        )
        .optional()
        .map_err(|error| {
            format!("Failed to load active worker lease for state '{state_id}': {error}")
        })
}

fn worker_lease_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkerLeaseRecord> {
    let resource_class_text: String = row.get(5)?;
    Ok(WorkerLeaseRecord {
        lease_id: row.get(0)?,
        state_id: row.get(1)?,
        entity_id: row.get(2)?,
        entity_file_id: row.get(3)?,
        stage_id: row.get(4)?,
        resource_class: ResourceClass::from_str(&resource_class_text).unwrap_or_default(),
        worker_id: row.get(6)?,
        status: row.get(7)?,
        run_id: row.get(8)?,
        leased_at: row.get(9)?,
        lease_until: row.get(10)?,
        heartbeat_at: row.get(11)?,
        released_at: row.get(12)?,
        release_reason: row.get(13)?,
        updated_at: row.get(14)?,
    })
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
    update_stage_state_success_with_reason(
        connection,
        state_id,
        http_status,
        finished_at,
        created_child_path,
        RuntimeTransitionReason::RuntimeSuccess,
    )
}

fn update_stage_state_success_with_reason(
    connection: &Connection,
    state_id: i64,
    http_status: Option<i64>,
    finished_at: &str,
    created_child_path: Option<&str>,
    reason: RuntimeTransitionReason,
) -> Result<(), String> {
    ensure_state_transition(connection, state_id, &StageStatus::Done, reason)?;
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

#[allow(dead_code)]
pub(crate) fn start_claimed_stage_run(
    connection: &mut Connection,
    state_id: i64,
    input: &NewStageRunInput,
) -> Result<(), String> {
    start_claimed_stage_run_and_attach_lease(connection, state_id, input, None)
}

pub(crate) fn start_claimed_stage_run_and_attach_lease(
    connection: &mut Connection,
    state_id: i64,
    input: &NewStageRunInput,
    lease_id: Option<&str>,
) -> Result<(), String> {
    let gate_key = sqlite_write_gate_key_for_connection(connection)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    retry_sqlite_busy_operation(|| {
        with_sqlite_write_gate(&gate, &gate_key, "start claimed stage run", || {
            start_claimed_stage_run_once(connection, state_id, input, lease_id)
        })
    })
}

fn start_claimed_stage_run_once(
    connection: &mut Connection,
    state_id: i64,
    input: &NewStageRunInput,
    lease_id: Option<&str>,
) -> Result<(), String> {
    let transaction = immediate_transaction(connection, "stage-run start")?;
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
    if let Some(lease_id) = lease_id {
        attach_worker_lease_run(&transaction, lease_id, &input.run_id, &input.started_at)?;
    }
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
                && existing.artifact_id == file.artifact_id
                && existing.relation_to_source == file.relation_to_source
                && existing.storage_provider == file.storage_provider
                && existing.bucket == file.bucket
                && existing.key == file.key
                && existing.version_id == file.version_id
                && existing.etag == file.etag
                && existing.checksum_sha256 == file.checksum_sha256
                && existing.artifact_size == file.artifact_size
                && existing.payload_json == file.payload_json
                && existing.meta_json == file.meta_json
                && existing.validation_status == file.validation_status
                && existing.validation_errors == file.validation_errors
                && existing.file_exists
                && existing.is_managed_copy == file.is_managed_copy
                && existing.copy_source_file_id == file.copy_source_file_id
                && existing.producer_run_id == file.producer_run_id =>
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
                        artifact_id = ?5,
                        relation_to_source = ?6,
                        storage_provider = ?7,
                        bucket = ?8,
                        object_key = ?9,
                        version_id = ?10,
                        etag = ?11,
                        checksum_sha256 = ?12,
                        checksum = ?13,
                        file_mtime = ?14,
                        file_size = ?15,
                        artifact_size = ?16,
                        payload_json = ?17,
                        meta_json = ?18,
                        current_stage = ?19,
                        next_stage = ?20,
                        status = ?21,
                        validation_status = ?22,
                        validation_errors_json = ?23,
                        is_managed_copy = ?24,
                        copy_source_file_id = ?25,
                        producer_run_id = ?26,
                        file_exists = 1,
                        missing_since = NULL,
                        last_seen_at = ?27,
                        updated_at = ?28
                    WHERE id = ?1
                    "#,
                    params![
                        existing.id,
                        file.entity_id,
                        file.stage_id,
                        file.file_name,
                        file.artifact_id,
                        file.relation_to_source,
                        file.storage_provider.as_str(),
                        file.bucket,
                        file.key,
                        file.version_id,
                        file.etag,
                        file.checksum_sha256,
                        file.checksum,
                        file.file_mtime,
                        file.file_size as i64,
                        file.artifact_size.map(|value| value as i64),
                        file.payload_json,
                        file.meta_json,
                        file.current_stage,
                        file.next_stage,
                        status,
                        validation_status,
                        serialized_errors,
                        bool_to_i64(file.is_managed_copy),
                        file.copy_source_file_id,
                        file.producer_run_id,
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
                        artifact_id,
                        relation_to_source,
                        storage_provider,
                        bucket,
                        object_key,
                        version_id,
                        etag,
                        checksum_sha256,
                        checksum,
                        file_mtime,
                        file_size,
                        artifact_size,
                        payload_json,
                        meta_json,
                        current_stage,
                        next_stage,
                        status,
                        validation_status,
                        validation_errors_json,
                        is_managed_copy,
                        copy_source_file_id,
                        producer_run_id,
                        file_exists,
                        missing_since,
                        first_seen_at,
                        last_seen_at,
                        updated_at
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, 1, NULL, ?27, ?28, ?29)
                    "#,
                    params![
                        file.entity_id,
                        file.stage_id,
                        file.file_path,
                        file.file_name,
                        file.artifact_id,
                        file.relation_to_source,
                        file.storage_provider.as_str(),
                        file.bucket,
                        file.key,
                        file.version_id,
                        file.etag,
                        file.checksum_sha256,
                        file.checksum,
                        file.file_mtime,
                        file.file_size as i64,
                        file.artifact_size.map(|value| value as i64),
                        file.payload_json,
                        file.meta_json,
                        file.current_stage,
                        file.next_stage,
                        status,
                        validation_status,
                        serialized_errors,
                        bool_to_i64(file.is_managed_copy),
                        file.copy_source_file_id,
                        file.producer_run_id,
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

#[cfg(test)]
pub(crate) fn register_s3_artifact_pointer(
    path: &Path,
    input: &RegisterS3ArtifactPointerInput,
) -> Result<EntityFileRecord, String> {
    let mut files = register_s3_artifact_pointers(path, std::slice::from_ref(input))?;
    files
        .pop()
        .ok_or_else(|| "S3 artifact registration returned no rows.".to_string())
}

pub(crate) fn register_s3_artifact_pointers(
    path: &Path,
    inputs: &[RegisterS3ArtifactPointerInput],
) -> Result<Vec<EntityFileRecord>, String> {
    let gate_key = sqlite_write_gate_key_for_path(path)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    retry_sqlite_busy_operation(|| {
        with_sqlite_write_gate(&gate, &gate_key, "S3 artifact pointer registration", || {
            register_s3_artifact_pointers_once(path, inputs)
        })
    })
}

fn register_s3_artifact_pointers_once(
    path: &Path,
    inputs: &[RegisterS3ArtifactPointerInput],
) -> Result<Vec<EntityFileRecord>, String> {
    let mut connection = open_connection(path)?;
    let transaction = immediate_transaction(&mut connection, "S3 artifact registration")?;
    validate_s3_artifact_registration_batch(&transaction, inputs)?;

    let mut file_ids = Vec::new();
    let mut affected_entity_ids = HashSet::new();
    for input in inputs {
        file_ids.push(register_s3_artifact_pointer_in_transaction(
            &transaction,
            input,
        )?);
        affected_entity_ids.insert(input.entity_id.clone());
    }
    recompute_entity_summaries_for_entity_ids(&transaction, &affected_entity_ids)?;

    let mut files = Vec::new();
    for file_id in file_ids {
        files.push(
            find_entity_file_by_id(&transaction, file_id)?.ok_or_else(|| {
                format!("Registered S3 artifact row '{}' was not found.", file_id)
            })?,
        );
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit S3 artifact registration: {error}"))?;
    Ok(files)
}

pub(crate) fn register_s3_artifact_pointers_best_effort(
    path: &Path,
    inputs: &[RegisterS3ArtifactPointerInput],
) -> Result<(Vec<EntityFileRecord>, OutputRegistrationReport), String> {
    let gate_key = sqlite_write_gate_key_for_path(path)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    let batch_size = s3_registration_write_batch_size();
    let mut registered_files = Vec::new();
    let mut report = OutputRegistrationReport::default();

    for chunk in inputs.chunks(batch_size) {
        let (chunk_files, chunk_report) = retry_sqlite_busy_operation(|| {
            with_sqlite_write_gate(&gate, &gate_key, "partial S3 artifact registration", || {
                register_s3_artifact_pointers_best_effort_once(path, chunk)
            })
        })?;
        registered_files.extend(chunk_files);
        merge_output_registration_report(&mut report, chunk_report);
    }

    Ok((registered_files, report))
}

fn s3_registration_write_batch_size() -> usize {
    env_u64(
        "BEEHIVE_S3_REGISTRATION_WRITE_BATCH_SIZE",
        DEFAULT_S3_REGISTRATION_WRITE_BATCH_SIZE,
    )
    .clamp(1, 1_000) as usize
}

fn merge_output_registration_report(
    target: &mut OutputRegistrationReport,
    source: OutputRegistrationReport,
) {
    target.registered_count += source.registered_count;
    target.skipped_count += source.skipped_count;
    target.invalid_count += source.invalid_count;
    target.conflict_count += source.conflict_count;
    target.failed_count += source.failed_count;
    target.outputs.extend(source.outputs);
}

fn register_s3_artifact_pointers_best_effort_once(
    path: &Path,
    inputs: &[RegisterS3ArtifactPointerInput],
) -> Result<(Vec<EntityFileRecord>, OutputRegistrationReport), String> {
    let mut connection = open_connection(path)?;
    let transaction = immediate_transaction(&mut connection, "partial S3 artifact registration")?;

    let mut registered_files = Vec::new();
    let mut report = OutputRegistrationReport::default();

    for input in inputs {
        match precheck_s3_artifact_registration(&transaction, input)? {
            RegistrationPrecheck::Register => {
                match register_s3_artifact_pointer_in_transaction(&transaction, input) {
                    Ok(file_id) => {
                        let file =
                            find_entity_file_by_id(&transaction, file_id)?.ok_or_else(|| {
                                format!("Registered S3 artifact row '{}' was not found.", file_id)
                            })?;
                        report.registered_count += 1;
                        report.outputs.push(output_registration_item(
                            input,
                            "registered",
                            None::<String>,
                        ));
                        registered_files.push(file);
                    }
                    Err(message) => {
                        report.failed_count += 1;
                        report.outputs.push(output_registration_item(
                            input,
                            "failed",
                            Some(message),
                        ));
                    }
                }
            }
            RegistrationPrecheck::Idempotent(existing_id) => {
                let file = find_entity_file_by_id(&transaction, existing_id)?.ok_or_else(|| {
                    format!(
                        "Idempotent S3 artifact row '{}' was not found.",
                        existing_id
                    )
                })?;
                report.skipped_count += 1;
                report.outputs.push(output_registration_item(
                    input,
                    "idempotent_skipped",
                    Some("Output artifact is already registered with the same identity and location.".to_string()),
                ));
                registered_files.push(file);
            }
            RegistrationPrecheck::Invalid(message) => {
                report.invalid_count += 1;
                report
                    .outputs
                    .push(output_registration_item(input, "invalid", Some(message)));
            }
            RegistrationPrecheck::Conflict(message) => {
                report.conflict_count += 1;
                report
                    .outputs
                    .push(output_registration_item(input, "conflict", Some(message)));
            }
        }
    }

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit partial S3 artifact registration: {error}"))?;
    Ok((registered_files, report))
}

enum RegistrationPrecheck {
    Register,
    Idempotent(i64),
    Invalid(String),
    Conflict(String),
}

fn precheck_s3_artifact_registration(
    transaction: &Transaction<'_>,
    input: &RegisterS3ArtifactPointerInput,
) -> Result<RegistrationPrecheck, String> {
    if input.artifact_id.trim().is_empty() {
        return Ok(RegistrationPrecheck::Invalid(
            "S3 artifact registration requires non-empty artifact_id.".to_string(),
        ));
    }
    if input.entity_id.trim().is_empty() {
        return Ok(RegistrationPrecheck::Invalid(
            "S3 artifact registration requires non-empty entity_id.".to_string(),
        ));
    }
    if input.bucket.trim().is_empty() || input.key.trim().is_empty() {
        return Ok(RegistrationPrecheck::Invalid(
            "S3 artifact registration requires non-empty bucket/key.".to_string(),
        ));
    }

    let Some(stage) = find_stage_by_id(transaction, &input.stage_id)? else {
        return Ok(RegistrationPrecheck::Invalid(format!(
            "Target stage '{}' was not found.",
            input.stage_id
        )));
    };
    if !stage.is_active {
        return Ok(RegistrationPrecheck::Invalid(format!(
            "Target stage '{}' is inactive.",
            input.stage_id
        )));
    }

    if let Some(producer_run_id) = input
        .producer_run_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        if let Some(existing) = find_s3_artifact_by_producer_and_artifact(
            transaction,
            producer_run_id,
            &input.artifact_id,
        )? {
            if s3_registration_matches(&existing, input) {
                return Ok(RegistrationPrecheck::Idempotent(existing.id));
            }
            return Ok(RegistrationPrecheck::Conflict(format!(
                "S3 artifact '{}' for run '{}' is already registered with different identity or location.",
                input.artifact_id, producer_run_id
            )));
        }
    }

    let file_path = format!("s3://{}/{}", input.bucket, input.key);
    if let Some(existing) = find_entity_file_by_path(transaction, &file_path)? {
        if s3_registration_matches(&existing, input) {
            return Ok(RegistrationPrecheck::Idempotent(existing.id));
        }
        return Ok(RegistrationPrecheck::Conflict(format!(
            "S3 output key '{}' is already registered with different identity, artifact, producer, or stage.",
            file_path
        )));
    }

    if let Some(existing) =
        find_entity_file_by_entity_stage(transaction, &input.entity_id, &input.stage_id)?
    {
        if s3_registration_matches(&existing, input) {
            return Ok(RegistrationPrecheck::Idempotent(existing.id));
        }
        return Ok(RegistrationPrecheck::Conflict(format!(
            "Entity '{}' already has a different artifact registered on stage '{}'.",
            input.entity_id, input.stage_id
        )));
    }

    Ok(RegistrationPrecheck::Register)
}

fn s3_registration_matches(
    existing: &EntityFileRecord,
    input: &RegisterS3ArtifactPointerInput,
) -> bool {
    existing.storage_provider == StorageProvider::S3
        && existing.entity_id == input.entity_id
        && existing.stage_id == input.stage_id
        && existing.artifact_id.as_deref() == Some(input.artifact_id.as_str())
        && existing.bucket.as_deref() == Some(input.bucket.as_str())
        && existing.key.as_deref() == Some(input.key.as_str())
        && existing.producer_run_id == input.producer_run_id
}

fn output_registration_item(
    input: &RegisterS3ArtifactPointerInput,
    status: &str,
    message: Option<String>,
) -> OutputRegistrationReportItem {
    OutputRegistrationReportItem {
        artifact_id: Some(input.artifact_id.clone()).filter(|value| !value.trim().is_empty()),
        entity_id: Some(input.entity_id.clone()).filter(|value| !value.trim().is_empty()),
        target_stage_id: Some(input.stage_id.clone()).filter(|value| !value.trim().is_empty()),
        status: status.to_string(),
        message,
    }
}

fn validate_s3_artifact_registration_batch(
    transaction: &Transaction<'_>,
    inputs: &[RegisterS3ArtifactPointerInput],
) -> Result<(), String> {
    let mut artifact_ids = HashSet::new();
    let mut bucket_keys = HashMap::new();
    for input in inputs {
        if input.artifact_id.trim().is_empty() {
            return Err("S3 artifact registration requires non-empty artifact_id.".to_string());
        }
        if input.entity_id.trim().is_empty() {
            return Err("S3 artifact registration requires non-empty entity_id.".to_string());
        }
        if input.bucket.trim().is_empty() || input.key.trim().is_empty() {
            return Err("S3 artifact registration requires non-empty bucket/key.".to_string());
        }
        if !artifact_ids.insert(input.artifact_id.clone()) {
            return Err(format!(
                "S3 artifact_id '{}' appears more than once in one registration batch.",
                input.artifact_id
            ));
        }
        let bucket_key = (input.bucket.clone(), input.key.clone());
        if bucket_keys
            .insert(bucket_key.clone(), input.artifact_id.clone())
            .is_some()
        {
            return Err(format!(
                "S3 output key 's3://{}/{}' appears more than once in one registration batch.",
                bucket_key.0, bucket_key.1
            ));
        }

        let stage = find_stage_by_id(transaction, &input.stage_id)?
            .ok_or_else(|| format!("Target stage '{}' was not found.", input.stage_id))?;
        if !stage.is_active {
            return Err(format!("Target stage '{}' is inactive.", input.stage_id));
        }

        if let Some(producer_run_id) = input
            .producer_run_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            if let Some(existing) = find_s3_artifact_by_producer_and_artifact(
                transaction,
                producer_run_id,
                &input.artifact_id,
            )? {
                let same_location = existing.bucket.as_deref() == Some(input.bucket.as_str())
                    && existing.key.as_deref() == Some(input.key.as_str());
                if !same_location {
                    return Err(format!(
                        "S3 artifact '{}' for run '{}' is already registered at a different bucket/key.",
                        input.artifact_id, producer_run_id
                    ));
                }
                if existing.entity_id != input.entity_id {
                    return Err(format!(
                        "S3 artifact '{}' for run '{}' is already registered for entity '{}', not '{}'.",
                        input.artifact_id, producer_run_id, existing.entity_id, input.entity_id
                    ));
                }
                if existing.stage_id != input.stage_id {
                    return Err(format!(
                        "S3 artifact '{}' for run '{}' is already registered for stage '{}', not '{}'.",
                        input.artifact_id, producer_run_id, existing.stage_id, input.stage_id
                    ));
                }
            }
        }

        let file_path = format!("s3://{}/{}", input.bucket, input.key);
        if let Some(existing) = find_entity_file_by_path(transaction, &file_path)? {
            if existing.storage_provider != StorageProvider::S3 {
                return Err(format!(
                    "S3 output key '{}' collides with a non-S3 entity file row.",
                    file_path
                ));
            }
            if existing.entity_id != input.entity_id {
                return Err(format!(
                    "S3 output key '{}' is already registered for entity '{}', not '{}'.",
                    file_path, existing.entity_id, input.entity_id
                ));
            }
            if existing.artifact_id.as_deref() != Some(input.artifact_id.as_str()) {
                return Err(format!(
                    "S3 output key '{}' is already registered with artifact_id '{:?}', not '{}'.",
                    file_path, existing.artifact_id, input.artifact_id
                ));
            }
            if existing.producer_run_id != input.producer_run_id {
                return Err(format!(
                    "S3 output key '{}' is already registered with a different producer_run_id.",
                    file_path
                ));
            }
            if existing.stage_id != input.stage_id {
                return Err(format!(
                    "S3 output key '{}' is already registered for stage '{}', not '{}'.",
                    file_path, existing.stage_id, input.stage_id
                ));
            }
        }
    }
    Ok(())
}

fn register_s3_artifact_pointer_in_transaction(
    transaction: &Transaction<'_>,
    input: &RegisterS3ArtifactPointerInput,
) -> Result<i64, String> {
    let stage = find_stage_by_id(transaction, &input.stage_id)?
        .ok_or_else(|| format!("Target stage '{}' was not found.", input.stage_id))?;
    if !stage.is_active {
        return Err(format!("Target stage '{}' is inactive.", input.stage_id));
    }
    let now = Utc::now().to_rfc3339();
    ensure_entity_stub(&transaction, &input.entity_id, &now)?;
    let file_path = format!("s3://{}/{}", input.bucket, input.key);
    let file_name = input
        .key
        .rsplit('/')
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(input.artifact_id.as_str())
        .to_string();
    let checksum = input
        .checksum_sha256
        .clone()
        .unwrap_or_else(|| format!("s3-pointer:{}:{}", input.bucket, input.key));
    let meta_json = serde_json::to_string(&json!({
        "beehive": {
            "storage_provider": "s3",
            "artifact_id": input.artifact_id,
            "relation_to_source": input.relation_to_source,
            "source_file_id": input.source_file_id,
            "producer_run_id": input.producer_run_id,
        }
    }))
    .map_err(|error| format!("Failed to serialize S3 artifact metadata: {error}"))?;
    let file_mtime = input.last_modified.clone().unwrap_or_else(|| now.clone());
    let (_outcome, file_id) = upsert_entity_file(
        &transaction,
        &PersistEntityFileInput {
            entity_id: input.entity_id.clone(),
            stage_id: input.stage_id.clone(),
            file_path: file_path.clone(),
            file_name,
            artifact_id: Some(input.artifact_id.clone()),
            relation_to_source: input.relation_to_source.clone(),
            storage_provider: StorageProvider::S3,
            bucket: Some(input.bucket.clone()),
            key: Some(input.key.clone()),
            version_id: input.version_id.clone(),
            etag: input.etag.clone(),
            checksum_sha256: input.checksum_sha256.clone(),
            checksum,
            file_mtime,
            file_size: input.size.unwrap_or(0),
            artifact_size: input.size,
            payload_json: "{}".to_string(),
            meta_json,
            current_stage: Some(input.stage_id.clone()),
            next_stage: stage.next_stage.clone(),
            status: input.status.clone(),
            validation_status: EntityValidationStatus::Valid,
            validation_errors: Vec::new(),
            is_managed_copy: input.source_file_id.is_some(),
            copy_source_file_id: input.source_file_id,
            producer_run_id: input.producer_run_id.clone(),
            first_seen_at: now.clone(),
            last_seen_at: now.clone(),
            updated_at: now.clone(),
        },
    )?;
    upsert_entity_stage_state(
        &transaction,
        &PersistEntityStageStateInput {
            entity_id: input.entity_id.clone(),
            stage_id: input.stage_id.clone(),
            file_path,
            file_instance_id: Some(file_id),
            file_exists: true,
            status: input.status.clone(),
            max_attempts: stage.max_attempts,
            discovered_at: now.clone(),
            last_seen_at: now.clone(),
            updated_at: now.clone(),
        },
    )?;
    Ok(file_id)
}

fn find_s3_artifact_by_producer_and_artifact(
    connection: &Connection,
    producer_run_id: &str,
    artifact_id: &str,
) -> Result<Option<EntityFileRecord>, String> {
    connection
        .query_row(
            entity_files_select_sql(Some(
                "WHERE storage_provider = 's3' AND producer_run_id = ?1 AND artifact_id = ?2 ORDER BY id DESC LIMIT 1",
            )),
            params![producer_run_id, artifact_id],
            entity_file_from_row,
        )
        .optional()
        .map_err(|error| {
            format!(
                "Failed to load S3 artifact '{}' for producer run '{}': {error}",
                artifact_id, producer_run_id
            )
        })
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
        if file.storage_provider != StorageProvider::Local {
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

pub(crate) fn mark_missing_s3_files_for_active_stages(
    path: &Path,
    active_stage_ids: &HashSet<String>,
    seen_paths: &HashSet<String>,
    scan_id: &str,
    seen_at: &str,
) -> Result<u64, String> {
    let mut connection = open_connection(path)?;
    let transaction = connection.transaction().map_err(|error| {
        format!("Failed to start S3 missing artifact reconciliation transaction: {error}")
    })?;
    let existing_files = load_entity_files_from_connection(&transaction, None)?;
    let mut missing_count = 0;

    for file in existing_files {
        if !active_stage_ids.contains(&file.stage_id) {
            continue;
        }
        if file.storage_provider != StorageProvider::S3 {
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
                    "Failed to mark S3 artifact '{}' as missing: {error}",
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
                    "Failed to mark S3 stage state missing for entity '{}' on stage '{}': {error}",
                    file.entity_id, file.stage_id
                )
            })?;

        insert_app_event(
            &transaction,
            AppEventLevel::Warning,
            "s3_artifact_missing",
            &format!(
                "Tracked S3 artifact '{}' was not found during reconciliation.",
                file.file_path
            ),
            Some(json!({
                "scan_id": scan_id,
                "entity_id": file.entity_id,
                "stage_id": file.stage_id,
                "file_path": file.file_path,
                "bucket": file.bucket,
                "key": file.key,
            })),
            seen_at,
        )?;
        missing_count += 1;
    }

    if missing_count > 0 {
        recompute_entity_summaries(&transaction)?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit S3 missing artifact reconciliation: {error}"))?;

    Ok(missing_count)
}

pub(crate) fn recompute_entity_summaries(transaction: &Transaction<'_>) -> Result<(), String> {
    let entity_ids = load_entity_ids(transaction)?;
    let entity_ids = entity_ids.into_iter().collect::<HashSet<_>>();
    recompute_entity_summaries_for_entity_ids(transaction, &entity_ids)
}

pub(crate) fn recompute_entity_summary_for_entity_id(
    transaction: &Transaction<'_>,
    entity_id: &str,
) -> Result<(), String> {
    recompute_entity_summary(transaction, entity_id)
}

fn recompute_entity_summaries_for_entity_ids(
    transaction: &Transaction<'_>,
    entity_ids: &HashSet<String>,
) -> Result<(), String> {
    for entity_id in entity_ids {
        recompute_entity_summary(transaction, entity_id)?;
    }
    Ok(())
}

fn recompute_entity_summary(connection: &Connection, entity_id: &str) -> Result<(), String> {
    let files = load_entity_files_from_connection(connection, Some(entity_id))?;
    if files.is_empty() {
        return Ok(());
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
    let runtime_status = connection
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

    connection
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
        .map_err(|error| format!("Failed to recompute entity summary '{entity_id}': {error}"))?;
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
    if repeated_app_event_should_be_suppressed(connection, code, created_at)? {
        return Ok(());
    }

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

pub(crate) fn prune_runtime_history(path: &Path) -> Result<RuntimeRetentionPruneSummary, String> {
    if !env_flag_enabled("BEEHIVE_RUNTIME_RETENTION_ENABLED", true) {
        return Ok(RuntimeRetentionPruneSummary::default());
    }
    with_immediate_write_transaction_timeout(
        path,
        "runtime retention prune",
        StdDuration::from_millis(RUNTIME_RETENTION_WRITE_GATE_TIMEOUT_MS),
        |transaction| {
            let mut summary = RuntimeRetentionPruneSummary::default();
            summary.app_events_deleted += prune_app_events_by_age(transaction)?;
            summary.app_events_deleted += prune_app_events_by_count(transaction)?;
            summary.worker_leases_deleted += prune_worker_leases_by_age(transaction)?;
            summary.worker_leases_deleted += prune_worker_leases_by_count(transaction)?;
            Ok(summary)
        },
    )
}

fn repeated_app_event_should_be_suppressed(
    connection: &Connection,
    code: &str,
    created_at: &str,
) -> Result<bool, String> {
    let Some(window_sec) = app_event_throttle_window_sec(code) else {
        return Ok(false);
    };
    let Ok(created_at) = DateTime::parse_from_rfc3339(created_at) else {
        return Ok(false);
    };
    let cutoff = (created_at.with_timezone(&Utc) - Duration::seconds(window_sec)).to_rfc3339();
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM app_events WHERE code = ?1 AND created_at >= ?2 LIMIT 1)",
            params![code, cutoff],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists != 0)
        .map_err(|error| format!("Failed to check app event throttle for '{code}': {error}"))
}

fn app_event_throttle_window_sec(code: &str) -> Option<i64> {
    match code {
        "worker_internal_error_reconciled" => Some(30),
        "worker_lease_expired" => Some(10),
        "active_lease_with_finished_run_reconciled" => Some(10),
        _ => None,
    }
}

fn runtime_retention_delete_batch_limit() -> u64 {
    env_u64(
        "BEEHIVE_RUNTIME_RETENTION_DELETE_BATCH",
        DEFAULT_RUNTIME_RETENTION_DELETE_BATCH,
    )
}

fn prune_app_events_by_age(connection: &Connection) -> Result<u64, String> {
    let days = env_u64(
        "BEEHIVE_APP_EVENTS_RETENTION_DAYS",
        DEFAULT_APP_EVENTS_RETENTION_DAYS,
    );
    let cutoff = (Utc::now() - Duration::days(days as i64)).to_rfc3339();
    let batch_limit = runtime_retention_delete_batch_limit();
    connection
        .execute(
            r#"
            DELETE FROM app_events
            WHERE id IN (
                SELECT id FROM app_events
                WHERE created_at < ?1
                ORDER BY id ASC
                LIMIT ?2
            )
            "#,
            params![cutoff, batch_limit as i64],
        )
        .map(|deleted| deleted as u64)
        .map_err(|error| format!("Failed to prune app_events by age: {error}"))
}

fn prune_app_events_by_count(connection: &Connection) -> Result<u64, String> {
    let max_rows = env_u64("BEEHIVE_APP_EVENTS_MAX_ROWS", DEFAULT_APP_EVENTS_MAX_ROWS);
    let cutoff_id = connection
        .query_row(
            "SELECT id FROM app_events ORDER BY id DESC LIMIT 1 OFFSET ?1",
            params![max_rows as i64],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Failed to find app_events prune cutoff: {error}"))?;
    let Some(cutoff_id) = cutoff_id else {
        return Ok(0);
    };
    let batch_limit = runtime_retention_delete_batch_limit();
    connection
        .execute(
            r#"
            DELETE FROM app_events
            WHERE id IN (
                SELECT id FROM app_events
                WHERE id <= ?1
                ORDER BY id ASC
                LIMIT ?2
            )
            "#,
            params![cutoff_id, batch_limit as i64],
        )
        .map(|deleted| deleted as u64)
        .map_err(|error| format!("Failed to prune app_events by count: {error}"))
}

fn prune_worker_leases_by_age(connection: &Connection) -> Result<u64, String> {
    let days = env_u64(
        "BEEHIVE_WORKER_LEASES_RETENTION_DAYS",
        DEFAULT_WORKER_LEASES_RETENTION_DAYS,
    );
    let cutoff = (Utc::now() - Duration::days(days as i64)).to_rfc3339();
    let batch_limit = runtime_retention_delete_batch_limit();
    connection
        .execute(
            r#"
            DELETE FROM worker_leases
            WHERE id IN (
                SELECT id FROM worker_leases
                WHERE status <> 'active'
                  AND COALESCE(released_at, updated_at, lease_until, heartbeat_at, leased_at) < ?1
                ORDER BY id ASC
                LIMIT ?2
            )
            "#,
            params![cutoff, batch_limit as i64],
        )
        .map(|deleted| deleted as u64)
        .map_err(|error| format!("Failed to prune worker_leases by age: {error}"))
}

fn prune_worker_leases_by_count(connection: &Connection) -> Result<u64, String> {
    let max_rows = env_u64(
        "BEEHIVE_WORKER_LEASES_MAX_ROWS",
        DEFAULT_WORKER_LEASES_MAX_ROWS,
    );
    let cutoff_id = connection
        .query_row(
            r#"
            SELECT id
            FROM worker_leases
            WHERE status <> 'active'
            ORDER BY id DESC
            LIMIT 1 OFFSET ?1
            "#,
            params![max_rows as i64],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| format!("Failed to find worker_leases prune cutoff: {error}"))?;
    let Some(cutoff_id) = cutoff_id else {
        return Ok(0);
    };
    let batch_limit = runtime_retention_delete_batch_limit();
    connection
        .execute(
            r#"
            DELETE FROM worker_leases
            WHERE id IN (
                SELECT id FROM worker_leases
                WHERE status <> 'active' AND id <= ?1
                ORDER BY id ASC
                LIMIT ?2
            )
            "#,
            params![cutoff_id, batch_limit as i64],
        )
        .map(|deleted| deleted as u64)
        .map_err(|error| format!("Failed to prune worker_leases by count: {error}"))
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
        0 => create_schema_v12(connection)?,
        1 => {
            migrate_v1_to_v2(connection)?;
            migrate_v2_to_v3(connection)?;
            migrate_v3_to_v4(connection)?;
            migrate_v4_to_v5(connection)?;
            migrate_v5_to_v6(connection)?;
            migrate_v6_to_v7(connection)?;
            migrate_v7_to_v8(connection)?;
            migrate_v8_to_v9(connection)?;
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        2 => {
            migrate_v2_to_v3(connection)?;
            migrate_v3_to_v4(connection)?;
            migrate_v4_to_v5(connection)?;
            migrate_v5_to_v6(connection)?;
            migrate_v6_to_v7(connection)?;
            migrate_v7_to_v8(connection)?;
            migrate_v8_to_v9(connection)?;
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        3 => {
            migrate_v3_to_v4(connection)?;
            migrate_v4_to_v5(connection)?;
            migrate_v5_to_v6(connection)?;
            migrate_v6_to_v7(connection)?;
            migrate_v7_to_v8(connection)?;
            migrate_v8_to_v9(connection)?;
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        4 => {
            migrate_v4_to_v5(connection)?;
            migrate_v5_to_v6(connection)?;
            migrate_v6_to_v7(connection)?;
            migrate_v7_to_v8(connection)?;
            migrate_v8_to_v9(connection)?;
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        5 => {
            migrate_v5_to_v6(connection)?;
            migrate_v6_to_v7(connection)?;
            migrate_v7_to_v8(connection)?;
            migrate_v8_to_v9(connection)?;
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        6 => {
            migrate_v6_to_v7(connection)?;
            migrate_v7_to_v8(connection)?;
            migrate_v8_to_v9(connection)?;
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        7 => {
            migrate_v7_to_v8(connection)?;
            migrate_v8_to_v9(connection)?;
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        8 => {
            migrate_v8_to_v9(connection)?;
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        9 => {
            migrate_v9_to_v10(connection)?;
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        10 => {
            migrate_v10_to_v11(connection)?;
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        11 => {
            migrate_v11_to_v12(connection)?;
            migrate_v12_to_v13(connection)?;
        }
        12 => migrate_v12_to_v13(connection)?,
        13 => {}
        version => {
            return Err(format!(
            "Unsupported SQLite schema version '{version}'. Expected 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, or 13."
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
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_worker_claim ON entity_stage_states(status, file_exists, updated_at, id);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_worker_retry_claim ON entity_stage_states(status, next_retry_at, file_exists, updated_at, id);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_entity_stage ON entity_stage_states(entity_id, stage_id);
            CREATE INDEX IF NOT EXISTS idx_entities_updated_at ON entities(updated_at);
            CREATE INDEX IF NOT EXISTS idx_entities_current_stage_status ON entities(current_stage_id, current_status);
            CREATE INDEX IF NOT EXISTS idx_entities_archived_updated_at ON entities(is_archived, updated_at);
            CREATE INDEX IF NOT EXISTS idx_stage_runs_started_at ON stage_runs(started_at);
            CREATE INDEX IF NOT EXISTS idx_stage_runs_entity_stage ON stage_runs(entity_id, stage_id);
            CREATE INDEX IF NOT EXISTS idx_app_events_level_created_at ON app_events(level, created_at);
            CREATE INDEX IF NOT EXISTS idx_entity_files_storage_key ON entity_files(storage_provider, bucket, object_key);
            CREATE INDEX IF NOT EXISTS idx_worker_leases_status_until ON worker_leases(status, lease_until);
            CREATE INDEX IF NOT EXISTS idx_worker_leases_resource_status ON worker_leases(resource_class, status);
            CREATE INDEX IF NOT EXISTS idx_worker_leases_state_status ON worker_leases(state_id, status);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_worker_leases_one_active_state ON worker_leases(state_id) WHERE status = 'active';
            CREATE UNIQUE INDEX IF NOT EXISTS idx_entity_files_s3_producer_artifact ON entity_files(producer_run_id, artifact_id)
                WHERE storage_provider = 's3' AND producer_run_id IS NOT NULL AND artifact_id IS NOT NULL;
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

fn create_schema_v12(connection: &Connection) -> Result<(), String> {
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
                input_uri TEXT,
                output_folder TEXT NOT NULL,
                workflow_url TEXT NOT NULL,
                max_attempts INTEGER NOT NULL CHECK (max_attempts >= 1),
                retry_delay_sec INTEGER NOT NULL CHECK (retry_delay_sec >= 0),
                next_stage TEXT,
                save_path_aliases_json TEXT NOT NULL DEFAULT '[]',
                resource_class TEXT NOT NULL DEFAULT 'default' CHECK (resource_class IN ('default', 'local_llm')),
                allow_empty_outputs INTEGER NOT NULL DEFAULT 0,
                allow_multiple_outputs INTEGER NOT NULL DEFAULT 0,
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
                display_name TEXT,
                operator_note TEXT,
                is_archived INTEGER NOT NULL DEFAULT 0,
                archived_at TEXT,
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
                artifact_id TEXT,
                relation_to_source TEXT,
                storage_provider TEXT NOT NULL DEFAULT 'local',
                bucket TEXT,
                object_key TEXT,
                version_id TEXT,
                etag TEXT,
                checksum_sha256 TEXT,
                checksum TEXT NOT NULL,
                file_mtime TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                artifact_size INTEGER,
                payload_json TEXT NOT NULL DEFAULT '{}',
                meta_json TEXT NOT NULL DEFAULT '{}',
                current_stage TEXT,
                next_stage TEXT,
                status TEXT NOT NULL,
                validation_status TEXT NOT NULL,
                validation_errors_json TEXT NOT NULL DEFAULT '[]',
                is_managed_copy INTEGER NOT NULL DEFAULT 0,
                copy_source_file_id INTEGER,
                producer_run_id TEXT,
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

            CREATE TABLE IF NOT EXISTS worker_leases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                lease_id TEXT NOT NULL UNIQUE,
                state_id INTEGER NOT NULL,
                entity_id TEXT NOT NULL,
                entity_file_id INTEGER NOT NULL,
                stage_id TEXT NOT NULL,
                resource_class TEXT NOT NULL CHECK (resource_class IN ('default', 'local_llm')),
                worker_id TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('active', 'done', 'failed', 'expired', 'released')),
                run_id TEXT,
                leased_at TEXT NOT NULL,
                lease_until TEXT NOT NULL,
                heartbeat_at TEXT NOT NULL,
                released_at TEXT,
                release_reason TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (state_id) REFERENCES entity_stage_states(id),
                FOREIGN KEY (entity_file_id) REFERENCES entity_files(id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id)
            );

            CREATE TABLE IF NOT EXISTS worker_pool_controls (
                resource_class TEXT PRIMARY KEY CHECK (resource_class IN ('default', 'local_llm')),
                desired_concurrency INTEGER NOT NULL DEFAULT 0,
                is_started INTEGER NOT NULL DEFAULT 0,
                is_paused INTEGER NOT NULL DEFAULT 0,
                pause_reason TEXT,
                updated_at TEXT NOT NULL
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
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_worker_claim ON entity_stage_states(status, file_exists, updated_at, id);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_worker_retry_claim ON entity_stage_states(status, next_retry_at, file_exists, updated_at, id);
            CREATE INDEX IF NOT EXISTS idx_worker_leases_status_until ON worker_leases(status, lease_until);
            CREATE INDEX IF NOT EXISTS idx_worker_leases_resource_status ON worker_leases(resource_class, status);
            CREATE INDEX IF NOT EXISTS idx_worker_leases_state_status ON worker_leases(state_id, status);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_worker_leases_one_active_state ON worker_leases(state_id) WHERE status = 'active';

            CREATE UNIQUE INDEX IF NOT EXISTS idx_entity_files_s3_producer_artifact ON entity_files(producer_run_id, artifact_id)
                WHERE storage_provider = 's3' AND producer_run_id IS NOT NULL AND artifact_id IS NOT NULL;

            PRAGMA user_version = 13;
            "#,
        )
        .map_err(|error| format!("Failed to create SQLite schema v13: {error}"))?;
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

fn migrate_v4_to_v5(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            ALTER TABLE stages ADD COLUMN input_uri TEXT;
            ALTER TABLE stages ADD COLUMN save_path_aliases_json TEXT NOT NULL DEFAULT '[]';

            ALTER TABLE entity_files ADD COLUMN storage_provider TEXT NOT NULL DEFAULT 'local';
            ALTER TABLE entity_files ADD COLUMN bucket TEXT;
            ALTER TABLE entity_files ADD COLUMN object_key TEXT;
            ALTER TABLE entity_files ADD COLUMN version_id TEXT;
            ALTER TABLE entity_files ADD COLUMN etag TEXT;
            ALTER TABLE entity_files ADD COLUMN checksum_sha256 TEXT;
            ALTER TABLE entity_files ADD COLUMN artifact_size INTEGER;
            ALTER TABLE entity_files ADD COLUMN producer_run_id TEXT;

            PRAGMA user_version = 5;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v4 to v5: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v5",
        "SQLite schema migrated from version 4 to version 5.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "5", &now)?;
    Ok(())
}

fn migrate_v5_to_v6(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            ALTER TABLE stages ADD COLUMN allow_empty_outputs INTEGER NOT NULL DEFAULT 0;

            ALTER TABLE entity_files ADD COLUMN artifact_id TEXT;
            ALTER TABLE entity_files ADD COLUMN relation_to_source TEXT;

            CREATE UNIQUE INDEX IF NOT EXISTS idx_entity_files_s3_producer_artifact ON entity_files(producer_run_id, artifact_id)
                WHERE storage_provider = 's3' AND producer_run_id IS NOT NULL AND artifact_id IS NOT NULL;

            PRAGMA user_version = 6;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v5 to v6: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v6",
        "SQLite schema migrated from version 5 to version 6.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "6", &now)?;
    Ok(())
}

fn migrate_v6_to_v7(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            ALTER TABLE entities ADD COLUMN display_name TEXT;
            ALTER TABLE entities ADD COLUMN operator_note TEXT;
            ALTER TABLE entities ADD COLUMN is_archived INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE entities ADD COLUMN archived_at TEXT;

            CREATE INDEX IF NOT EXISTS idx_entities_archived_updated_at ON entities(is_archived, updated_at);

            PRAGMA user_version = 7;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v6 to v7: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v7",
        "SQLite schema migrated from version 6 to version 7.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "7", &now)?;
    Ok(())
}

fn migrate_v7_to_v8(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            ALTER TABLE stages ADD COLUMN allow_multiple_outputs INTEGER NOT NULL DEFAULT 0;

            PRAGMA user_version = 8;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v7 to v8: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v8",
        "SQLite schema migrated from version 7 to version 8.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "8", &now)?;
    Ok(())
}

fn migrate_v8_to_v9(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            ALTER TABLE stages ADD COLUMN resource_class TEXT NOT NULL DEFAULT 'default' CHECK (resource_class IN ('default', 'local_llm'));

            PRAGMA user_version = 9;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v8 to v9: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v9",
        "SQLite schema migrated from version 8 to version 9.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "9", &now)?;
    Ok(())
}

fn migrate_v9_to_v10(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS worker_leases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                lease_id TEXT NOT NULL UNIQUE,
                state_id INTEGER NOT NULL,
                entity_id TEXT NOT NULL,
                entity_file_id INTEGER NOT NULL,
                stage_id TEXT NOT NULL,
                resource_class TEXT NOT NULL CHECK (resource_class IN ('default', 'local_llm')),
                worker_id TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('active', 'done', 'failed', 'expired', 'released')),
                run_id TEXT,
                leased_at TEXT NOT NULL,
                lease_until TEXT NOT NULL,
                heartbeat_at TEXT NOT NULL,
                released_at TEXT,
                release_reason TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (state_id) REFERENCES entity_stage_states(id),
                FOREIGN KEY (entity_file_id) REFERENCES entity_files(id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id)
            );

            CREATE INDEX IF NOT EXISTS idx_worker_leases_status_until ON worker_leases(status, lease_until);
            CREATE INDEX IF NOT EXISTS idx_worker_leases_resource_status ON worker_leases(resource_class, status);
            CREATE INDEX IF NOT EXISTS idx_worker_leases_state_status ON worker_leases(state_id, status);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_worker_leases_one_active_state ON worker_leases(state_id) WHERE status = 'active';

            PRAGMA user_version = 10;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v9 to v10: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v10",
        "SQLite schema migrated from version 9 to version 10.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "10", &now)?;
    Ok(())
}

fn migrate_v10_to_v11(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS worker_pool_controls (
                resource_class TEXT PRIMARY KEY CHECK (resource_class IN ('default', 'local_llm')),
                is_paused INTEGER NOT NULL DEFAULT 0,
                pause_reason TEXT,
                updated_at TEXT NOT NULL
            );

            PRAGMA user_version = 11;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v10 to v11: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v11",
        "SQLite schema migrated from version 10 to version 11.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "11", &now)?;
    Ok(())
}

fn migrate_v11_to_v12(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            ALTER TABLE worker_pool_controls ADD COLUMN desired_concurrency INTEGER NOT NULL DEFAULT 0;
            ALTER TABLE worker_pool_controls ADD COLUMN is_started INTEGER NOT NULL DEFAULT 0;

            PRAGMA user_version = 12;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v11 to v12: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v12",
        "SQLite schema migrated from version 11 to version 12.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "12", &now)?;
    Ok(())
}

fn migrate_v12_to_v13(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_worker_claim
                ON entity_stage_states(status, file_exists, updated_at, id);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_worker_retry_claim
                ON entity_stage_states(status, next_retry_at, file_exists, updated_at, id);

            PRAGMA user_version = 13;
            "#,
        )
        .map_err(|error| format!("Failed to migrate schema from v12 to v13: {error}"))?;

    let now = Utc::now().to_rfc3339();
    insert_app_event(
        connection,
        AppEventLevel::Info,
        "schema_migrated_to_v13",
        "SQLite schema migrated from version 12 to version 13.",
        None,
        &now,
    )?;
    set_setting(connection, "schema_version", "13", &now)?;
    Ok(())
}

fn sync_stages(connection: &mut Connection, stages: &[StageDefinition]) -> Result<(), String> {
    let gate_key = sqlite_write_gate_key_for_connection(connection)?;
    let gate = sqlite_write_gate_for_key(gate_key.clone())?;
    retry_sqlite_busy_operation(|| {
        with_sqlite_write_gate(&gate, &gate_key, "stage sync", || {
            sync_stages_once(connection, stages)
        })
    })
}

fn sync_stages_once(connection: &mut Connection, stages: &[StageDefinition]) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let transaction = immediate_transaction(connection, "stage sync")?;

    let incoming_ids = stages
        .iter()
        .map(|stage| stage.id.clone())
        .collect::<HashSet<_>>();
    let existing_ids = load_existing_stage_ids(&transaction)?;

    for stage in stages {
        let save_path_aliases_json = serde_json::to_string(&stage.save_path_aliases)
            .map_err(|error| format!("Failed to serialize save_path_aliases: {error}"))?;
        transaction
            .execute(
                r#"
                INSERT INTO stages (
                    stage_id,
                    input_folder,
                    input_uri,
                    output_folder,
                    workflow_url,
                    max_attempts,
                    retry_delay_sec,
                    next_stage,
                    save_path_aliases_json,
                    resource_class,
                    allow_empty_outputs,
                    allow_multiple_outputs,
                    is_active,
                    archived_at,
                    last_seen_in_config_at,
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, NULL, ?13, ?13, ?13)
                ON CONFLICT(stage_id) DO UPDATE SET
                    input_folder = excluded.input_folder,
                    input_uri = excluded.input_uri,
                    output_folder = excluded.output_folder,
                    workflow_url = excluded.workflow_url,
                    max_attempts = excluded.max_attempts,
                    retry_delay_sec = excluded.retry_delay_sec,
                    next_stage = excluded.next_stage,
                    save_path_aliases_json = excluded.save_path_aliases_json,
                    resource_class = excluded.resource_class,
                    allow_empty_outputs = excluded.allow_empty_outputs,
                    allow_multiple_outputs = excluded.allow_multiple_outputs,
                    is_active = 1,
                    archived_at = NULL,
                    last_seen_in_config_at = excluded.last_seen_in_config_at,
                    updated_at = excluded.updated_at
                "#,
                params![
                    stage.id,
                    stage.input_folder,
                    stage.input_uri,
                    stage.output_folder,
                    stage.workflow_url,
                    stage.max_attempts as i64,
                    stage.retry_delay_sec as i64,
                    stage.next_stage,
                    save_path_aliases_json,
                    stage.resource_class.as_str(),
                    bool_to_i64(stage.allow_empty_outputs),
                    bool_to_i64(stage.allow_multiple_outputs),
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
                stage.input_uri,
                stage.output_folder,
                stage.workflow_url,
                stage.max_attempts,
                stage.retry_delay_sec,
                stage.next_stage,
                stage.save_path_aliases_json,
                stage.resource_class,
                stage.allow_empty_outputs,
                stage.allow_multiple_outputs,
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
                stage.input_uri,
                stage.output_folder,
                stage.workflow_url,
                stage.max_attempts,
                stage.retry_delay_sec,
                stage.next_stage,
                stage.save_path_aliases_json,
                stage.resource_class,
                stage.allow_empty_outputs,
                stage.allow_multiple_outputs,
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
            let save_path_aliases_json: String = row.get(8)?;
            let save_path_aliases =
                parse_json::<Vec<String>>(&save_path_aliases_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        8,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                    )
                })?;
            let resource_class_text: String = row.get(9)?;
            let resource_class = match resource_class_text.as_str() {
                "default" => ResourceClass::Default,
                "local_llm" => ResourceClass::LocalLlm,
                _ => ResourceClass::Default,
            };
            Ok(StageRecord {
                id: row.get(0)?,
                input_folder: row.get(1)?,
                input_uri: row.get(2)?,
                output_folder: row.get(3)?,
                workflow_url: row.get(4)?,
                max_attempts: row.get::<_, i64>(5)? as u64,
                retry_delay_sec: row.get::<_, i64>(6)? as u64,
                next_stage: row.get(7)?,
                save_path_aliases,
                resource_class,
                allow_empty_outputs: row.get::<_, i64>(10)? == 1,
                allow_multiple_outputs: row.get::<_, i64>(11)? == 1,
                is_active: row.get::<_, i64>(12)? == 1,
                archived_at: row.get(13)?,
                last_seen_in_config_at: row.get(14)?,
                created_at: row.get(15)?,
                updated_at: row.get(16)?,
                entity_count: row.get::<_, i64>(17)? as u64,
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
                display_name,
                operator_note,
                is_archived,
                archived_at,
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
                artifact_id,
                relation_to_source,
                storage_provider,
                bucket,
                object_key,
                version_id,
                etag,
                checksum_sha256,
                checksum,
                file_mtime,
                file_size,
                artifact_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                producer_run_id,
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
                artifact_id,
                relation_to_source,
                storage_provider,
                bucket,
                object_key,
                version_id,
                etag,
                checksum_sha256,
                checksum,
                file_mtime,
                file_size,
                artifact_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                producer_run_id,
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
            "WHERE storage_provider = 's3' AND producer_run_id = ?1 AND artifact_id = ?2 ORDER BY id DESC LIMIT 1",
        ) => {
            r#"
            SELECT
                id,
                entity_id,
                stage_id,
                file_path,
                file_name,
                artifact_id,
                relation_to_source,
                storage_provider,
                bucket,
                object_key,
                version_id,
                etag,
                checksum_sha256,
                checksum,
                file_mtime,
                file_size,
                artifact_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                producer_run_id,
                file_exists,
                missing_since,
                first_seen_at,
                last_seen_at,
                updated_at
            FROM entity_files
            WHERE storage_provider = 's3' AND producer_run_id = ?1 AND artifact_id = ?2
            ORDER BY id DESC
            LIMIT 1
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
                artifact_id,
                relation_to_source,
                storage_provider,
                bucket,
                object_key,
                version_id,
                etag,
                checksum_sha256,
                checksum,
                file_mtime,
                file_size,
                artifact_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                producer_run_id,
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
                artifact_id,
                relation_to_source,
                storage_provider,
                bucket,
                object_key,
                version_id,
                etag,
                checksum_sha256,
                checksum,
                file_mtime,
                file_size,
                artifact_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                producer_run_id,
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
                artifact_id,
                relation_to_source,
                storage_provider,
                bucket,
                object_key,
                version_id,
                etag,
                checksum_sha256,
                checksum,
                file_mtime,
                file_size,
                artifact_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                producer_run_id,
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
                artifact_id,
                relation_to_source,
                storage_provider,
                bucket,
                object_key,
                version_id,
                etag,
                checksum_sha256,
                checksum,
                file_mtime,
                file_size,
                artifact_size,
                payload_json,
                meta_json,
                current_stage,
                next_stage,
                status,
                validation_status,
                validation_errors_json,
                is_managed_copy,
                copy_source_file_id,
                producer_run_id,
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
    if file.storage_provider == StorageProvider::S3 {
        return serialize_json_pretty(&json!({
            "entity_id": file.entity_id,
            "stage_id": file.stage_id,
            "status": file.status,
            "storage_provider": "s3",
            "artifact_id": file.artifact_id,
            "relation_to_source": file.relation_to_source,
            "bucket": file.bucket,
            "key": file.key,
            "version_id": file.version_id,
            "etag": file.etag,
            "checksum_sha256": file.checksum_sha256,
            "size": file.artifact_size,
            "producer_run_id": file.producer_run_id,
            "business_json_preview": null,
            "note": "S3 business JSON is not loaded by Beehive during execution."
        }));
    }
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
    let resource_class_text: String = row.get(12)?;
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
        resource_class: ResourceClass::from_str(&resource_class_text).unwrap_or_default(),
        lease_id: None,
        worker_id: None,
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
    let payload_json: Option<String> = row.get(9)?;
    let display_name: Option<String> = row
        .get::<_, Option<String>>(1)?
        .filter(|value| !value.trim().is_empty())
        .or_else(|| entity_display_name_from_payload(payload_json.as_deref()));
    Ok(EntityTableRow {
        entity_id: row.get(0)?,
        display_name,
        operator_note: row.get(2)?,
        is_archived: row.get::<_, i64>(3)? == 1,
        archived_at: row.get(4)?,
        current_stage_id: row.get(5)?,
        current_status: row.get(6)?,
        latest_file_path: row.get(7)?,
        latest_file_id: row.get(8)?,
        file_count: row.get::<_, i64>(10)? as u64,
        attempts: row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
        max_attempts: row.get::<_, Option<i64>>(12)?.map(|value| value as u64),
        last_error: row.get(13)?,
        last_http_status: row.get(14)?,
        next_retry_at: row.get(15)?,
        last_started_at: row.get(16)?,
        last_finished_at: row.get(17)?,
        validation_status: parse_validation_status(&row.get::<_, String>(18)?)?,
        updated_at: row.get(19)?,
        last_seen_at: row.get(20)?,
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
    let validation_status = parse_validation_status(&row.get::<_, String>(10)?)?;
    let validation_errors_json: String = row.get(11)?;
    let validation_errors = parse_json::<Vec<ConfigValidationIssue>>(&validation_errors_json)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                11,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        })?;

    Ok(EntityRecord {
        entity_id: row.get(0)?,
        display_name: row.get(1)?,
        operator_note: row.get(2)?,
        is_archived: row.get::<_, i64>(3)? == 1,
        archived_at: row.get(4)?,
        current_stage_id: row.get(5)?,
        current_status: row.get(6)?,
        latest_file_path: row.get(7)?,
        latest_file_id: row.get(8)?,
        file_count: row.get::<_, i64>(9)? as u64,
        validation_status,
        validation_errors,
        first_seen_at: row.get(12)?,
        last_seen_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn entity_file_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityFileRecord> {
    let storage_provider = parse_storage_provider(&row.get::<_, String>(7)?)?;
    let validation_status = parse_validation_status(&row.get::<_, String>(22)?)?;
    let validation_errors_json: String = row.get(23)?;
    let validation_errors = parse_json::<Vec<ConfigValidationIssue>>(&validation_errors_json)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                23,
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
        artifact_id: row.get(5)?,
        relation_to_source: row.get(6)?,
        storage_provider,
        bucket: row.get(8)?,
        key: row.get(9)?,
        version_id: row.get(10)?,
        etag: row.get(11)?,
        checksum_sha256: row.get(12)?,
        checksum: row.get(13)?,
        file_mtime: row.get(14)?,
        file_size: row.get::<_, i64>(15)? as u64,
        artifact_size: row.get::<_, Option<i64>>(16)?.map(|value| value as u64),
        payload_json: row.get(17)?,
        meta_json: row.get(18)?,
        current_stage: row.get(19)?,
        next_stage: row.get(20)?,
        status: row.get(21)?,
        validation_status,
        validation_errors,
        is_managed_copy: row.get::<_, i64>(24)? == 1,
        copy_source_file_id: row.get(25)?,
        producer_run_id: row.get(26)?,
        file_exists: row.get::<_, i64>(27)? == 1,
        missing_since: row.get(28)?,
        first_seen_at: row.get(29)?,
        last_seen_at: row.get(30)?,
        updated_at: row.get(31)?,
    })
}

fn parse_storage_provider(value: &str) -> rusqlite::Result<StorageProvider> {
    match value {
        "local" => Ok(StorageProvider::Local),
        "s3" => Ok(StorageProvider::S3),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unknown storage provider '{other}'."),
            )),
        )),
    }
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
            storage: None,
            runtime: RuntimeConfig::default(),
            stages,
        }
    }

    fn stage(id: &str, next_stage: Option<&str>) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: format!("stages/{id}"),
            input_uri: None,
            output_folder: format!("stages/{id}-out"),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: next_stage.map(ToOwned::to_owned),
            save_path_aliases: Vec::new(),
            resource_class: Default::default(),
            allow_empty_outputs: false,
            allow_multiple_outputs: false,
        }
    }

    fn stage_with_resource(
        id: &str,
        next_stage: Option<&str>,
        resource_class: ResourceClass,
    ) -> StageDefinition {
        StageDefinition {
            resource_class,
            ..stage(id, next_stage)
        }
    }

    fn setup_scanned_workdir(
        stages: Vec<StageDefinition>,
        files: &[(&str, &str)],
    ) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(&database_path, &test_config(stages)).expect("bootstrap");
        for (stage_id, entity_id) in files {
            let source_path = workdir
                .join("stages")
                .join(stage_id)
                .join(format!("{entity_id}.json"));
            fs::create_dir_all(source_path.parent().expect("source parent")).expect("parent");
            fs::write(
                &source_path,
                format!(r#"{{"id":"{entity_id}","payload":{{"stage":"{stage_id}"}}}}"#),
            )
            .expect("source");
        }
        scan_workspace(&workdir, &database_path).expect("scan");
        (tempdir, workdir, database_path)
    }

    fn start_test_pool(
        connection: &Connection,
        resource_class: ResourceClass,
        desired_concurrency: u32,
    ) {
        set_worker_pool_runtime_state_with_connection(
            connection,
            resource_class,
            true,
            desired_concurrency,
        )
        .expect("start worker pool");
    }

    fn s3_pointer_input(
        entity_id: &str,
        artifact_id: &str,
        key: &str,
        producer_run_id: &str,
    ) -> RegisterS3ArtifactPointerInput {
        RegisterS3ArtifactPointerInput {
            entity_id: entity_id.to_string(),
            artifact_id: artifact_id.to_string(),
            relation_to_source: Some("child_entity".to_string()),
            stage_id: "raw_entities".to_string(),
            bucket: "steos-s3-data".to_string(),
            key: key.to_string(),
            version_id: None,
            etag: None,
            checksum_sha256: None,
            size: Some(42),
            last_modified: None,
            source_file_id: None,
            producer_run_id: Some(producer_run_id.to_string()),
            status: StageStatus::Pending,
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
            resource_class: Default::default(),
            allow_empty_outputs: false,
            allow_multiple_outputs: false,
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
    fn bootstrap_creates_database_file_and_required_tables_at_current_version() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config(vec![stage("ingest", Some("normalize"))]);

        let result = bootstrap_database(&database_path, &config).expect("bootstrap");
        let connection = Connection::open(&database_path).expect("open db");
        let table_names = load_table_names(&connection).expect("table names");

        assert!(database_path.exists());
        assert_eq!(result.schema_version, 13);
        assert!(table_names.contains(&"entity_files".to_string()));
        assert!(table_names.contains(&"entities".to_string()));
        assert!(table_names.contains(&"entity_stage_states".to_string()));
        assert!(table_names.contains(&"worker_leases".to_string()));
        assert!(table_names.contains(&"worker_pool_controls".to_string()));
        let stages = list_stages(&database_path).expect("stages");
        assert_eq!(stages[0].resource_class, ResourceClass::Default);
    }

    #[test]
    fn existing_v2_database_is_migrated_to_current_version() {
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

        assert_eq!(result.schema_version, 13);
        assert_eq!(entity.file_count, 1);
        assert_eq!(files.len(), 1);
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v3"));
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v4"));
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v9"));
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v10"));
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v11"));
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v12"));
    }

    #[test]
    fn v1_database_can_bootstrap_through_current_version() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let connection = Connection::open(&database_path).expect("open db");
        create_v1_schema(&connection);
        drop(connection);

        let result = bootstrap_database(&database_path, &test_config(vec![stage("ingest", None)]))
            .expect("bootstrap");

        assert_eq!(result.schema_version, 13);
    }

    #[test]
    fn worker_claim_filters_by_resource_class_and_limit_zero_disables_claim() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![
                stage_with_resource("default_stage", None, ResourceClass::Default),
                stage_with_resource("llm_stage", None, ResourceClass::LocalLlm),
            ],
            &[
                ("default_stage", "entity-default"),
                ("llm_stage", "entity-llm"),
            ],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 10);
        start_test_pool(&connection, ResourceClass::LocalLlm, 10);

        let disabled = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 0,
                worker_id: "default-worker-disabled".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("disabled claim");
        assert!(disabled.is_empty());

        let default_tasks = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 10,
                worker_id: "default-worker".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("default claim");
        assert_eq!(default_tasks.len(), 1);
        assert_eq!(default_tasks[0].stage_id, "default_stage");
        assert_eq!(default_tasks[0].resource_class, ResourceClass::Default);
        assert!(default_tasks[0].lease_id.is_some());

        let llm_tasks = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::LocalLlm,
                limit: 10,
                worker_id: "llm-worker".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("llm claim");
        assert_eq!(llm_tasks.len(), 1);
        assert_eq!(llm_tasks[0].stage_id, "llm_stage");
        assert_eq!(llm_tasks[0].resource_class, ResourceClass::LocalLlm);
    }

    #[test]
    fn sqlite_connection_pragmas_include_busy_timeout_and_wal() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let connection = open_connection(&database_path).expect("open");

        let busy_timeout: i64 = connection
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .expect("busy timeout");
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal mode");

        assert_eq!(busy_timeout, 30_000);
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    }

    #[test]
    fn sqlite_busy_timeout_env_parser_defaults_and_accepts_positive_values() {
        assert_eq!(parse_sqlite_busy_timeout_ms(None), 30_000);
        assert_eq!(parse_sqlite_busy_timeout_ms(Some("")), 30_000);
        assert_eq!(parse_sqlite_busy_timeout_ms(Some("0")), 30_000);
        assert_eq!(parse_sqlite_busy_timeout_ms(Some("15000")), 15_000);
    }

    #[test]
    fn sqlite_busy_detection_matches_error_codes_and_messages() {
        let busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            Some("database is locked".to_string()),
        );
        assert!(sqlite_error_is_busy(&busy));
        assert!(sqlite_message_is_busy(
            "Failed to start transaction: database is locked"
        ));
        assert!(sqlite_message_is_busy("SQLITE_BUSY while committing"));
        assert!(!sqlite_message_is_busy("UNIQUE constraint failed"));
    }

    #[test]
    fn sqlite_busy_retry_helper_retries_then_succeeds() {
        let mut attempts = 0;
        let mut sleeps = Vec::new();
        let mut operation = || {
            attempts += 1;
            if attempts < 3 {
                Err("database is locked".to_string())
            } else {
                Ok("ok")
            }
        };

        let result = retry_sqlite_busy_operation_with_sleep(&mut operation, |delay| {
            sleeps.push(delay.as_millis())
        })
        .expect("retry result");

        assert_eq!(result, "ok");
        assert_eq!(attempts, 3);
        assert_eq!(sleeps, vec![50, 100]);
    }

    #[test]
    fn sqlite_busy_retry_helper_stops_after_retry_limit() {
        let mut attempts = 0;
        let mut sleeps = Vec::new();
        let mut operation = || {
            attempts += 1;
            Err::<(), _>("database is busy".to_string())
        };

        let result = retry_sqlite_busy_operation_with_sleep(&mut operation, |delay| {
            sleeps.push(delay.as_millis())
        });

        assert!(result.is_err());
        assert_eq!(attempts, SQLITE_WRITE_RETRY_BACKOFF_MS.len() + 1);
        assert_eq!(sleeps.len(), SQLITE_WRITE_RETRY_BACKOFF_MS.len());
    }

    #[test]
    fn worker_pool_pause_prevents_claim_and_resume_allows_claim() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![
                stage_with_resource("default_stage", None, ResourceClass::Default),
                stage_with_resource("llm_stage", None, ResourceClass::LocalLlm),
            ],
            &[
                ("default_stage", "entity-default"),
                ("llm_stage", "entity-llm"),
            ],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        start_test_pool(&connection, ResourceClass::LocalLlm, 1);

        set_worker_pool_paused_with_connection(
            &connection,
            ResourceClass::Default,
            true,
            Some("maintenance"),
        )
        .expect("pause default");
        let default_paused_claim = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "default-worker".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("paused claim");
        assert!(default_paused_claim.is_empty());

        let llm_claim = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::LocalLlm,
                limit: 1,
                worker_id: "llm-worker".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("llm claim");
        assert_eq!(llm_claim.len(), 1);

        set_worker_pool_paused_with_connection(&connection, ResourceClass::Default, false, None)
            .expect("resume default");
        let default_resumed_claim = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "default-worker".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("resumed claim");
        assert_eq!(default_resumed_claim.len(), 1);
    }

    #[test]
    fn worker_claim_requires_start_and_respects_desired_concurrency() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1"), ("incoming", "entity-2")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        let stopped = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 10,
                worker_id: "worker-stopped".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("stopped claim");
        assert!(stopped.is_empty());

        start_test_pool(&connection, ResourceClass::Default, 1);
        let first = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 10,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("first claim");
        assert_eq!(first.len(), 1);

        let capped = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 10,
                worker_id: "worker-two".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("capped claim");
        assert!(capped.is_empty());
    }

    #[test]
    fn depth_first_claims_fresh_child_before_older_source() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "source-old"), ("incoming", "child-new")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        seed_worker_ordering_fixture(&connection);
        start_test_pool(&connection, ResourceClass::Default, 1);

        let tasks = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "depth-worker".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("depth-first claim");

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].entity_id, "child-new");
    }

    #[test]
    fn fifo_claims_older_source_before_fresh_child() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "source-old"), ("incoming", "child-new")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        seed_worker_ordering_fixture(&connection);
        start_test_pool(&connection, ResourceClass::Default, 1);

        let tasks = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "fifo-worker".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::Fifo,
                now: Utc::now(),
            },
        )
        .expect("fifo claim");

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].entity_id, "source-old");
    }

    #[test]
    fn worker_claim_prefers_pending_before_due_retry_wait() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "pending-healthy"), ("incoming", "retry-due")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        let old = (Utc::now() - Duration::minutes(30)).to_rfc3339();
        let recent = Utc::now().to_rfc3339();
        connection
            .execute(
                "UPDATE entity_stage_states SET updated_at = ?2 WHERE entity_id = ?1",
                params!["pending-healthy", old],
            )
            .expect("older pending");
        connection
            .execute(
                r#"
                UPDATE entity_stage_states
                SET status = 'retry_wait',
                    attempts = 1,
                    next_retry_at = ?2,
                    updated_at = ?3
                WHERE entity_id = ?1
                "#,
                params!["retry-due", old, recent],
            )
            .expect("retry due");
        connection
            .execute(
                "UPDATE entity_files SET producer_run_id = 'run-retry' WHERE entity_id = ?1",
                params!["retry-due"],
            )
            .expect("mark retry as child");
        start_test_pool(&connection, ResourceClass::Default, 1);

        let tasks = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "retry-order-worker".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("retry demotion claim");

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].entity_id, "pending-healthy");
    }

    fn seed_worker_ordering_fixture(connection: &Connection) {
        let old = (Utc::now() - Duration::minutes(30)).to_rfc3339();
        let recent = Utc::now().to_rfc3339();
        connection
            .execute(
                "UPDATE entity_stage_states SET updated_at = ?2 WHERE entity_id = ?1",
                params!["source-old", old],
            )
            .expect("older source");
        connection
            .execute(
                "UPDATE entity_stage_states SET updated_at = ?2 WHERE entity_id = ?1",
                params!["child-new", recent],
            )
            .expect("newer child");
        connection
            .execute(
                "UPDATE entity_files SET producer_run_id = 'run-child' WHERE entity_id = ?1",
                params!["child-new"],
            )
            .expect("mark child");
    }

    #[test]
    fn worker_summary_includes_queue_counts_and_pause_state_by_resource_class() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![
                stage_with_resource("default_stage", None, ResourceClass::Default),
                stage_with_resource("llm_stage", None, ResourceClass::LocalLlm),
            ],
            &[
                ("default_stage", "entity-default"),
                ("llm_stage", "entity-llm"),
            ],
        );
        let connection = open_connection(&database_path).expect("open");
        set_worker_pool_paused_with_connection(
            &connection,
            ResourceClass::LocalLlm,
            true,
            Some("gpu maintenance"),
        )
        .expect("pause llm");
        drop(connection);

        let summary = get_worker_summary(
            &database_path,
            &test_config(vec![
                stage_with_resource("default_stage", None, ResourceClass::Default),
                stage_with_resource("llm_stage", None, ResourceClass::LocalLlm),
            ]),
        )
        .expect("summary");
        let default_pool = summary
            .pools
            .iter()
            .find(|pool| pool.resource_class == ResourceClass::Default)
            .expect("default pool");
        let llm_pool = summary
            .pools
            .iter()
            .find(|pool| pool.resource_class == ResourceClass::LocalLlm)
            .expect("llm pool");

        assert_eq!(default_pool.pending_count, 1);
        assert_eq!(default_pool.active_leases, 0);
        assert!(!default_pool.is_paused);
        assert_eq!(llm_pool.pending_count, 1);
        assert!(llm_pool.is_paused);
        assert_eq!(llm_pool.pause_reason.as_deref(), Some("gpu maintenance"));
    }

    #[test]
    fn worker_double_claim_and_manual_claim_refuse_active_lease() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let first = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("first claim");
        assert_eq!(first.len(), 1);

        let second = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-two".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("second claim");
        assert!(second.is_empty());
        assert_eq!(
            query_count(
                &connection,
                "SELECT COUNT(*) FROM worker_leases WHERE state_id = ?1 AND status = 'active'",
                params![first[0].state_id],
            )
            .expect("active count"),
            1,
        );

        let manual = claim_specific_runtime_task(
            &mut connection,
            "entity-1",
            "incoming",
            &Utc::now().to_rfc3339(),
        );
        assert!(manual.is_err());
    }

    #[test]
    fn worker_heartbeat_extends_active_lease_and_rejects_wrong_worker() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 10,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        let lease_id = task.lease_id.expect("lease id");
        let before_until: String = connection
            .query_row(
                "SELECT lease_until FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .expect("before");
        let heartbeat_at = Utc::now() + Duration::seconds(60);
        heartbeat_worker_lease_with_connection(
            &connection,
            &lease_id,
            "worker-one",
            1800,
            &heartbeat_at,
        )
        .expect("heartbeat");
        let (after_heartbeat, after_until): (String, String) = connection
            .query_row(
                "SELECT heartbeat_at, lease_until FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("after");

        assert_eq!(after_heartbeat, heartbeat_at.to_rfc3339());
        assert!(after_until > before_until);
        let wrong_worker = heartbeat_worker_lease_with_connection(
            &connection,
            &lease_id,
            "worker-two",
            1800,
            &Utc::now(),
        );
        assert!(wrong_worker.is_err());
    }

    #[test]
    fn worker_recovery_expires_queued_lease_and_restores_pending_state() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        let lease_id = task.lease_id.expect("lease id");
        let not_recovered =
            recover_expired_worker_leases_with_connection(&connection, &Utc::now().to_rfc3339())
                .expect("recover current");
        assert_eq!(not_recovered, 0);

        let old = (Utc::now() - Duration::seconds(60)).to_rfc3339();
        connection
            .execute(
                "UPDATE worker_leases SET lease_until = ?2 WHERE lease_id = ?1",
                params![lease_id, old],
            )
            .expect("expire lease");
        let recovered =
            recover_expired_worker_leases_with_connection(&connection, &Utc::now().to_rfc3339())
                .expect("recover expired");
        let state = get_stage_state(&database_path, "entity-1", "incoming")
            .expect("state")
            .expect("state exists");
        let lease_status: String = connection
            .query_row(
                "SELECT status FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .expect("lease status");

        assert_eq!(recovered, 1);
        assert_eq!(lease_status, "expired");
        assert_eq!(state.status, "pending");
    }

    #[test]
    fn worker_recovery_skips_expired_lease_with_finished_run() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        let lease_id = task.lease_id.expect("lease id");
        let run_id = "finished-run";
        let now = Utc::now().to_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO stage_runs (
                    run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url,
                    request_json, response_json, http_status, success, started_at, finished_at,
                    duration_ms
                )
                VALUES (?1, 'entity-1', ?2, 'incoming', 1, 'http://localhost', '{}', '{}', 200, 1, ?3, ?3, 1)
                "#,
                params![run_id, task.file_instance_id, now],
            )
            .expect("stage run");
        connection
            .execute(
                "UPDATE worker_leases SET run_id = ?2, lease_until = ?3 WHERE lease_id = ?1",
                params![
                    lease_id,
                    run_id,
                    (Utc::now() - Duration::seconds(60)).to_rfc3339()
                ],
            )
            .expect("attach finished run");

        let recovered =
            recover_expired_worker_leases_with_connection(&connection, &Utc::now().to_rfc3339())
                .expect("recover");
        let lease_status: String = connection
            .query_row(
                "SELECT status FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .expect("lease status");

        assert_eq!(recovered, 0);
        assert_eq!(lease_status, "active");
    }

    #[test]
    fn reconcile_finished_successful_run_marks_state_done_and_lease_done() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        let lease_id = task.lease_id.clone().expect("lease id");
        let now = Utc::now().to_rfc3339();
        start_claimed_stage_run(
            &mut connection,
            task.state_id,
            &NewStageRunInput {
                run_id: "finished-success".to_string(),
                entity_id: task.entity_id.clone(),
                entity_file_id: task.file_instance_id,
                stage_id: task.stage_id.clone(),
                attempt_no: 1,
                workflow_url: task.workflow_url.clone(),
                request_json: "{}".to_string(),
                started_at: now.clone(),
            },
        )
        .expect("start run");
        attach_worker_lease_run(&connection, &lease_id, "finished-success", &now)
            .expect("attach run");
        finish_stage_run(
            &connection,
            &FinishStageRunInput {
                run_id: "finished-success".to_string(),
                response_json: Some("{}".to_string()),
                http_status: Some(200),
                success: true,
                error_type: None,
                error_message: None,
                finished_at: now.clone(),
                duration_ms: 1,
            },
        )
        .expect("finish run");

        let before = get_worker_summary(
            &database_path,
            &test_config(vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )]),
        )
        .expect("summary");
        assert!(before
            .worker_state_anomaly_counts
            .iter()
            .any(|item| item.diagnosis == "active_lease_with_finished_run" && item.count == 1));

        let reconciled = reconcile_stuck_worker_states(&database_path, 1800).expect("reconcile");
        let state = get_stage_state(&database_path, "entity-1", "incoming")
            .expect("state")
            .expect("state exists");
        let lease_status: String = connection
            .query_row(
                "SELECT status FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .expect("lease status");

        assert_eq!(reconciled, 1);
        assert_eq!(state.status, "done");
        assert_eq!(lease_status, "done");
    }

    #[test]
    fn reconcile_finished_failed_run_schedules_retry_and_fails_lease() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        let lease_id = task.lease_id.clone().expect("lease id");
        let now = Utc::now().to_rfc3339();
        start_claimed_stage_run(
            &mut connection,
            task.state_id,
            &NewStageRunInput {
                run_id: "finished-failed".to_string(),
                entity_id: task.entity_id.clone(),
                entity_file_id: task.file_instance_id,
                stage_id: task.stage_id.clone(),
                attempt_no: 1,
                workflow_url: task.workflow_url.clone(),
                request_json: "{}".to_string(),
                started_at: now.clone(),
            },
        )
        .expect("start run");
        attach_worker_lease_run(&connection, &lease_id, "finished-failed", &now)
            .expect("attach run");
        finish_stage_run(
            &connection,
            &FinishStageRunInput {
                run_id: "finished-failed".to_string(),
                response_json: None,
                http_status: Some(500),
                success: false,
                error_type: Some("http_error".to_string()),
                error_message: Some("HTTP 500".to_string()),
                finished_at: now.clone(),
                duration_ms: 1,
            },
        )
        .expect("finish run");

        let reconciled = reconcile_stuck_worker_states(&database_path, 1800).expect("reconcile");
        let state = get_stage_state(&database_path, "entity-1", "incoming")
            .expect("state")
            .expect("state exists");
        let lease_status: String = connection
            .query_row(
                "SELECT status FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .expect("lease status");

        assert_eq!(reconciled, 1);
        assert_eq!(state.status, "retry_wait");
        assert_eq!(lease_status, "failed");
    }

    #[test]
    fn reconcile_unleased_in_progress_and_queued_states_is_idempotent() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[
                ("incoming", "entity-progress"),
                ("incoming", "entity-queued"),
            ],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 2);
        let mut tasks = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 2,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::Fifo,
                now: Utc::now(),
            },
        )
        .expect("claim");
        tasks.sort_by(|left, right| left.entity_id.cmp(&right.entity_id));
        let progress = tasks
            .iter()
            .find(|task| task.entity_id == "entity-progress")
            .expect("progress task")
            .clone();
        let queued = tasks
            .iter()
            .find(|task| task.entity_id == "entity-queued")
            .expect("queued task")
            .clone();
        let old = (Utc::now() - Duration::seconds(7200)).to_rfc3339();
        start_claimed_stage_run(
            &mut connection,
            progress.state_id,
            &NewStageRunInput {
                run_id: "old-unleased-run".to_string(),
                entity_id: progress.entity_id.clone(),
                entity_file_id: progress.file_instance_id,
                stage_id: progress.stage_id.clone(),
                attempt_no: 1,
                workflow_url: progress.workflow_url.clone(),
                request_json: "{}".to_string(),
                started_at: old,
            },
        )
        .expect("start old run");
        attach_worker_lease_run(
            &connection,
            progress.lease_id.as_deref().expect("progress lease"),
            "old-unleased-run",
            &Utc::now().to_rfc3339(),
        )
        .expect("attach run");
        connection
            .execute(
                "UPDATE worker_leases SET status = 'released', released_at = ?2, release_reason = 'test', updated_at = ?2 WHERE lease_id = ?1",
                params![progress.lease_id.as_deref().expect("progress lease"), Utc::now().to_rfc3339()],
            )
            .expect("release progress lease");
        connection
            .execute(
                "UPDATE worker_leases SET status = 'released', released_at = ?2, release_reason = 'test', updated_at = ?2 WHERE lease_id = ?1",
                params![queued.lease_id.as_deref().expect("queued lease"), Utc::now().to_rfc3339()],
            )
            .expect("release queued lease");

        let before = get_worker_summary(
            &database_path,
            &test_config(vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )]),
        )
        .expect("summary");
        assert!(before
            .worker_state_anomaly_counts
            .iter()
            .any(|item| item.diagnosis == "in_progress_without_active_lease" && item.count == 1));
        assert!(before
            .worker_state_anomaly_counts
            .iter()
            .any(|item| item.diagnosis == "queued_without_active_lease" && item.count == 1));

        let reconciled = reconcile_stuck_worker_states(&database_path, 1).expect("reconcile");
        let second = reconcile_stuck_worker_states(&database_path, 1).expect("reconcile again");
        let progress_state = get_stage_state(&database_path, "entity-progress", "incoming")
            .expect("progress state")
            .expect("progress exists");
        let queued_state = get_stage_state(&database_path, "entity-queued", "incoming")
            .expect("queued state")
            .expect("queued exists");
        let run_error_type: String = connection
            .query_row(
                "SELECT error_type FROM stage_runs WHERE run_id = 'old-unleased-run'",
                [],
                |row| row.get(0),
            )
            .expect("run error type");

        assert_eq!(reconciled, 2);
        assert_eq!(second, 0);
        assert_eq!(progress_state.status, "retry_wait");
        assert_eq!(queued_state.status, "pending");
        assert_eq!(
            run_error_type,
            "in_progress_without_active_lease_reconciled"
        );
    }

    #[test]
    fn worker_internal_error_finalizer_clears_active_lease_and_in_progress() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        let lease_id = task.lease_id.clone().expect("lease id");
        let now = Utc::now().to_rfc3339();
        start_claimed_stage_run(
            &mut connection,
            task.state_id,
            &NewStageRunInput {
                run_id: "internal-error-run".to_string(),
                entity_id: task.entity_id.clone(),
                entity_file_id: task.file_instance_id,
                stage_id: task.stage_id.clone(),
                attempt_no: 1,
                workflow_url: task.workflow_url.clone(),
                request_json: "{}".to_string(),
                started_at: now.clone(),
            },
        )
        .expect("start run");
        attach_worker_lease_run(&connection, &lease_id, "internal-error-run", &now)
            .expect("attach run");

        let mut task_without_lease_id = task.clone();
        task_without_lease_id.lease_id = None;
        finish_worker_task_internal_error(
            &database_path,
            &task_without_lease_id,
            "synthetic internal error",
        )
        .expect("finalize");
        let state = get_stage_state(&database_path, "entity-1", "incoming")
            .expect("state")
            .expect("state exists");
        let lease_status: String = connection
            .query_row(
                "SELECT status FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .expect("lease status");
        let run_error_type: String = connection
            .query_row(
                "SELECT error_type FROM stage_runs WHERE run_id = 'internal-error-run'",
                [],
                |row| row.get(0),
            )
            .expect("run error type");

        assert_eq!(state.status, "retry_wait");
        assert_eq!(lease_status, "failed");
        assert_eq!(run_error_type, "worker_internal_error");
    }

    #[test]
    fn release_finished_active_worker_lease_succeeds() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        let lease_id = task.lease_id.expect("lease id");
        let run_id = "finished-run-release";
        let now = Utc::now().to_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO stage_runs (
                    run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url,
                    request_json, response_json, http_status, success, started_at, finished_at,
                    duration_ms
                )
                VALUES (?1, 'entity-1', ?2, 'incoming', 1, 'http://localhost', '{}', '{}', 200, 1, ?3, ?3, 1)
                "#,
                params![run_id, task.file_instance_id, now],
            )
            .expect("stage run");
        connection
            .execute(
                "UPDATE worker_leases SET run_id = ?2 WHERE lease_id = ?1",
                params![&lease_id, run_id],
            )
            .expect("attach run");

        let released = release_worker_lease_with_connection(
            &connection,
            &lease_id,
            "manual_release_after_finished_run",
            &Utc::now().to_rfc3339(),
        )
        .expect("release");
        let lease_status: String = connection
            .query_row(
                "SELECT status FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .expect("lease status");

        assert!(released);
        assert_eq!(lease_status, "released");
    }

    #[test]
    fn release_fresh_active_unfinished_worker_lease_rejects() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage_with_resource(
                "incoming",
                None,
                ResourceClass::Default,
            )],
            &[("incoming", "entity-1")],
        );
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                resource_class: ResourceClass::Default,
                limit: 1,
                worker_id: "worker-one".to_string(),
                lease_ttl_sec: 1800,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        let lease_id = task.lease_id.expect("lease id");

        let result = release_worker_lease_with_connection(
            &connection,
            &lease_id,
            "unsafe release",
            &Utc::now().to_rfc3339(),
        );
        let lease_status: String = connection
            .query_row(
                "SELECT status FROM worker_leases WHERE lease_id = ?1",
                params![lease_id],
                |row| row.get(0),
            )
            .expect("lease status");

        assert!(result.is_err());
        assert_eq!(lease_status, "active");
    }

    #[test]
    fn s3_artifact_registration_preserves_entity_identity_and_replays_idempotently() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("raw", Some("raw_entities")),
                stage("raw_entities", None),
            ]),
        )
        .expect("bootstrap");
        let batch = vec![
            s3_pointer_input(
                "entity-alpha",
                "art-alpha",
                "main_dir/processed/raw_entities/art-alpha.json",
                "run-1",
            ),
            s3_pointer_input(
                "entity-beta",
                "art-beta",
                "main_dir/processed/raw_entities/art-beta.json",
                "run-1",
            ),
        ];

        let first = register_s3_artifact_pointers(&database_path, &batch).expect("first");
        let second = register_s3_artifact_pointers(&database_path, &batch).expect("replay");
        let files = list_entity_files(&database_path, None).expect("files");

        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 2);
        assert_eq!(first[0].entity_id, "entity-alpha");
        assert_eq!(first[0].artifact_id.as_deref(), Some("art-alpha"));
        assert_eq!(
            files
                .iter()
                .filter(|file| file.stage_id == "raw_entities")
                .count(),
            2
        );

        let conflict = vec![s3_pointer_input(
            "entity-alpha",
            "art-alpha",
            "main_dir/processed/raw_entities/art-alpha-conflict.json",
            "run-1",
        )];
        let error = register_s3_artifact_pointers(&database_path, &conflict).expect_err("conflict");
        assert!(error.contains("different bucket/key"));
    }

    #[test]
    fn s3_artifact_registration_validates_full_batch_before_mutation() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("raw", Some("raw_entities")),
                stage("raw_entities", None),
            ]),
        )
        .expect("bootstrap");
        let batch = vec![
            s3_pointer_input(
                "entity-alpha",
                "art-alpha",
                "main_dir/processed/raw_entities/art-alpha.json",
                "run-1",
            ),
            s3_pointer_input(
                "entity-beta",
                "art-alpha",
                "main_dir/processed/raw_entities/art-beta.json",
                "run-1",
            ),
        ];

        let error = register_s3_artifact_pointers(&database_path, &batch).expect_err("duplicate");
        let files = list_entity_files(&database_path, None).expect("files");

        assert!(error.contains("appears more than once"));
        assert!(files.is_empty());
    }

    #[test]
    fn workspace_explorer_exposes_s3_pointer_metadata_without_local_actions() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![s3_stage(
                "raw_entities",
                "s3://steos-s3-data/main_dir/processed/raw_entities",
            )]),
        )
        .expect("bootstrap");
        register_s3_artifact_pointers(
            &database_path,
            &[s3_pointer_input(
                "entity-alpha",
                "art-alpha",
                "main_dir/processed/raw_entities/art-alpha.json",
                "run-1",
            )],
        )
        .expect("register");

        let explorer = get_workspace_explorer(&workdir, &database_path).expect("explorer");
        let stage = explorer
            .stages
            .iter()
            .find(|stage| stage.stage_id == "raw_entities")
            .expect("stage");
        let file = stage.files.first().expect("file");

        assert_eq!(stage.storage_provider, StorageProvider::S3);
        assert_eq!(
            stage.input_uri.as_deref(),
            Some("s3://steos-s3-data/main_dir/processed/raw_entities")
        );
        assert!(stage.folder_exists);
        assert_eq!(file.storage_provider, StorageProvider::S3);
        assert_eq!(file.artifact_id.as_deref(), Some("art-alpha"));
        assert_eq!(file.producer_run_id.as_deref(), Some("run-1"));
        assert_eq!(file.bucket.as_deref(), Some("steos-s3-data"));
        assert_eq!(
            file.key.as_deref(),
            Some("main_dir/processed/raw_entities/art-alpha.json")
        );
        assert!(!file.can_open_file);
        assert!(!file.can_open_folder);
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
        fs::write(
            incoming.join("entity-cyrillic.json"),
            r#"{"id":"symptom_Кольца_Кайзера-Флейшера_e74b3ffa92f0","payload":{"value":3},"status":"pending"}"#,
        )
        .expect("entity cyrillic");
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
        let cyrillic = list_entity_table_page(
            &database_path,
            &EntityListQuery {
                search: Some("Кольца".to_string()),
                page: Some(1),
                page_size: Some(10),
                ..EntityListQuery::default()
            },
        )
        .expect("cyrillic search");

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
        assert_eq!(paged.total, 3);
        assert_eq!(paged.page, 2);
        assert_eq!(paged.entities.len(), 1);
        assert_eq!(paged.entities[0].entity_id, "entity-b");
        assert_eq!(cyrillic.total, 1);
        assert_eq!(
            cyrillic.entities[0].entity_id,
            "symptom_Кольца_Кайзера-Флейшера_e74b3ffa92f0"
        );
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
    fn bulk_reset_failed_blocked_entity_stages_to_pending_updates_counts_and_states() {
        let (_tempdir, _workdir, database_path) = setup_scanned_workdir(
            vec![stage("incoming", None)],
            &[
                ("incoming", "failed-entity"),
                ("incoming", "blocked-entity"),
            ],
        );
        let connection = open_connection(&database_path).expect("open");
        connection
            .execute(
                r#"
                UPDATE entity_stage_states
                SET status = 'failed', attempts = 2, last_error = 'bad', last_http_status = 500
                WHERE entity_id = 'failed-entity'
                "#,
                [],
            )
            .expect("failed state");
        connection
            .execute(
                r#"
                UPDATE entity_stage_states
                SET status = 'blocked', attempts = 1, last_error = 'blocked', last_http_status = 400
                WHERE entity_id = 'blocked-entity'
                "#,
                [],
            )
            .expect("blocked state");
        drop(connection);

        let result = reset_failed_blocked_entity_stages_to_pending(
            &database_path,
            Some("retry all failed and blocked"),
        )
        .expect("bulk reset");
        let failed_state = get_stage_state(&database_path, "failed-entity", "incoming")
            .expect("failed lookup")
            .expect("failed state exists");
        let blocked_state = get_stage_state(&database_path, "blocked-entity", "incoming")
            .expect("blocked lookup")
            .expect("blocked state exists");
        let events = list_app_events(&database_path, 10).expect("events");

        assert_eq!(result.reset_count, 2);
        assert_eq!(result.failed_before, 1);
        assert_eq!(result.blocked_before, 1);
        assert_eq!(result.skipped_active_lease_count, 0);
        assert_eq!(failed_state.status, "pending");
        assert_eq!(failed_state.attempts, 0);
        assert!(failed_state.last_error.is_none());
        assert_eq!(blocked_state.status, "pending");
        assert_eq!(blocked_state.attempts, 0);
        assert!(blocked_state.last_error.is_none());
        assert!(events
            .iter()
            .any(|event| event.code == "entity_stage_states_bulk_reset"));
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
        let file_id: i64 = connection
            .query_row(
                "SELECT id FROM entity_files WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                [],
                |row| row.get(0),
            )
            .expect("file id");
        let now = Utc::now().to_rfc3339();
        connection
            .execute(
                r#"
                INSERT INTO stage_runs (
                    run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url,
                    request_json, response_json, http_status, success, error_type, error_message,
                    started_at, finished_at, duration_ms
                )
                VALUES ('manual-reset-history', 'entity-1', ?1, 'incoming', 2, 'http://localhost/test',
                    '{}', '{}', 500, 0, 'n8n_error', 'bad', ?2, ?2, 10)
                "#,
                params![file_id, &now],
            )
            .expect("stage run");
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
        assert_eq!(
            list_stage_runs(&database_path, Some("entity-1"))
                .expect("runs")
                .len(),
            1
        );

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
            .any(|event| event.code == "entity_stage_state_manual_reset"));
        assert!(events.iter().any(|event| event.code == "manual_skip"));
    }

    #[test]
    fn manual_reset_allows_only_failed_blocked_and_retry_wait() {
        for status in ["failed", "blocked", "retry_wait"] {
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
                    SET status = ?1,
                        attempts = 3,
                        last_error = 'bad',
                        last_http_status = 500,
                        next_retry_at = ?2
                    WHERE entity_id = 'entity-1' AND stage_id = 'incoming'
                    "#,
                    params![status, Utc::now().to_rfc3339()],
                )
                .expect("set state");
            drop(connection);

            reset_entity_stage_to_pending(&database_path, "entity-1", "incoming", Some(status))
                .expect("reset");
            let state = get_stage_state(&database_path, "entity-1", "incoming")
                .expect("state")
                .expect("state exists");

            assert_eq!(state.status, "pending");
            assert_eq!(state.attempts, 0);
            assert!(state.last_error.is_none());
            assert!(state.next_retry_at.is_none());
        }
    }

    #[test]
    fn manual_reset_rejects_queued_and_skipped_states() {
        for status in ["queued", "skipped"] {
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
                    "UPDATE entity_stage_states SET status = ?1 WHERE entity_id = 'entity-1'",
                    params![status],
                )
                .expect("set state");
            drop(connection);

            let error = reset_entity_stage_to_pending(&database_path, "entity-1", "incoming", None)
                .expect_err("reset rejected");

            if status == "queued" {
                assert!(error.contains("state_queued_cannot_reset"));
            } else {
                assert!(error.contains("state_not_resettable"));
            }
        }
    }

    #[test]
    fn manual_reset_rejects_active_worker_lease() {
        let (_tempdir, _workdir, database_path) =
            setup_scanned_workdir(vec![stage("incoming", None)], &[("incoming", "entity-1")]);
        let mut connection = open_connection(&database_path).expect("open");
        start_test_pool(&connection, ResourceClass::Default, 1);
        let task = claim_worker_runtime_tasks(
            &mut connection,
            &WorkerLeaseClaimInput {
                worker_id: "worker-1".to_string(),
                resource_class: ResourceClass::Default,
                limit: 1,
                lease_ttl_sec: 300,
                scheduling_policy: SchedulingPolicy::DepthFirst,
                now: Utc::now(),
            },
        )
        .expect("claim")
        .pop()
        .expect("task");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'in_progress' WHERE id = ?1",
                params![task.state_id],
            )
            .expect("in progress");
        drop(connection);

        let error = reset_entity_stage_to_pending(&database_path, "entity-1", "incoming", None)
            .expect_err("reset rejected");

        assert!(error.contains("active_worker_lease_exists"));
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

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};

use crate::domain::{
    AppEventLevel, AppEventRecord, ConfigValidationIssue, DatabaseState, EntityDetailPayload,
    EntityFilters, EntityRecord, EntityStageStateRecord, EntityValidationStatus,
    InvalidDiscoveryRecord, PipelineConfig, RuntimeSummary, StageDefinition, StageRecord,
    StageStatus, StatusCount, WorkspaceExplorerResult, WorkspaceFileRecord, WorkspaceStageGroup,
};
use crate::workdir::path_string;

const SCHEMA_VERSION: u32 = 2;

pub(crate) struct PersistEntityInput {
    pub entity_id: String,
    pub file_path: String,
    pub file_name: String,
    pub stage_id: String,
    pub current_stage: Option<String>,
    pub next_stage: Option<String>,
    pub status: StageStatus,
    pub checksum: String,
    pub file_mtime: String,
    pub file_size: u64,
    pub payload_json: String,
    pub meta_json: String,
    pub validation_status: EntityValidationStatus,
    pub validation_errors: Vec<ConfigValidationIssue>,
    pub discovered_at: String,
    pub updated_at: String,
}

pub(crate) struct PersistEntityStageStateInput {
    pub entity_id: String,
    pub stage_id: String,
    pub file_path: String,
    pub status: StageStatus,
    pub max_attempts: u64,
    pub discovered_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityWriteOutcome {
    Inserted,
    Updated,
    Unchanged,
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
    let total_registered_entities = query_count(&connection, "SELECT COUNT(*) FROM entities", [])?;
    let latest_discovery_at = load_setting(&connection, "last_scan_completed_at")?;
    let discovery_error_count = load_setting(&connection, "last_scan_error_count")?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let entities_by_status = load_status_counts(&connection)?;

    Ok(RuntimeSummary {
        schema_version,
        active_stage_count,
        inactive_stage_count,
        total_registered_entities,
        entities_by_status,
        latest_discovery_at,
        discovery_error_count,
    })
}

pub fn list_stages(path: &Path) -> Result<Vec<StageRecord>, String> {
    let connection = open_connection(path)?;
    load_stage_records_from_connection(&connection)
}

pub fn list_entities(path: &Path, filters: &EntityFilters) -> Result<Vec<EntityRecord>, String> {
    let connection = open_connection(path)?;
    let mut entities = load_entities_from_connection(&connection)?;

    if let Some(stage_id) = filters.stage_id.as_ref().filter(|value| !value.is_empty()) {
        entities.retain(|entity| entity.stage_id == *stage_id);
    }
    if let Some(status) = filters.status.as_ref().filter(|value| !value.is_empty()) {
        entities.retain(|entity| entity.status == *status);
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
                    || entity.file_name.to_lowercase().contains(&search)
            });
        }
    }

    Ok(entities)
}

pub fn get_entity_detail(
    path: &Path,
    entity_id: &str,
) -> Result<Option<EntityDetailPayload>, String> {
    let connection = open_connection(path)?;
    let Some(entity) = find_entity_by_id(&connection, entity_id)? else {
        return Ok(None);
    };
    let stage_states = load_stage_states_for_entity(&connection, entity_id)?;
    let json_preview = build_json_preview(&entity)?;

    Ok(Some(EntityDetailPayload {
        entity,
        stage_states,
        json_preview,
    }))
}

pub fn list_app_events(path: &Path, limit: u32) -> Result<Vec<AppEventRecord>, String> {
    let connection = open_connection(path)?;
    load_app_events_from_connection(&connection, limit)
}

pub fn get_workspace_explorer(path: &Path) -> Result<WorkspaceExplorerResult, String> {
    let connection = open_connection(path)?;
    let stages = load_stage_records_from_connection(&connection)?;
    let mut files_by_stage: HashMap<String, Vec<WorkspaceFileRecord>> = HashMap::new();
    for entity in load_entities_from_connection(&connection)? {
        files_by_stage
            .entry(entity.stage_id.clone())
            .or_default()
            .push(WorkspaceFileRecord {
                entity_id: entity.entity_id,
                file_name: entity.file_name,
                file_path: entity.file_path,
                status: entity.status,
                validation_status: entity.validation_status,
                updated_at: entity.updated_at,
            });
    }

    let last_scan_id = load_setting(&connection, "last_scan_id")?;
    let mut invalid_by_stage: HashMap<String, Vec<InvalidDiscoveryRecord>> = HashMap::new();
    if let Some(scan_id) = last_scan_id {
        for event in load_app_events_from_connection(&connection, 250)? {
            if !matches!(
                event.code.as_str(),
                "invalid_json_file"
                    | "missing_entity_id"
                    | "missing_payload"
                    | "duplicate_entity_id"
                    | "entity_id_changed_for_path"
                    | "file_metadata_unavailable"
                    | "file_read_failed"
            ) {
                continue;
            }

            let Some(context) = event.context.as_ref() else {
                continue;
            };
            let Some(event_scan_id) = context.get("scan_id").and_then(Value::as_str) else {
                continue;
            };
            if event_scan_id != scan_id {
                continue;
            }

            let Some(stage_id) = context
                .get("stage_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
            else {
                continue;
            };

            invalid_by_stage
                .entry(stage_id.clone())
                .or_default()
                .push(InvalidDiscoveryRecord {
                    stage_id: Some(stage_id),
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
                    code: event.code,
                    message: event.message,
                    created_at: event.created_at,
                });
        }
    }

    let groups = stages
        .into_iter()
        .map(|stage| WorkspaceStageGroup {
            invalid_files: invalid_by_stage.remove(&stage.id).unwrap_or_default(),
            files: files_by_stage.remove(&stage.id).unwrap_or_default(),
            stage,
        })
        .collect();

    Ok(WorkspaceExplorerResult {
        groups,
        errors: Vec::new(),
    })
}

pub(crate) fn load_active_stages_from_connection(
    connection: &Connection,
) -> Result<Vec<StageRecord>, String> {
    let stages = load_stage_records_from_connection(connection)?;
    Ok(stages.into_iter().filter(|stage| stage.is_active).collect())
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
            FROM entities
            WHERE entity_id = ?1
            "#,
            params![entity_id],
            entity_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load entity '{entity_id}': {error}"))
}

pub(crate) fn find_entity_by_file_path(
    connection: &Connection,
    file_path: &str,
) -> Result<Option<EntityRecord>, String> {
    connection
        .query_row(
            r#"
            SELECT
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
            FROM entities
            WHERE file_path = ?1
            "#,
            params![file_path],
            entity_from_row,
        )
        .optional()
        .map_err(|error| format!("Failed to load entity for path '{file_path}': {error}"))
}

pub(crate) fn upsert_entity(
    transaction: &Transaction<'_>,
    entity: &PersistEntityInput,
) -> Result<EntityWriteOutcome, String> {
    let existing = find_entity_by_id(transaction, &entity.entity_id)?;

    let serialized_errors = serialize_json(&entity.validation_errors)?;
    let status = stage_status_value(&entity.status);
    let validation_status = validation_status_value(&entity.validation_status);

    match existing {
        Some(existing)
            if existing.file_path == entity.file_path
                && existing.checksum == entity.checksum
                && existing.file_mtime == entity.file_mtime
                && existing.file_size == entity.file_size
                && existing.current_stage == entity.current_stage
                && existing.next_stage == entity.next_stage
                && existing.payload_json == entity.payload_json
                && existing.meta_json == entity.meta_json
                && existing.status == status
                && existing.validation_status == entity.validation_status
                && existing.validation_errors == entity.validation_errors =>
        {
            Ok(EntityWriteOutcome::Unchanged)
        }
        Some(existing) => {
            transaction
                .execute(
                    r#"
                    UPDATE entities
                    SET
                        file_path = ?2,
                        file_name = ?3,
                        stage_id = ?4,
                        current_stage = ?5,
                        next_stage = ?6,
                        status = ?7,
                        checksum = ?8,
                        file_mtime = ?9,
                        file_size = ?10,
                        payload_json = ?11,
                        meta_json = ?12,
                        validation_status = ?13,
                        validation_errors_json = ?14,
                        updated_at = ?15
                    WHERE entity_id = ?1
                    "#,
                    params![
                        entity.entity_id,
                        entity.file_path,
                        entity.file_name,
                        entity.stage_id,
                        entity.current_stage,
                        entity.next_stage,
                        status,
                        entity.checksum,
                        entity.file_mtime,
                        entity.file_size as i64,
                        entity.payload_json,
                        entity.meta_json,
                        validation_status,
                        serialized_errors,
                        entity.updated_at,
                    ],
                )
                .map_err(|error| {
                    format!("Failed to update entity '{}': {error}", entity.entity_id)
                })?;

            if existing.discovered_at != entity.discovered_at {
                transaction
                    .execute(
                        "UPDATE entities SET discovered_at = ?2 WHERE entity_id = ?1 AND discovered_at = ''",
                        params![entity.entity_id, entity.discovered_at],
                    )
                    .map_err(|error| {
                        format!(
                            "Failed to reconcile discovery timestamp for entity '{}': {error}",
                            entity.entity_id
                        )
                    })?;
            }

            Ok(EntityWriteOutcome::Updated)
        }
        None => {
            transaction
                .execute(
                    r#"
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
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                    "#,
                    params![
                        entity.entity_id,
                        entity.file_path,
                        entity.file_name,
                        entity.stage_id,
                        entity.current_stage,
                        entity.next_stage,
                        status,
                        entity.checksum,
                        entity.file_mtime,
                        entity.file_size as i64,
                        entity.payload_json,
                        entity.meta_json,
                        validation_status,
                        serialized_errors,
                        entity.discovered_at,
                        entity.updated_at,
                    ],
                )
                .map_err(|error| {
                    format!("Failed to insert entity '{}': {error}", entity.entity_id)
                })?;

            Ok(EntityWriteOutcome::Inserted)
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
            VALUES (?1, ?2, ?3, ?4, 0, ?5, NULL, NULL, NULL, NULL, NULL, NULL, ?6, ?7)
            ON CONFLICT(entity_id, stage_id, file_path) DO UPDATE SET
                status = excluded.status,
                max_attempts = excluded.max_attempts,
                updated_at = excluded.updated_at
            "#,
            params![
                stage_state.entity_id,
                stage_state.stage_id,
                stage_state.file_path,
                stage_status_value(&stage_state.status),
                stage_state.max_attempts as i64,
                stage_state.discovered_at,
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
        0 => create_schema_v2(connection),
        1 => migrate_v1_to_v2(connection),
        SCHEMA_VERSION => {
            create_supporting_indexes(connection)?;
            write_schema_setting(connection, SCHEMA_VERSION, &Utc::now().to_rfc3339())
        }
        version => Err(format!("Unsupported SQLite schema version: {version}")),
    }
}

fn current_schema_version(connection: &Connection) -> Result<u32, String> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
        .map_err(|error| format!("Failed to read schema version: {error}"))
}

fn create_schema_v2(connection: &Connection) -> Result<(), String> {
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
                validation_status TEXT NOT NULL,
                validation_errors_json TEXT NOT NULL DEFAULT '[]',
                discovered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id)
            );

            CREATE TABLE IF NOT EXISTS entity_stage_states (
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

            CREATE TABLE IF NOT EXISTS stage_runs (
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

            CREATE TABLE IF NOT EXISTS app_events (
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
        .map_err(|error| format!("Failed to initialize SQLite schema v2: {error}"))?;

    create_supporting_indexes(connection)?;
    write_schema_setting(connection, SCHEMA_VERSION, &Utc::now().to_rfc3339())?;

    Ok(())
}

fn migrate_v1_to_v2(connection: &mut Connection) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start schema migration transaction: {error}"))?;

    transaction
        .execute_batch(
            r#"
            PRAGMA foreign_keys = OFF;
            ALTER TABLE stages ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1;
            ALTER TABLE stages ADD COLUMN archived_at TEXT;
            ALTER TABLE stages ADD COLUMN last_seen_in_config_at TEXT;
            UPDATE stages SET last_seen_in_config_at = updated_at WHERE last_seen_in_config_at IS NULL;
            ALTER TABLE entities RENAME TO entities_v1;
            ALTER TABLE entity_stage_states RENAME TO entity_stage_states_v1;
            "#,
        )
        .map_err(|error| format!("Failed to prepare schema migration to v2: {error}"))?;

    transaction
        .execute_batch(
            r#"
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
                validation_status TEXT NOT NULL,
                validation_errors_json TEXT NOT NULL DEFAULT '[]',
                discovered_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id)
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
                FOREIGN KEY (entity_id) REFERENCES entities(entity_id),
                FOREIGN KEY (stage_id) REFERENCES stages(stage_id),
                UNIQUE(entity_id, stage_id, file_path)
            );
            "#,
        )
        .map_err(|error| format!("Failed to create v2 tables during migration: {error}"))?;

    transaction
        .execute(
            r#"
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
                '',
                '',
                COALESCE(current_stage, ''),
                current_stage,
                next_stage,
                status,
                '',
                created_at,
                0,
                payload_json,
                meta_json,
                'invalid',
                '[{"severity":"warning","code":"migrated_from_stage1","path":"entities","message":"Entity migrated from Stage 1 without file metadata."}]',
                created_at,
                updated_at
            FROM entities_v1
            "#,
            [],
        )
        .map_err(|error| format!("Failed to migrate Stage 1 entities to v2: {error}"))?;

    transaction
        .execute(
            r#"
            INSERT INTO entity_stage_states (
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
                state.entity_id,
                state.stage_id,
                '',
                state.status,
                state.attempts,
                COALESCE(stage.max_attempts, 1),
                state.last_error,
                NULL,
                NULL,
                NULL,
                NULL,
                NULL,
                state.created_at,
                state.updated_at
            FROM entity_stage_states_v1 state
            LEFT JOIN stages stage ON stage.stage_id = state.stage_id
            "#,
            [],
        )
        .map_err(|error| format!("Failed to migrate Stage 1 entity states to v2: {error}"))?;

    transaction
        .execute_batch(
            r#"
            DROP TABLE entities_v1;
            DROP TABLE entity_stage_states_v1;
            PRAGMA user_version = 2;
            PRAGMA foreign_keys = ON;
            "#,
        )
        .map_err(|error| format!("Failed to finalize schema migration to v2: {error}"))?;

    create_supporting_indexes(&transaction)?;
    write_schema_setting(&transaction, SCHEMA_VERSION, &now)?;
    insert_app_event(
        &transaction,
        AppEventLevel::Info,
        "schema_migrated_to_v2",
        "SQLite schema migrated from version 1 to version 2.",
        Some(json!({ "from_version": 1, "to_version": 2 })),
        &now,
    )?;

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit schema migration to v2: {error}"))?;

    Ok(())
}

fn create_supporting_indexes(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_stages_is_active ON stages(is_active);
            CREATE INDEX IF NOT EXISTS idx_entities_stage_id ON entities(stage_id);
            CREATE INDEX IF NOT EXISTS idx_entities_status ON entities(status);
            CREATE INDEX IF NOT EXISTS idx_entities_validation_status ON entities(validation_status);
            CREATE INDEX IF NOT EXISTS idx_entity_stage_states_entity_id ON entity_stage_states(entity_id);
            CREATE INDEX IF NOT EXISTS idx_app_events_created_at ON app_events(created_at DESC);
            "#,
        )
        .map_err(|error| format!("Failed to create SQLite indexes: {error}"))?;

    Ok(())
}

fn write_schema_setting(connection: &Connection, version: u32, now: &str) -> Result<(), String> {
    set_setting(connection, "schema_version", &version.to_string(), now)
}

fn sync_stages(connection: &mut Connection, stages: &[StageDefinition]) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to begin stage sync transaction: {error}"))?;
    let now = Utc::now().to_rfc3339();
    let current_stage_ids: Vec<&str> = stages.iter().map(|stage| stage.id.as_str()).collect();

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
                    &stage.id,
                    &stage.input_folder,
                    &stage.output_folder,
                    &stage.workflow_url,
                    stage.max_attempts as i64,
                    stage.retry_delay_sec as i64,
                    stage.next_stage.as_deref(),
                    &now
                ],
            )
            .map_err(|error| format!("Failed to sync stage '{}': {error}", stage.id))?;
    }

    let stale_stage_ids = load_stale_stage_ids(&transaction, &current_stage_ids)?;
    for stage_id in stale_stage_ids {
        transaction
            .execute(
                r#"
                UPDATE stages
                SET
                    is_active = 0,
                    archived_at = COALESCE(archived_at, ?2),
                    updated_at = ?2
                WHERE stage_id = ?1
                "#,
                params![stage_id, &now],
            )
            .map_err(|error| format!("Failed to archive stale stage '{}': {error}", stage_id))?;

        insert_app_event(
            &transaction,
            AppEventLevel::Warning,
            "stage_deactivated",
            &format!("Stage '{stage_id}' was removed from pipeline.yaml and marked inactive."),
            Some(json!({ "stage_id": stage_id })),
            &now,
        )?;
    }

    set_setting(&transaction, "last_stage_sync_at", &now, &now)?;

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit stage sync transaction: {error}"))?;

    Ok(())
}

fn load_stale_stage_ids(
    connection: &Connection,
    current_stage_ids: &[&str],
) -> Result<Vec<String>, String> {
    let sql = if current_stage_ids.is_empty() {
        "SELECT stage_id FROM stages WHERE is_active = 1".to_string()
    } else {
        let placeholders = std::iter::repeat_n("?", current_stage_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "SELECT stage_id FROM stages WHERE is_active = 1 AND stage_id NOT IN ({placeholders})"
        )
    };

    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("Failed to prepare stale stage query: {error}"))?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(current_stage_ids.iter().copied()),
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("Failed to query stale stages: {error}"))?;

    let mut stage_ids = Vec::new();
    for row in rows {
        stage_ids.push(row.map_err(|error| format!("Failed to read stale stage id: {error}"))?);
    }

    Ok(stage_ids)
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
                COUNT(entity.entity_id) as entity_count
            FROM stages stage
            LEFT JOIN entities entity ON entity.stage_id = stage.stage_id
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
        .map_err(|error| format!("Failed to prepare stage list query: {error}"))?;
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

fn load_entities_from_connection(connection: &Connection) -> Result<Vec<EntityRecord>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
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
            FROM entities
            ORDER BY updated_at DESC, entity_id
            "#,
        )
        .map_err(|error| format!("Failed to prepare entity list query: {error}"))?;
    let rows = statement
        .query_map([], entity_from_row)
        .map_err(|error| format!("Failed to query entities: {error}"))?;

    let mut entities = Vec::new();
    for row in rows {
        entities.push(row.map_err(|error| format!("Failed to read entity row: {error}"))?);
    }

    Ok(entities)
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
                status: row.get(4)?,
                attempts: row.get::<_, i64>(5)? as u64,
                max_attempts: row.get::<_, i64>(6)? as u64,
                last_error: row.get(7)?,
                last_http_status: row.get(8)?,
                next_retry_at: row.get(9)?,
                last_started_at: row.get(10)?,
                last_finished_at: row.get(11)?,
                created_child_path: row.get(12)?,
                discovered_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        })
        .map_err(|error| {
            format!("Failed to query stage states for entity '{entity_id}': {error}")
        })?;

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
        .prepare("SELECT status, COUNT(*) FROM entities GROUP BY status ORDER BY status")
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

fn build_json_preview(entity: &EntityRecord) -> Result<String, String> {
    let payload = parse_json_value(&entity.payload_json)?;
    let meta = parse_json_value(&entity.meta_json)?;
    serialize_json_pretty(&json!({
        "id": entity.entity_id,
        "current_stage": entity.current_stage,
        "next_stage": entity.next_stage,
        "status": entity.status,
        "payload": payload,
        "meta": meta,
    }))
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

fn entity_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityRecord> {
    let validation_status = parse_validation_status(&row.get::<_, String>(12)?)?;
    let validation_errors_json: String = row.get(13)?;
    let validation_errors = parse_json::<Vec<ConfigValidationIssue>>(&validation_errors_json)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                13,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
            )
        })?;

    Ok(EntityRecord {
        entity_id: row.get(0)?,
        file_path: row.get(1)?,
        file_name: row.get(2)?,
        stage_id: row.get(3)?,
        current_stage: row.get(4)?,
        next_stage: row.get(5)?,
        status: row.get(6)?,
        checksum: row.get(7)?,
        file_mtime: row.get(8)?,
        file_size: row.get::<_, i64>(9)? as u64,
        payload_json: row.get(10)?,
        meta_json: row.get(11)?,
        validation_status,
        validation_errors,
        discovered_at: row.get(14)?,
        updated_at: row.get(15)?,
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
            12,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unknown entity validation status '{value}'"),
            )),
        )),
    }
}

fn stage_status_value(status: &StageStatus) -> &'static str {
    match status {
        StageStatus::Pending => "pending",
        StageStatus::Queued => "queued",
        StageStatus::InProgress => "in_progress",
        StageStatus::RetryWait => "retry_wait",
        StageStatus::Done => "done",
        StageStatus::Failed => "failed",
        StageStatus::Blocked => "blocked",
        StageStatus::Skipped => "skipped",
    }
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

pub(crate) fn system_time_to_rfc3339(value: std::time::SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339()
}

#[cfg(test)]
fn load_stage_by_id(connection: &Connection, stage_id: &str) -> Result<StageRecord, String> {
    connection
        .query_row(
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
                COUNT(entity.entity_id) as entity_count
            FROM stages stage
            LEFT JOIN entities entity ON entity.stage_id = stage.stage_id
            WHERE stage.stage_id = ?1
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
            "#,
            params![stage_id],
            |row| {
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
            },
        )
        .map_err(|error| format!("Failed to load stage '{stage_id}': {error}"))
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
    use crate::domain::{PipelineConfig, ProjectConfig, RuntimeConfig};

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

    #[test]
    fn bootstrap_creates_database_file_and_required_tables_at_v2() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config(vec![stage("ingest", Some("normalize"))]);

        let result = bootstrap_database(&database_path, &config).expect("bootstrap");
        let connection = Connection::open(&database_path).expect("open db");
        let table_names = load_table_names(&connection).expect("load table names");

        assert!(database_path.exists());
        assert_eq!(result.schema_version, 2);
        assert_eq!(result.active_stage_count, 1);
        assert_eq!(result.inactive_stage_count, 0);
        assert!(table_names.contains(&"settings".to_string()));
        assert!(table_names.contains(&"stages".to_string()));
        assert!(table_names.contains(&"entities".to_string()));
        assert!(table_names.contains(&"entity_stage_states".to_string()));
        assert!(table_names.contains(&"stage_runs".to_string()));
        assert!(table_names.contains(&"app_events".to_string()));
    }

    #[test]
    fn existing_v1_database_is_migrated_to_v2() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let connection = Connection::open(&database_path).expect("open db");
        create_v1_schema(&connection);
        let now = Utc::now().to_rfc3339();
        connection
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
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, 3, 10, NULL, ?5, ?5)
                "#,
                params![
                    "ingest",
                    "stages/incoming",
                    "stages/out",
                    "http://localhost/workflow",
                    &now
                ],
            )
            .expect("seed stage");
        drop(connection);

        let result = bootstrap_database(&database_path, &test_config(vec![stage("ingest", None)]))
            .expect("bootstrap migrated db");
        let connection = Connection::open(&database_path).expect("open migrated db");
        let stage = load_stage_by_id(&connection, "ingest").expect("load stage");
        let events = load_app_events_from_connection(&connection, 10).expect("load events");

        assert_eq!(result.schema_version, 2);
        assert!(stage.is_active);
        assert!(stage.last_seen_in_config_at.is_some());
        assert!(events
            .iter()
            .any(|event| event.code == "schema_migrated_to_v2"));
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
        .expect("first bootstrap");
        let result =
            bootstrap_database(&database_path, &test_config(vec![stage("normalize", None)]))
                .expect("second bootstrap");

        let connection = Connection::open(&database_path).expect("open db");
        let ingest = load_stage_by_id(&connection, "ingest").expect("load ingest");
        let normalize = load_stage_by_id(&connection, "normalize").expect("load normalize");

        assert_eq!(result.active_stage_count, 1);
        assert_eq!(result.inactive_stage_count, 1);
        assert!(!ingest.is_active);
        assert!(ingest.archived_at.is_some());
        assert!(normalize.is_active);
    }

    #[test]
    fn active_stage_is_reactivated_when_it_returns_to_yaml() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");

        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("ingest", Some("normalize")),
                stage("normalize", None),
            ]),
        )
        .expect("first bootstrap");
        bootstrap_database(&database_path, &test_config(vec![stage("normalize", None)]))
            .expect("second bootstrap");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage("ingest", Some("normalize")),
                stage("normalize", None),
            ]),
        )
        .expect("third bootstrap");

        let connection = Connection::open(&database_path).expect("open db");
        let ingest = load_stage_by_id(&connection, "ingest").expect("load ingest");

        assert!(ingest.is_active);
        assert_eq!(ingest.archived_at, None);
    }
}

use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::domain::{DatabaseState, PipelineConfig, StageDefinition};
use crate::workdir::path_string;

pub fn bootstrap_database(path: &Path, config: &PipelineConfig) -> Result<DatabaseState, String> {
    let mut connection = Connection::open(path).map_err(|error| {
        format!(
            "Failed to open SQLite database '{}': {error}",
            path.display()
        )
    })?;

    initialize_schema(&connection)?;
    sync_stages(&mut connection, &config.stages)?;

    let stage_ids = load_stage_ids(&connection)?;
    let schema_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
        .map_err(|error| format!("Failed to read schema version: {error}"))?;

    Ok(DatabaseState {
        database_path: path_string(path),
        is_ready: true,
        schema_version,
        stage_count: stage_ids.len() as u64,
        synced_stage_ids: stage_ids,
    })
}

fn initialize_schema(connection: &Connection) -> Result<(), String> {
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
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS entities (
                entity_id TEXT PRIMARY KEY,
                current_stage TEXT,
                next_stage TEXT,
                status TEXT NOT NULL,
                payload_json TEXT NOT NULL DEFAULT '{}',
                meta_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS entity_stage_states (
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

            PRAGMA user_version = 1;
            "#,
        )
        .map_err(|error| format!("Failed to initialize SQLite schema: {error}"))?;

    let now = Utc::now().to_rfc3339();
    connection
        .execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES ('schema_version', '1', ?1)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            params![now],
        )
        .map_err(|error| format!("Failed to write schema setting: {error}"))?;

    Ok(())
}

fn sync_stages(connection: &mut Connection, stages: &[StageDefinition]) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to begin stage sync transaction: {error}"))?;
    let now = Utc::now().to_rfc3339();
    let stage_ids: Vec<&str> = stages.iter().map(|stage| stage.id.as_str()).collect();

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
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                ON CONFLICT(stage_id) DO UPDATE SET
                    input_folder = excluded.input_folder,
                    output_folder = excluded.output_folder,
                    workflow_url = excluded.workflow_url,
                    max_attempts = excluded.max_attempts,
                    retry_delay_sec = excluded.retry_delay_sec,
                    next_stage = excluded.next_stage,
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

    if stage_ids.is_empty() {
        transaction
            .execute("DELETE FROM stages", [])
            .map_err(|error| format!("Failed to clear stale stages: {error}"))?;
    } else {
        let placeholders = std::iter::repeat_n("?", stage_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let delete_sql = format!("DELETE FROM stages WHERE stage_id NOT IN ({placeholders})");
        transaction
            .execute(
                &delete_sql,
                rusqlite::params_from_iter(stage_ids.iter().copied()),
            )
            .map_err(|error| format!("Failed to delete stale stages: {error}"))?;
    }

    transaction
        .execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES ('last_stage_sync_at', ?1, ?1)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            params![now],
        )
        .map_err(|error| format!("Failed to write stage sync metadata: {error}"))?;

    transaction
        .commit()
        .map_err(|error| format!("Failed to commit stage sync transaction: {error}"))?;

    Ok(())
}

fn load_stage_ids(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare("SELECT stage_id FROM stages ORDER BY stage_id")
        .map_err(|error| format!("Failed to prepare stage list query: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Failed to query synced stages: {error}"))?;

    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|error| format!("Failed to read synced stage id: {error}"))?);
    }

    Ok(ids)
}

#[cfg(test)]
fn load_stage_by_id(connection: &Connection, stage_id: &str) -> Result<StageDefinition, String> {
    connection
        .query_row(
            r#"
            SELECT
                stage_id,
                input_folder,
                output_folder,
                workflow_url,
                max_attempts,
                retry_delay_sec,
                next_stage
            FROM stages
            WHERE stage_id = ?1
            "#,
            params![stage_id],
            |row| {
                Ok(StageDefinition {
                    id: row.get(0)?,
                    input_folder: row.get(1)?,
                    output_folder: row.get(2)?,
                    workflow_url: row.get(3)?,
                    max_attempts: row.get::<_, i64>(4)? as u64,
                    retry_delay_sec: row.get::<_, i64>(5)? as u64,
                    next_stage: row.get(6)?,
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
    use crate::domain::{PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition};

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

    #[test]
    fn stage_sync_is_idempotent() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config(vec![StageDefinition {
            id: "ingest".to_string(),
            input_folder: "stages/incoming".to_string(),
            output_folder: "stages/normalized".to_string(),
            workflow_url: "http://localhost:5678/webhook/ingest".to_string(),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: Some("normalize".to_string()),
        }]);

        let first = bootstrap_database(&database_path, &config).expect("first bootstrap");
        let second = bootstrap_database(&database_path, &config).expect("second bootstrap");

        assert_eq!(first.stage_count, 1);
        assert_eq!(second.stage_count, 1);
        assert_eq!(second.synced_stage_ids, vec!["ingest".to_string()]);
    }

    #[test]
    fn stage_sync_updates_existing_stage_definition() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let first_config = test_config(vec![StageDefinition {
            id: "ingest".to_string(),
            input_folder: "stages/incoming".to_string(),
            output_folder: "stages/normalized".to_string(),
            workflow_url: "http://localhost:5678/webhook/ingest".to_string(),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: Some("normalize".to_string()),
        }]);
        let second_config = test_config(vec![StageDefinition {
            id: "ingest".to_string(),
            input_folder: "stages/review".to_string(),
            output_folder: "stages/final".to_string(),
            workflow_url: "http://localhost:5678/webhook/ingest-v2".to_string(),
            max_attempts: 5,
            retry_delay_sec: 45,
            next_stage: None,
        }]);

        bootstrap_database(&database_path, &first_config).expect("first bootstrap");
        bootstrap_database(&database_path, &second_config).expect("second bootstrap");

        let connection = Connection::open(&database_path).expect("open db");
        let stage = load_stage_by_id(&connection, "ingest").expect("load stage");

        assert_eq!(stage.input_folder, "stages/review");
        assert_eq!(stage.output_folder, "stages/final");
        assert_eq!(
            stage.workflow_url,
            "http://localhost:5678/webhook/ingest-v2"
        );
        assert_eq!(stage.max_attempts, 5);
        assert_eq!(stage.retry_delay_sec, 45);
        assert_eq!(stage.next_stage, None);
    }

    #[test]
    fn stage_sync_removes_stale_stage_definitions() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let first_config = test_config(vec![
            StageDefinition {
                id: "ingest".to_string(),
                input_folder: "stages/incoming".to_string(),
                output_folder: "stages/normalized".to_string(),
                workflow_url: "http://localhost:5678/webhook/ingest".to_string(),
                max_attempts: 3,
                retry_delay_sec: 10,
                next_stage: Some("normalize".to_string()),
            },
            StageDefinition {
                id: "normalize".to_string(),
                input_folder: "stages/normalized".to_string(),
                output_folder: "stages/done".to_string(),
                workflow_url: "http://localhost:5678/webhook/normalize".to_string(),
                max_attempts: 3,
                retry_delay_sec: 10,
                next_stage: None,
            },
        ]);
        let second_config = test_config(vec![StageDefinition {
            id: "normalize".to_string(),
            input_folder: "stages/normalized".to_string(),
            output_folder: "stages/done".to_string(),
            workflow_url: "http://localhost:5678/webhook/normalize".to_string(),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: None,
        }]);

        bootstrap_database(&database_path, &first_config).expect("first bootstrap");
        let result = bootstrap_database(&database_path, &second_config).expect("second bootstrap");
        let repeated = bootstrap_database(&database_path, &second_config).expect("third bootstrap");

        assert_eq!(result.synced_stage_ids, vec!["normalize".to_string()]);
        assert_eq!(repeated.synced_stage_ids, vec!["normalize".to_string()]);

        let connection = Connection::open(&database_path).expect("open db");
        let stage_ids = load_stage_ids(&connection).expect("load stage ids");
        assert_eq!(stage_ids, vec!["normalize".to_string()]);
    }

    #[test]
    fn bootstrap_creates_database_file_and_required_tables() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        let config = test_config(vec![StageDefinition {
            id: "ingest".to_string(),
            input_folder: "stages/incoming".to_string(),
            output_folder: "stages/normalized".to_string(),
            workflow_url: "http://localhost:5678/webhook/ingest".to_string(),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: Some("normalize".to_string()),
        }]);

        let result = bootstrap_database(&database_path, &config).expect("bootstrap");
        let connection = Connection::open(&database_path).expect("open db");
        let table_names = load_table_names(&connection).expect("load table names");

        assert!(database_path.exists());
        assert_eq!(result.schema_version, 1);
        assert!(table_names.contains(&"settings".to_string()));
        assert!(table_names.contains(&"stages".to_string()));
        assert!(table_names.contains(&"entities".to_string()));
        assert!(table_names.contains(&"entity_stage_states".to_string()));
        assert!(table_names.contains(&"stage_runs".to_string()));
        assert!(table_names.contains(&"app_events".to_string()));
    }
}

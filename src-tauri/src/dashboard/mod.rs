use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::database::{load_setting, open_connection};
use crate::domain::{
    DashboardActiveTask, DashboardErrorItem, DashboardOverview, DashboardProjectContext,
    DashboardRunItem, DashboardRuntimeOverview, DashboardStageCounters, DashboardStageEdge,
    DashboardStageGraph, DashboardStageHealth, DashboardStageNode, DashboardTotals, StageRecord,
};
use crate::workdir::path_string;

const ACTIVE_TASK_LIMIT: u32 = 50;
const ERROR_LIMIT: u32 = 20;
const RUN_LIMIT: u32 = 20;

#[derive(Debug, Clone, Default)]
struct CounterBucket {
    total: u64,
    pending: u64,
    queued: u64,
    in_progress: u64,
    retry_wait: u64,
    done: u64,
    failed: u64,
    blocked: u64,
    skipped: u64,
    unknown: u64,
    missing_files: u64,
    existing_files: u64,
    last_started_at: Option<String>,
    last_finished_at: Option<String>,
}

pub fn get_dashboard_overview(
    database_path: &Path,
    project_name: &str,
    workdir_path: &Path,
) -> Result<DashboardOverview, String> {
    let connection = open_connection(database_path)?;
    let now = Utc::now().to_rfc3339();
    let stages = load_dashboard_stages(&connection)?;
    let counters_by_stage = load_stage_counters(&connection, &stages)?;
    let edges = build_stage_edges(&stages);
    let nodes = build_stage_nodes(&stages, &counters_by_stage, &edges);
    let active_tasks = load_active_tasks(&connection, &now, ACTIVE_TASK_LIMIT)?;
    let last_errors = load_last_errors(&connection, ERROR_LIMIT)?;
    let recent_runs = load_recent_runs(&connection, RUN_LIMIT)?;

    let stages_total = stages.len() as u64;
    let active_stages_total = stages.iter().filter(|stage| stage.is_active).count() as u64;
    let inactive_stages_total = stages_total.saturating_sub(active_stages_total);
    let runtime = DashboardRuntimeOverview {
        last_scan_at: load_setting(&connection, "last_scan_completed_at")?,
        last_run_at: optional_string(
            &connection,
            "SELECT MAX(started_at) FROM stage_runs",
            [],
            "last run timestamp",
        )?,
        last_successful_run_at: optional_string(
            &connection,
            "SELECT MAX(finished_at) FROM stage_runs WHERE success = 1",
            [],
            "last successful run timestamp",
        )?,
        last_error_at: optional_string(
            &connection,
            "SELECT MAX(created_at) FROM app_events WHERE level = 'error'",
            [],
            "last error timestamp",
        )?,
        due_tasks_count: count_due_tasks(&connection, &now)?,
        in_progress_count: count_status(&connection, "in_progress")?,
        retry_wait_count: count_status(&connection, "retry_wait")?,
        failed_count: count_status(&connection, "failed")?,
        blocked_count: count_status(&connection, "blocked")?,
    };

    let totals = DashboardTotals {
        entities_total: query_count(&connection, "SELECT COUNT(*) FROM entities", [])?,
        entity_files_total: query_count(&connection, "SELECT COUNT(*) FROM entity_files", [])?,
        stages_total,
        active_stages_total,
        inactive_stages_total,
        active_tasks_total: query_count(
            &connection,
            "SELECT COUNT(*) FROM entity_stage_states WHERE status IN ('pending', 'queued', 'in_progress', 'retry_wait')",
            [],
        )?,
        errors_total: query_count(
            &connection,
            "SELECT COUNT(*) FROM app_events WHERE level = 'error'",
            [],
        )?,
        warnings_total: query_count(
            &connection,
            "SELECT COUNT(*) FROM app_events WHERE level = 'warning'",
            [],
        )?,
    };

    Ok(DashboardOverview {
        generated_at: now,
        project: DashboardProjectContext {
            name: project_name.to_string(),
            workdir_path: path_string(workdir_path),
        },
        totals,
        runtime,
        stage_graph: DashboardStageGraph { nodes, edges },
        stage_counters: stages
            .iter()
            .map(|stage| {
                let bucket = counters_by_stage
                    .get(&stage.id)
                    .cloned()
                    .unwrap_or_default();
                DashboardStageCounters {
                    stage_id: stage.id.clone(),
                    stage_label: stage.id.clone(),
                    is_active: stage.is_active,
                    total: bucket.total,
                    pending: bucket.pending,
                    queued: bucket.queued,
                    in_progress: bucket.in_progress,
                    retry_wait: bucket.retry_wait,
                    done: bucket.done,
                    failed: bucket.failed,
                    blocked: bucket.blocked,
                    skipped: bucket.skipped,
                    unknown: bucket.unknown,
                    missing_files: bucket.missing_files,
                    existing_files: bucket.existing_files,
                    last_started_at: bucket.last_started_at,
                    last_finished_at: bucket.last_finished_at,
                }
            })
            .collect(),
        active_tasks,
        last_errors,
        recent_runs,
    })
}

fn load_dashboard_stages(connection: &Connection) -> Result<Vec<StageRecord>, String> {
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
                COUNT(DISTINCT state.entity_id)
            FROM stages stage
            LEFT JOIN entity_stage_states state ON state.stage_id = stage.stage_id
            GROUP BY stage.stage_id
            ORDER BY stage.created_at ASC, stage.stage_id ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare dashboard stage query: {error}"))?;
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
        .map_err(|error| format!("Failed to query dashboard stages: {error}"))?;

    let mut stages = Vec::new();
    for row in rows {
        stages.push(row.map_err(|error| format!("Failed to read dashboard stage row: {error}"))?);
    }
    Ok(stages)
}

fn build_stage_edges(stages: &[StageRecord]) -> Vec<DashboardStageEdge> {
    let stage_by_id: HashMap<&str, &StageRecord> = stages
        .iter()
        .map(|stage| (stage.id.as_str(), stage))
        .collect();
    stages
        .iter()
        .filter_map(|stage| {
            let target_id = stage.next_stage.as_ref()?;
            match stage_by_id.get(target_id.as_str()) {
                Some(target) if target.is_active => Some(DashboardStageEdge {
                    from_stage_id: stage.id.clone(),
                    to_stage_id: target_id.clone(),
                    is_valid: true,
                    problem: None,
                }),
                Some(_) => Some(DashboardStageEdge {
                    from_stage_id: stage.id.clone(),
                    to_stage_id: target_id.clone(),
                    is_valid: false,
                    problem: Some(format!("Target stage '{target_id}' is inactive.")),
                }),
                None => Some(DashboardStageEdge {
                    from_stage_id: stage.id.clone(),
                    to_stage_id: target_id.clone(),
                    is_valid: false,
                    problem: Some(format!("Target stage '{target_id}' does not exist.")),
                }),
            }
        })
        .collect()
}

fn build_stage_nodes(
    stages: &[StageRecord],
    counters_by_stage: &HashMap<String, CounterBucket>,
    edges: &[DashboardStageEdge],
) -> Vec<DashboardStageNode> {
    let missing_edge_sources: HashSet<&str> = edges
        .iter()
        .filter(|edge| {
            !edge.is_valid
                && edge
                    .problem
                    .as_deref()
                    .unwrap_or_default()
                    .contains("does not exist")
        })
        .map(|edge| edge.from_stage_id.as_str())
        .collect();
    let inactive_edge_sources: HashSet<&str> = edges
        .iter()
        .filter(|edge| {
            !edge.is_valid
                && edge
                    .problem
                    .as_deref()
                    .unwrap_or_default()
                    .contains("inactive")
        })
        .map(|edge| edge.from_stage_id.as_str())
        .collect();

    stages
        .iter()
        .enumerate()
        .map(|(index, stage)| {
            let bucket = counters_by_stage
                .get(&stage.id)
                .cloned()
                .unwrap_or_default();
            let health = if !stage.is_active {
                DashboardStageHealth::Inactive
            } else if bucket.failed > 0
                || bucket.blocked > 0
                || missing_edge_sources.contains(stage.id.as_str())
            {
                DashboardStageHealth::Error
            } else if bucket.retry_wait > 0
                || bucket.in_progress > 0
                || inactive_edge_sources.contains(stage.id.as_str())
            {
                DashboardStageHealth::Warning
            } else {
                DashboardStageHealth::Ok
            };
            DashboardStageNode {
                id: stage.id.clone(),
                label: stage.id.clone(),
                input_folder: stage.input_folder.clone(),
                output_folder: (!stage.output_folder.trim().is_empty())
                    .then(|| stage.output_folder.clone()),
                workflow_url: Some(stage.workflow_url.clone()),
                is_active: stage.is_active,
                archived_at: stage.archived_at.clone(),
                next_stage: stage.next_stage.clone(),
                position_index: index as u64,
                health,
            }
        })
        .collect()
}

fn load_stage_counters(
    connection: &Connection,
    stages: &[StageRecord],
) -> Result<HashMap<String, CounterBucket>, String> {
    let mut counters: HashMap<String, CounterBucket> = stages
        .iter()
        .map(|stage| (stage.id.clone(), CounterBucket::default()))
        .collect();

    let mut statement = connection
        .prepare(
            r#"
            SELECT
                stage_id,
                status,
                COUNT(*),
                MAX(last_started_at),
                MAX(last_finished_at)
            FROM entity_stage_states
            GROUP BY stage_id, status
            "#,
        )
        .map_err(|error| format!("Failed to prepare stage counter query: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|error| format!("Failed to query stage counters: {error}"))?;

    for row in rows {
        let (stage_id, status, count, last_started_at, last_finished_at) =
            row.map_err(|error| format!("Failed to read stage counter row: {error}"))?;
        let bucket = counters.entry(stage_id).or_default();
        bucket.total += count;
        match status.as_str() {
            "pending" => bucket.pending += count,
            "queued" => bucket.queued += count,
            "in_progress" => bucket.in_progress += count,
            "retry_wait" => bucket.retry_wait += count,
            "done" => bucket.done += count,
            "failed" => bucket.failed += count,
            "blocked" => bucket.blocked += count,
            "skipped" => bucket.skipped += count,
            _ => bucket.unknown += count,
        }
        bucket.last_started_at = max_optional_text(bucket.last_started_at.take(), last_started_at);
        bucket.last_finished_at =
            max_optional_text(bucket.last_finished_at.take(), last_finished_at);
    }

    let mut file_statement = connection
        .prepare(
            r#"
            SELECT
                stage_id,
                SUM(CASE WHEN file_exists = 1 THEN 1 ELSE 0 END),
                SUM(CASE WHEN file_exists = 0 THEN 1 ELSE 0 END)
            FROM entity_files
            GROUP BY stage_id
            "#,
        )
        .map_err(|error| format!("Failed to prepare stage file counter query: {error}"))?;
    let file_rows = file_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or_default() as u64,
                row.get::<_, Option<i64>>(2)?.unwrap_or_default() as u64,
            ))
        })
        .map_err(|error| format!("Failed to query stage file counters: {error}"))?;
    for row in file_rows {
        let (stage_id, existing_files, missing_files) =
            row.map_err(|error| format!("Failed to read stage file counter row: {error}"))?;
        let bucket = counters.entry(stage_id).or_default();
        bucket.existing_files = existing_files;
        bucket.missing_files = missing_files;
    }

    Ok(counters)
}

fn load_active_tasks(
    connection: &Connection,
    now: &str,
    limit: u32,
) -> Result<Vec<DashboardActiveTask>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT
                entity_id,
                stage_id,
                status,
                attempts,
                max_attempts,
                next_retry_at,
                last_started_at,
                updated_at,
                file_path,
                last_error
            FROM entity_stage_states
            WHERE status IN ('in_progress', 'queued', 'retry_wait', 'pending')
            ORDER BY
                CASE
                    WHEN status = 'in_progress' THEN 0
                    WHEN status = 'queued' THEN 1
                    WHEN status = 'retry_wait' AND next_retry_at IS NOT NULL AND next_retry_at <= ?1 THEN 2
                    WHEN status = 'retry_wait' THEN 3
                    WHEN status = 'pending' THEN 4
                    ELSE 4
                END,
                COALESCE(last_started_at, updated_at) ASC,
                id ASC
            LIMIT ?2
            "#,
        )
        .map_err(|error| format!("Failed to prepare active task query: {error}"))?;
    let rows = statement
        .query_map(params![now, limit as i64], |row| {
            let status: String = row.get(2)?;
            let next_retry_at: Option<String> = row.get(5)?;
            let reason = task_reason(&status, next_retry_at.as_deref(), now);
            Ok(DashboardActiveTask {
                entity_id: row.get(0)?,
                stage_id: row.get(1)?,
                status,
                attempts: row.get::<_, i64>(3)? as u64,
                max_attempts: row.get::<_, i64>(4)? as u64,
                next_retry_at,
                last_started_at: row.get(6)?,
                updated_at: row.get(7)?,
                file_path: row.get(8)?,
                reason,
            })
        })
        .map_err(|error| format!("Failed to query active tasks: {error}"))?;

    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row.map_err(|error| format!("Failed to read active task row: {error}"))?);
    }
    Ok(tasks)
}

fn load_last_errors(
    connection: &Connection,
    limit: u32,
) -> Result<Vec<DashboardErrorItem>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT id, level, code, message, context_json, created_at
            FROM app_events
            WHERE level IN ('error', 'warning')
            ORDER BY created_at DESC, id DESC
            LIMIT ?1
            "#,
        )
        .map_err(|error| format!("Failed to prepare dashboard error query: {error}"))?;
    let rows = statement
        .query_map(params![limit as i64], |row| {
            let context_json: Option<String> = row.get(4)?;
            let context = context_json
                .as_deref()
                .and_then(|text| serde_json::from_str::<Value>(text).ok());
            Ok(DashboardErrorItem {
                id: row.get(0)?,
                level: row.get(1)?,
                event_type: row.get(2)?,
                message: row.get(3)?,
                entity_id: context_string(&context, "entity_id"),
                stage_id: context_string(&context, "stage_id"),
                run_id: context_string(&context, "run_id"),
                created_at: row.get(5)?,
            })
        })
        .map_err(|error| format!("Failed to query dashboard errors: {error}"))?;

    let mut errors = Vec::new();
    for row in rows {
        errors.push(row.map_err(|error| format!("Failed to read dashboard error row: {error}"))?);
    }
    Ok(errors)
}

fn load_recent_runs(connection: &Connection, limit: u32) -> Result<Vec<DashboardRunItem>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT run_id, entity_id, stage_id, success, http_status, error_type, error_message,
                   started_at, finished_at, duration_ms
            FROM stage_runs
            ORDER BY started_at DESC, id DESC
            LIMIT ?1
            "#,
        )
        .map_err(|error| format!("Failed to prepare recent run query: {error}"))?;
    let rows = statement
        .query_map(params![limit as i64], |row| {
            Ok(DashboardRunItem {
                run_id: row.get(0)?,
                entity_id: row.get(1)?,
                stage_id: row.get(2)?,
                success: row.get::<_, i64>(3)? == 1,
                http_status: row.get(4)?,
                error_type: row.get(5)?,
                error_message: row.get(6)?,
                started_at: row.get(7)?,
                finished_at: row.get(8)?,
                duration_ms: row.get::<_, Option<i64>>(9)?.map(|value| value as u64),
            })
        })
        .map_err(|error| format!("Failed to query recent runs: {error}"))?;

    let mut runs = Vec::new();
    for row in rows {
        runs.push(row.map_err(|error| format!("Failed to read recent run row: {error}"))?);
    }
    Ok(runs)
}

fn count_due_tasks(connection: &Connection, now: &str) -> Result<u64, String> {
    query_count(
        connection,
        r#"
        SELECT COUNT(*)
        FROM entity_stage_states state
        INNER JOIN stages stage ON stage.stage_id = state.stage_id
        WHERE stage.is_active = 1
          AND state.file_exists = 1
          AND state.attempts < state.max_attempts
          AND (
              state.status = 'pending'
              OR (state.status = 'retry_wait' AND state.next_retry_at IS NOT NULL AND state.next_retry_at <= ?1)
          )
        "#,
        params![now],
    )
}

fn count_status(connection: &Connection, status: &str) -> Result<u64, String> {
    query_count(
        connection,
        "SELECT COUNT(*) FROM entity_stage_states WHERE status = ?1",
        params![status],
    )
}

fn query_count<P>(connection: &Connection, sql: &str, params: P) -> Result<u64, String>
where
    P: rusqlite::Params,
{
    connection
        .query_row(sql, params, |row| row.get::<_, i64>(0))
        .map(|value| value as u64)
        .map_err(|error| format!("Failed to query dashboard count: {error}"))
}

fn optional_string<P>(
    connection: &Connection,
    sql: &str,
    params: P,
    label: &str,
) -> Result<Option<String>, String>
where
    P: rusqlite::Params,
{
    connection
        .query_row(sql, params, |row| row.get::<_, Option<String>>(0))
        .optional()
        .map(|value| value.flatten())
        .map_err(|error| format!("Failed to query dashboard {label}: {error}"))
}

fn max_optional_text(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn task_reason(status: &str, next_retry_at: Option<&str>, now: &str) -> Option<String> {
    match status {
        "in_progress" => Some("Task is currently in progress.".to_string()),
        "queued" => Some("Task is queued for execution.".to_string()),
        "pending" => Some("Task is pending execution.".to_string()),
        "retry_wait" => match next_retry_at {
            Some(value) if value <= now => Some("Retry is due now.".to_string()),
            Some(value) => Some(format!("Retry waits until {value}.")),
            None => Some("Retry wait has no next retry timestamp.".to_string()),
        },
        _ => None,
    }
}

fn context_string(context: &Option<Value>, key: &str) -> Option<String> {
    context
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use rusqlite::{params, Connection};
    use serde_json::json;

    use super::*;
    use crate::database::{bootstrap_database, insert_app_event, set_setting};
    use crate::domain::{
        AppEventLevel, PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition,
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

    fn setup_database(stages: Vec<StageDefinition>) -> (tempfile::TempDir, std::path::PathBuf) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let database_path = tempdir.path().join("app.db");
        bootstrap_database(&database_path, &test_config(stages)).expect("bootstrap");
        (tempdir, database_path)
    }

    fn seed_state(
        connection: &Connection,
        entity_id: &str,
        stage_id: &str,
        status: &str,
        file_exists: bool,
        next_retry_at: Option<&str>,
    ) {
        let now = Utc::now().to_rfc3339();
        let file_path = format!("stages/{stage_id}/{entity_id}.json");
        connection
            .execute(
                r#"
                INSERT OR IGNORE INTO entities (
                    entity_id, current_stage_id, current_status, latest_file_path, latest_file_id,
                    file_count, validation_status, validation_errors_json, first_seen_at, last_seen_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, NULL, 1, 'valid', '[]', ?5, ?5, ?5)
                "#,
                params![entity_id, stage_id, status, &file_path, &now],
            )
            .expect("insert entity");
        connection
            .execute(
                r#"
                INSERT INTO entity_files (
                    entity_id, stage_id, file_path, file_name, checksum, file_mtime, file_size,
                    payload_json, meta_json, current_stage, next_stage, status, validation_status,
                    validation_errors_json, is_managed_copy, copy_source_file_id, file_exists,
                    missing_since, first_seen_at, last_seen_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, 'checksum', ?5, 12, '{}', '{}', ?2, NULL, ?6, 'valid',
                    '[]', 0, NULL, ?7, NULL, ?5, ?5, ?5)
                "#,
                params![
                    entity_id,
                    stage_id,
                    &file_path,
                    format!("{entity_id}.json"),
                    &now,
                    status,
                    if file_exists { 1 } else { 0 }
                ],
            )
            .expect("insert file");
        let file_id = connection.last_insert_rowid();
        connection
            .execute(
                r#"
                INSERT INTO entity_stage_states (
                    entity_id, stage_id, file_path, file_instance_id, file_exists, status,
                    attempts, max_attempts, last_error, last_http_status, next_retry_at,
                    last_started_at, last_finished_at, created_child_path, discovered_at,
                    last_seen_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 3, NULL, NULL, ?7, ?8, ?9, NULL, ?10, ?10, ?10)
                "#,
                params![
                    entity_id,
                    stage_id,
                    &file_path,
                    file_id,
                    if file_exists { 1 } else { 0 },
                    status,
                    next_retry_at,
                    if status == "in_progress" {
                        Some(now.as_str())
                    } else {
                        None
                    },
                    if status == "done" || status == "failed" {
                        Some(now.as_str())
                    } else {
                        None
                    },
                    &now
                ],
            )
            .expect("insert state");
    }

    fn overview(database_path: &Path) -> DashboardOverview {
        let workdir = database_path.parent().expect("parent");
        get_dashboard_overview(database_path, "beehive", workdir).expect("overview")
    }

    #[test]
    fn fresh_database_returns_stages_and_zero_counters() {
        let (_tempdir, database_path) = setup_database(vec![
            stage("incoming", Some("normalized")),
            stage("normalized", None),
        ]);

        let overview = overview(&database_path);

        assert_eq!(overview.totals.entities_total, 0);
        assert_eq!(overview.stage_graph.nodes.len(), 2);
        assert_eq!(overview.stage_graph.edges.len(), 1);
        assert!(overview.stage_graph.edges[0].is_valid);
        assert!(overview
            .stage_counters
            .iter()
            .all(|counter| counter.total == 0));
    }

    #[test]
    fn stage_graph_marks_missing_and_inactive_targets() {
        let (_tempdir, database_path) = setup_database(vec![
            stage("incoming", Some("normalized")),
            stage("normalized", Some("missing")),
        ]);
        bootstrap_database(
            &database_path,
            &test_config(vec![stage("incoming", Some("normalized"))]),
        )
        .expect("archive normalized");

        let overview = overview(&database_path);
        let incoming = overview
            .stage_graph
            .edges
            .iter()
            .find(|edge| edge.from_stage_id == "incoming")
            .expect("incoming edge");
        let normalized = overview
            .stage_graph
            .edges
            .iter()
            .find(|edge| edge.from_stage_id == "normalized")
            .expect("normalized edge");

        assert!(!incoming.is_valid);
        assert!(incoming
            .problem
            .as_deref()
            .unwrap_or_default()
            .contains("inactive"));
        assert!(!normalized.is_valid);
        assert!(normalized
            .problem
            .as_deref()
            .unwrap_or_default()
            .contains("does not exist"));
        assert_eq!(
            overview
                .stage_graph
                .nodes
                .iter()
                .find(|node| node.id == "normalized")
                .expect("normalized node")
                .health,
            DashboardStageHealth::Inactive
        );
    }

    #[test]
    fn stage_graph_edges_follow_next_stage_not_node_order() {
        let (_tempdir, database_path) = setup_database(vec![
            stage("a", Some("c")),
            stage("b", Some("d")),
            stage("c", None),
            stage("d", None),
        ]);

        let overview = overview(&database_path);
        let edges: HashSet<(String, String)> = overview
            .stage_graph
            .edges
            .iter()
            .map(|edge| (edge.from_stage_id.clone(), edge.to_stage_id.clone()))
            .collect();

        assert!(edges.contains(&("a".to_string(), "c".to_string())));
        assert!(edges.contains(&("b".to_string(), "d".to_string())));
        assert!(!edges.contains(&("a".to_string(), "b".to_string())));
        assert!(!edges.contains(&("b".to_string(), "c".to_string())));
        assert_eq!(overview.stage_graph.edges.len(), 2);
    }

    #[test]
    fn status_counters_and_active_tasks_are_aggregated_from_stage_states() {
        let (_tempdir, database_path) = setup_database(vec![
            stage("incoming", Some("normalized")),
            stage("normalized", None),
        ]);
        let connection = open_connection(&database_path).expect("open");
        let future_retry = (Utc::now() + Duration::minutes(10)).to_rfc3339();
        seed_state(&connection, "pending-1", "incoming", "pending", true, None);
        seed_state(&connection, "queued-1", "incoming", "queued", true, None);
        seed_state(&connection, "done-1", "incoming", "done", true, None);
        seed_state(&connection, "failed-1", "incoming", "failed", true, None);
        seed_state(&connection, "blocked-1", "incoming", "blocked", false, None);
        seed_state(&connection, "skipped-1", "incoming", "skipped", true, None);
        seed_state(
            &connection,
            "unknown-1",
            "incoming",
            "custom_hold",
            true,
            None,
        );
        seed_state(
            &connection,
            "retry-1",
            "incoming",
            "retry_wait",
            true,
            Some(&future_retry),
        );
        seed_state(
            &connection,
            "running-1",
            "normalized",
            "in_progress",
            true,
            None,
        );
        drop(connection);

        let overview = overview(&database_path);
        let incoming = overview
            .stage_counters
            .iter()
            .find(|counter| counter.stage_id == "incoming")
            .expect("incoming counters");

        assert_eq!(incoming.pending, 1);
        assert_eq!(incoming.queued, 1);
        assert_eq!(incoming.done, 1);
        assert_eq!(incoming.failed, 1);
        assert_eq!(incoming.blocked, 1);
        assert_eq!(incoming.skipped, 1);
        assert_eq!(incoming.unknown, 1);
        assert_eq!(incoming.total, 8);
        assert_eq!(incoming.existing_files, 7);
        assert_eq!(incoming.retry_wait, 1);
        assert_eq!(incoming.missing_files, 1);
        assert_eq!(overview.totals.active_tasks_total, 4);
        assert!(overview
            .active_tasks
            .iter()
            .any(|task| task.status == "in_progress"));
        assert!(overview
            .active_tasks
            .iter()
            .any(|task| task.status == "queued"));
        assert!(overview
            .active_tasks
            .iter()
            .any(|task| task.status == "retry_wait"));
        assert!(overview
            .active_tasks
            .iter()
            .all(|task| task.status != "done"));
    }

    #[test]
    fn last_errors_and_recent_runs_are_limited_and_latest_first() {
        let (_tempdir, database_path) = setup_database(vec![stage("incoming", None)]);
        let connection = open_connection(&database_path).expect("open");
        seed_state(&connection, "entity-1", "incoming", "failed", true, None);
        for index in 0..25 {
            insert_app_event(
                &connection,
                if index % 2 == 0 {
                    AppEventLevel::Error
                } else {
                    AppEventLevel::Warning
                },
                "dashboard_test_event",
                &format!("Event {index}"),
                Some(json!({"entity_id": "entity-1", "stage_id": "incoming", "run_id": format!("run-{index}")})),
                &(Utc::now() + Duration::seconds(index)).to_rfc3339(),
            )
            .expect("event");
        }
        for index in 0..25 {
            connection
                .execute(
                    r#"
                    INSERT INTO stage_runs (
                        run_id, entity_id, entity_file_id, stage_id, attempt_no, workflow_url,
                        request_json, response_json, http_status, success, error_type, error_message,
                        started_at, finished_at, duration_ms
                    )
                    VALUES (?1, 'entity-1', NULL, 'incoming', 1, 'http://localhost/mock', '{}', '{}',
                        ?2, ?3, ?4, ?5, ?6, ?6, ?7)
                    "#,
                    params![
                        format!("run-{index}"),
                        if index % 2 == 0 { 200 } else { 500 },
                        if index % 2 == 0 { 1 } else { 0 },
                        if index % 2 == 0 { Option::<String>::None } else { Some("http_status".to_string()) },
                        if index % 2 == 0 { Option::<String>::None } else { Some("failed".to_string()) },
                        (Utc::now() + Duration::seconds(index)).to_rfc3339(),
                        20 + index
                    ],
                )
                .expect("run");
        }
        drop(connection);

        let overview = overview(&database_path);

        assert_eq!(overview.last_errors.len(), ERROR_LIMIT as usize);
        assert_eq!(overview.recent_runs.len(), RUN_LIMIT as usize);
        assert!(overview.last_errors[0].created_at >= overview.last_errors[1].created_at);
        assert!(overview.recent_runs[0].started_at >= overview.recent_runs[1].started_at);
        assert_eq!(
            overview.last_errors[0].entity_id.as_deref(),
            Some("entity-1")
        );
        assert!(overview
            .recent_runs
            .iter()
            .any(|run| !run.success && run.error_type.as_deref() == Some("http_status")));
    }

    #[test]
    fn dashboard_overview_is_read_only_for_execution_state() {
        let (_tempdir, database_path) = setup_database(vec![stage("incoming", None)]);
        let connection = open_connection(&database_path).expect("open");
        seed_state(&connection, "entity-1", "incoming", "pending", true, None);
        set_setting(
            &connection,
            "last_scan_completed_at",
            "2026-04-25T00:00:00Z",
            "2026-04-25T00:00:00Z",
        )
        .expect("setting");
        drop(connection);

        let before = open_connection(&database_path)
            .expect("open before")
            .query_row(
                "SELECT status FROM entity_stage_states WHERE entity_id = 'entity-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("before status");
        let overview = overview(&database_path);
        let after_connection = open_connection(&database_path).expect("open after");
        let after = after_connection
            .query_row(
                "SELECT status FROM entity_stage_states WHERE entity_id = 'entity-1'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("after status");
        let run_count: i64 = after_connection
            .query_row("SELECT COUNT(*) FROM stage_runs", [], |row| row.get(0))
            .expect("run count");

        assert_eq!(before, "pending");
        assert_eq!(after, "pending");
        assert_eq!(run_count, 0);
        assert_eq!(
            overview.runtime.last_scan_at.as_deref(),
            Some("2026-04-25T00:00:00Z")
        );
    }
}

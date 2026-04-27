use std::path::Path;
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::database::{
    block_stage_state, claim_eligible_runtime_tasks, claim_specific_runtime_task,
    find_latest_entity_file_for_stage, finish_stage_run, insert_app_event, insert_stage_run,
    open_connection, release_queued_claim, update_stage_state_failure,
    update_stage_state_failure_with_reason, update_stage_state_for_run_start,
    update_stage_state_success, FinishStageRunInput, NewStageRunInput, RuntimeTaskRecord,
};
use crate::domain::{AppEventLevel, CommandErrorInfo, RunDueTasksSummary, StageStatus};
use crate::file_ops;
use crate::file_safety::read_stable_file;
use crate::state_machine::RuntimeTransitionReason;

pub fn run_due_tasks(
    workdir_path: &Path,
    database_path: &Path,
    max_tasks: u64,
    request_timeout_sec: u64,
    stuck_task_timeout_sec: u64,
    file_stability_delay_ms: u64,
) -> Result<RunDueTasksSummary, String> {
    let mut connection = open_connection(database_path)?;
    let stuck_reconciled = reconcile_stuck_tasks_with_connection(
        &connection,
        stuck_task_timeout_sec,
        &Utc::now().to_rfc3339(),
    )?;
    let tasks =
        claim_eligible_runtime_tasks(&mut connection, &Utc::now().to_rfc3339(), max_tasks.max(1))?;
    drop(connection);

    let mut summary = RunDueTasksSummary {
        claimed: tasks.len() as u64,
        stuck_reconciled,
        ..RunDueTasksSummary::default()
    };

    for task in tasks {
        let outcome = execute_task(
            workdir_path,
            database_path,
            task,
            request_timeout_sec,
            file_stability_delay_ms,
        );
        match outcome {
            Ok(TaskOutcome::Succeeded) => summary.succeeded += 1,
            Ok(TaskOutcome::RetryScheduled) => summary.retry_scheduled += 1,
            Ok(TaskOutcome::Failed) => summary.failed += 1,
            Ok(TaskOutcome::Blocked) => summary.blocked += 1,
            Ok(TaskOutcome::Skipped) => summary.skipped += 1,
            Err(message) => {
                summary.errors.push(CommandErrorInfo {
                    code: "task_execution_failed".to_string(),
                    message,
                    path: None,
                });
            }
        }
    }

    Ok(summary)
}

pub fn run_entity_stage(
    workdir_path: &Path,
    database_path: &Path,
    entity_id: &str,
    stage_id: &str,
    request_timeout_sec: u64,
    stuck_task_timeout_sec: u64,
    file_stability_delay_ms: u64,
) -> Result<RunDueTasksSummary, String> {
    let mut connection = open_connection(database_path)?;
    let stuck_reconciled = reconcile_stuck_tasks_with_connection(
        &connection,
        stuck_task_timeout_sec,
        &Utc::now().to_rfc3339(),
    )?;
    // Manual debug execution intentionally allows retry_wait even when next_retry_at is in the future,
    // but it still uses the same queued claim so active queued/in_progress work cannot be launched twice.
    let Some(task) = claim_specific_runtime_task(
        &mut connection,
        entity_id,
        stage_id,
        &Utc::now().to_rfc3339(),
    )?
    else {
        return Ok(RunDueTasksSummary {
            skipped: 1,
            stuck_reconciled,
            ..RunDueTasksSummary::default()
        });
    };
    drop(connection);

    let mut summary = RunDueTasksSummary {
        claimed: 1,
        stuck_reconciled,
        ..RunDueTasksSummary::default()
    };
    match execute_task(
        workdir_path,
        database_path,
        task,
        request_timeout_sec,
        file_stability_delay_ms,
    ) {
        Ok(TaskOutcome::Succeeded) => summary.succeeded = 1,
        Ok(TaskOutcome::RetryScheduled) => summary.retry_scheduled = 1,
        Ok(TaskOutcome::Failed) => summary.failed = 1,
        Ok(TaskOutcome::Blocked) => summary.blocked = 1,
        Ok(TaskOutcome::Skipped) => summary.skipped = 1,
        Err(message) => summary.errors.push(CommandErrorInfo {
            code: "task_execution_failed".to_string(),
            message,
            path: None,
        }),
    }
    Ok(summary)
}

pub fn reconcile_stuck_tasks(
    database_path: &Path,
    stuck_task_timeout_sec: u64,
) -> Result<u64, String> {
    let connection = open_connection(database_path)?;
    reconcile_stuck_tasks_with_connection(
        &connection,
        stuck_task_timeout_sec,
        &Utc::now().to_rfc3339(),
    )
}

enum TaskOutcome {
    Succeeded,
    RetryScheduled,
    Failed,
    Blocked,
    Skipped,
}

struct AttemptFailure {
    error_type: String,
    message: String,
    http_status: Option<i64>,
    response_json: Option<String>,
}

fn execute_task(
    workdir_path: &Path,
    database_path: &Path,
    task: RuntimeTaskRecord,
    request_timeout_sec: u64,
    file_stability_delay_ms: u64,
) -> Result<TaskOutcome, String> {
    if task.status != "queued" {
        return Ok(TaskOutcome::Skipped);
    }
    if !task.file_exists || task.file_instance_id <= 0 {
        return block_task(
            database_path,
            &task,
            &format!("Source file instance '{}' is missing.", task.file_path),
        );
    }
    if task.workflow_url.trim().is_empty() {
        return block_task(database_path, &task, "Stage workflow_url is empty.");
    }

    let connection = open_connection(database_path)?;
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "task_queued",
        &format!(
            "Queued entity '{}' on stage '{}'.",
            task.entity_id, task.stage_id
        ),
        Some(json!({"entity_id": task.entity_id, "stage_id": task.stage_id})),
        &Utc::now().to_rfc3339(),
    )?;

    let source_file =
        find_latest_entity_file_for_stage(&connection, &task.entity_id, &task.stage_id)?
            .ok_or_else(|| {
                format!(
                    "No source entity file exists for entity '{}' on stage '{}'.",
                    task.entity_id, task.stage_id
                )
            })?;
    if let Some(message) =
        source_file_preflight_error(workdir_path, &source_file, file_stability_delay_ms)
    {
        let now = Utc::now().to_rfc3339();
        release_queued_claim(&connection, task.state_id, &now)?;
        insert_app_event(
            &connection,
            AppEventLevel::Warning,
            if message.contains("changed") {
                "source_file_changed_before_execution"
            } else {
                "source_file_unstable_before_execution"
            },
            &message,
            Some(json!({
                "entity_id": task.entity_id,
                "stage_id": task.stage_id,
                "file_path": source_file.file_path,
                "file_id": source_file.id,
            })),
            &now,
        )?;
        return Ok(TaskOutcome::Skipped);
    }
    let attempt_no = task.attempts + 1;
    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let started_at_text = started_at.to_rfc3339();
    let request_json = build_request_json(&task, &source_file, attempt_no, &run_id)?;
    insert_stage_run(
        &connection,
        &NewStageRunInput {
            run_id: run_id.clone(),
            entity_id: task.entity_id.clone(),
            entity_file_id: source_file.id,
            stage_id: task.stage_id.clone(),
            attempt_no,
            workflow_url: task.workflow_url.clone(),
            request_json: serde_json::to_string(&request_json)
                .map_err(|error| format!("Failed to serialize n8n request JSON: {error}"))?,
            started_at: started_at_text.clone(),
        },
    )?;
    update_stage_state_for_run_start(&connection, task.state_id, attempt_no, &started_at_text)?;
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "task_started",
        &format!(
            "Started entity '{}' on stage '{}'.",
            task.entity_id, task.stage_id
        ),
        Some(json!({"entity_id": task.entity_id, "stage_id": task.stage_id, "run_id": run_id})),
        &started_at_text,
    )?;
    drop(connection);

    let timer = Instant::now();
    let http_result = call_webhook(&task.workflow_url, &request_json, request_timeout_sec)
        .and_then(|response| {
            validate_response(response, source_file.next_stage.or(task.next_stage.clone()))
        });
    let finished_at = Utc::now();
    let duration_ms = timer.elapsed().as_millis() as u64;
    let connection = open_connection(database_path)?;

    match http_result {
        Ok(success) => {
            let mut created_child_path = None;
            if success.next_stage_required {
                let copy = file_ops::create_next_stage_copy_from_response(
                    workdir_path,
                    database_path,
                    &task.entity_id,
                    &task.stage_id,
                    success.payload.unwrap_or_else(|| Value::Object(Map::new())),
                    success.meta,
                    &run_id,
                )?;
                match copy.status {
                    crate::domain::FileCopyStatus::Created
                    | crate::domain::FileCopyStatus::AlreadyExists => {
                        created_child_path = copy.target_file_path.clone();
                    }
                    crate::domain::FileCopyStatus::Blocked => {
                        finish_copy_blocked(
                            &connection,
                            &task,
                            &run_id,
                            copy.message,
                            success.http_status,
                            success.response_json,
                            finished_at,
                            duration_ms,
                        )?;
                        return Ok(TaskOutcome::Blocked);
                    }
                    crate::domain::FileCopyStatus::Failed => {
                        let outcome = finish_failure(
                            &connection,
                            &task,
                            &run_id,
                            attempt_no,
                            AttemptFailure {
                                error_type: "copy_failed".to_string(),
                                message: copy.message,
                                http_status: Some(success.http_status),
                                response_json: Some(success.response_json),
                            },
                            finished_at,
                            duration_ms,
                        )?;
                        return Ok(outcome);
                    }
                }
            }

            finish_stage_run(
                &connection,
                &FinishStageRunInput {
                    run_id: run_id.clone(),
                    response_json: Some(success.response_json),
                    http_status: Some(success.http_status),
                    success: true,
                    error_type: None,
                    error_message: None,
                    finished_at: finished_at.to_rfc3339(),
                    duration_ms,
                },
            )?;
            update_stage_state_success(
                &connection,
                task.state_id,
                Some(success.http_status),
                &finished_at.to_rfc3339(),
                created_child_path.as_deref(),
            )?;
            insert_app_event(
                &connection,
                AppEventLevel::Info,
                "task_succeeded",
                &format!(
                    "Entity '{}' succeeded on stage '{}'.",
                    task.entity_id, task.stage_id
                ),
                Some(
                    json!({"entity_id": task.entity_id, "stage_id": task.stage_id, "run_id": run_id}),
                ),
                &finished_at.to_rfc3339(),
            )?;
            Ok(TaskOutcome::Succeeded)
        }
        Err(failure) => finish_failure(
            &connection,
            &task,
            &run_id,
            attempt_no,
            failure,
            finished_at,
            duration_ms,
        ),
    }
}

fn finish_copy_blocked(
    connection: &rusqlite::Connection,
    task: &RuntimeTaskRecord,
    run_id: &str,
    message: String,
    http_status: i64,
    response_json: String,
    finished_at: DateTime<Utc>,
    duration_ms: u64,
) -> Result<(), String> {
    finish_stage_run(
        connection,
        &FinishStageRunInput {
            run_id: run_id.to_string(),
            response_json: Some(response_json),
            http_status: Some(http_status),
            success: false,
            error_type: Some("copy_blocked".to_string()),
            error_message: Some(message.clone()),
            finished_at: finished_at.to_rfc3339(),
            duration_ms,
        },
    )?;
    block_stage_state(
        connection,
        task.state_id,
        &message,
        &finished_at.to_rfc3339(),
    )?;
    insert_app_event(
        connection,
        AppEventLevel::Error,
        "task_blocked",
        &format!(
            "Entity '{}' on stage '{}' was blocked after HTTP success: {}",
            task.entity_id, task.stage_id, message
        ),
        Some(json!({
            "entity_id": task.entity_id,
            "stage_id": task.stage_id,
            "run_id": run_id,
            "error_type": "copy_blocked",
        })),
        &finished_at.to_rfc3339(),
    )?;
    Ok(())
}

fn finish_failure(
    connection: &rusqlite::Connection,
    task: &RuntimeTaskRecord,
    run_id: &str,
    attempt_no: u64,
    failure: AttemptFailure,
    finished_at: DateTime<Utc>,
    duration_ms: u64,
) -> Result<TaskOutcome, String> {
    let next_retry_at = if attempt_no < task.max_attempts {
        Some((finished_at + Duration::seconds(task.retry_delay_sec as i64)).to_rfc3339())
    } else {
        None
    };
    let next_status = if next_retry_at.is_some() {
        StageStatus::RetryWait
    } else {
        StageStatus::Failed
    };
    finish_stage_run(
        connection,
        &FinishStageRunInput {
            run_id: run_id.to_string(),
            response_json: failure.response_json.clone(),
            http_status: failure.http_status,
            success: false,
            error_type: Some(failure.error_type.clone()),
            error_message: Some(failure.message.clone()),
            finished_at: finished_at.to_rfc3339(),
            duration_ms,
        },
    )?;
    update_stage_state_failure(
        connection,
        task.state_id,
        next_status.clone(),
        &failure.message,
        failure.http_status,
        next_retry_at.as_deref(),
        &finished_at.to_rfc3339(),
    )?;
    let event_code = if matches!(next_status, StageStatus::RetryWait) {
        "task_retry_scheduled"
    } else {
        "task_failed"
    };
    insert_app_event(
        connection,
        AppEventLevel::Warning,
        event_code,
        &format!(
            "Entity '{}' on stage '{}' failed attempt {}: {}",
            task.entity_id, task.stage_id, attempt_no, failure.message
        ),
        Some(json!({
            "entity_id": task.entity_id,
            "stage_id": task.stage_id,
            "run_id": run_id,
            "error_type": failure.error_type,
            "next_retry_at": next_retry_at,
        })),
        &finished_at.to_rfc3339(),
    )?;
    Ok(if matches!(next_status, StageStatus::RetryWait) {
        TaskOutcome::RetryScheduled
    } else {
        TaskOutcome::Failed
    })
}

fn block_task(
    database_path: &Path,
    task: &RuntimeTaskRecord,
    message: &str,
) -> Result<TaskOutcome, String> {
    let connection = open_connection(database_path)?;
    let now = Utc::now().to_rfc3339();
    block_stage_state(&connection, task.state_id, message, &now)?;
    insert_app_event(
        &connection,
        AppEventLevel::Error,
        "task_blocked",
        message,
        Some(json!({"entity_id": task.entity_id, "stage_id": task.stage_id})),
        &now,
    )?;
    Ok(TaskOutcome::Blocked)
}

fn call_webhook(
    workflow_url: &str,
    request_json: &Value,
    timeout_sec: u64,
) -> Result<HttpResponse, AttemptFailure> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_sec.max(1)))
        .build()
        .map_err(|error| AttemptFailure {
            error_type: "network".to_string(),
            message: format!("Failed to build HTTP client: {error}"),
            http_status: None,
            response_json: None,
        })?;
    let response = client
        .post(workflow_url)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .json(request_json)
        .send()
        .map_err(|error| AttemptFailure {
            error_type: if error.is_timeout() {
                "timeout"
            } else {
                "network"
            }
            .to_string(),
            message: error.to_string(),
            http_status: None,
            response_json: None,
        })?;
    let status = response.status().as_u16() as i64;
    let body = response.text().map_err(|error| AttemptFailure {
        error_type: "network".to_string(),
        message: format!("Failed to read HTTP response body: {error}"),
        http_status: Some(status),
        response_json: None,
    })?;
    Ok(HttpResponse { status, body })
}

struct HttpResponse {
    status: i64,
    body: String,
}

struct SuccessfulResponse {
    http_status: i64,
    response_json: String,
    payload: Option<Value>,
    meta: Option<Value>,
    next_stage_required: bool,
}

fn validate_response(
    response: HttpResponse,
    resolved_next_stage: Option<String>,
) -> Result<SuccessfulResponse, AttemptFailure> {
    if !(200..=299).contains(&response.status) {
        return Err(AttemptFailure {
            error_type: "http_status".to_string(),
            message: format!("n8n webhook returned HTTP status {}.", response.status),
            http_status: Some(response.status),
            response_json: Some(response.body),
        });
    }
    let value = serde_json::from_str::<Value>(&response.body).map_err(|error| AttemptFailure {
        error_type: "invalid_json".to_string(),
        message: format!("n8n response was not valid JSON: {error}"),
        http_status: Some(response.status),
        response_json: Some(response.body.clone()),
    })?;
    let Some(root) = value.as_object() else {
        return Err(AttemptFailure {
            error_type: "contract".to_string(),
            message: "n8n response JSON must be an object.".to_string(),
            http_status: Some(response.status),
            response_json: Some(response.body),
        });
    };
    if root.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(AttemptFailure {
            error_type: "contract".to_string(),
            message: "n8n response declared success=false.".to_string(),
            http_status: Some(response.status),
            response_json: Some(response.body),
        });
    }
    let next_stage_required = resolved_next_stage.is_some();
    let payload = root.get("payload").cloned();
    if next_stage_required && !payload.as_ref().is_some_and(Value::is_object) {
        return Err(AttemptFailure {
            error_type: "contract".to_string(),
            message: "n8n response payload must be an object when next_stage exists.".to_string(),
            http_status: Some(response.status),
            response_json: Some(response.body),
        });
    }
    Ok(SuccessfulResponse {
        http_status: response.status,
        response_json: response.body,
        payload,
        meta: root.get("meta").cloned(),
        next_stage_required,
    })
}

fn build_request_json(
    task: &RuntimeTaskRecord,
    source_file: &crate::domain::EntityFileRecord,
    attempt_no: u64,
    run_id: &str,
) -> Result<Value, String> {
    let payload = serde_json::from_str::<Value>(&source_file.payload_json)
        .map_err(|error| format!("Failed to parse source payload JSON from DB: {error}"))?;
    let mut meta = serde_json::from_str::<Value>(&source_file.meta_json)
        .map_err(|error| format!("Failed to parse source meta JSON from DB: {error}"))?
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut beehive = meta
        .remove("beehive")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    beehive.insert("app".to_string(), Value::String("beehive".to_string()));
    beehive.insert("stage_id".to_string(), Value::String(task.stage_id.clone()));
    beehive.insert(
        "entity_file_id".to_string(),
        Value::Number(source_file.id.into()),
    );
    beehive.insert("attempt".to_string(), Value::Number(attempt_no.into()));
    beehive.insert("run_id".to_string(), Value::String(run_id.to_string()));
    meta.insert("beehive".to_string(), Value::Object(beehive));

    Ok(json!({
        "entity_id": task.entity_id,
        "stage_id": task.stage_id,
        "entity_file_id": source_file.id,
        "source_file_path": source_file.file_path,
        "attempt": attempt_no,
        "run_id": run_id,
        "payload": payload,
        "meta": meta,
    }))
}

fn source_file_preflight_error(
    workdir_path: &Path,
    source_file: &crate::domain::EntityFileRecord,
    file_stability_delay_ms: u64,
) -> Option<String> {
    let stored_path = Path::new(&source_file.file_path);
    let resolved_path = if stored_path.is_absolute() {
        stored_path.to_path_buf()
    } else {
        workdir_path.join(stored_path)
    };
    let stable_read = match read_stable_file(&resolved_path, file_stability_delay_ms) {
        Ok(read) => read,
        Err(issue) => return Some(issue.message),
    };
    let checksum = format!("{:x}", Sha256::digest(&stable_read.bytes));
    if checksum != source_file.checksum
        || stable_read.file_size != source_file.file_size
        || stable_read.file_mtime != source_file.file_mtime
    {
        return Some(format!(
            "Source file '{}' changed after the last scan; run Scan workspace before execution.",
            resolved_path.display()
        ));
    }
    None
}

fn reconcile_stuck_tasks_with_connection(
    connection: &rusqlite::Connection,
    stuck_task_timeout_sec: u64,
    now: &str,
) -> Result<u64, String> {
    let cutoff = (Utc::now() - Duration::seconds(stuck_task_timeout_sec as i64)).to_rfc3339();
    let stale_queued = load_stale_queued_state_ids(connection, &cutoff)?;
    for state_id in &stale_queued {
        release_queued_claim(connection, *state_id, now)?;
        insert_app_event(
            connection,
            AppEventLevel::Warning,
            "queued_claim_released",
            "A stale queued task claim was released before execution started.",
            Some(json!({"state_id": state_id})),
            now,
        )?;
    }

    let mut statement = connection
        .prepare(
            r#"
            SELECT id, attempts, max_attempts
            FROM entity_stage_states
            WHERE status = 'in_progress'
              AND last_started_at IS NOT NULL
              AND last_started_at < ?1
            "#,
        )
        .map_err(|error| format!("Failed to prepare stuck task query: {error}"))?;
    let rows = statement
        .query_map(rusqlite::params![cutoff], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, i64>(2)? as u64,
            ))
        })
        .map_err(|error| format!("Failed to query stuck tasks: {error}"))?;
    let mut states = Vec::new();
    for row in rows {
        states.push(row.map_err(|error| format!("Failed to read stuck task row: {error}"))?);
    }
    drop(statement);

    for (state_id, attempts, max_attempts) in &states {
        let next_status = if attempts < max_attempts {
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
            *state_id,
            next_status,
            "Task was reconciled after being stuck in progress.",
            None,
            next_retry_at,
            now,
            RuntimeTransitionReason::StuckReconciliation,
        )?;
        insert_app_event(
            connection,
            AppEventLevel::Warning,
            "stuck_task_reconciled",
            "A stuck in_progress task was reconciled.",
            Some(json!({"state_id": state_id})),
            now,
        )?;
    }

    Ok((states.len() + stale_queued.len()) as u64)
}

fn load_stale_queued_state_ids(
    connection: &rusqlite::Connection,
    cutoff: &str,
) -> Result<Vec<i64>, String> {
    let mut statement = connection
        .prepare(
            r#"
            SELECT id
            FROM entity_stage_states
            WHERE status = 'queued'
              AND updated_at < ?1
            ORDER BY updated_at ASC, id ASC
            "#,
        )
        .map_err(|error| format!("Failed to prepare stale queued task query: {error}"))?;
    let rows = statement
        .query_map(rusqlite::params![cutoff], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("Failed to query stale queued tasks: {error}"))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|error| format!("Failed to read stale queued task row: {error}"))?);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use crate::database::{
        bootstrap_database, claim_eligible_runtime_tasks, get_entity_detail, list_stage_runs,
        open_connection,
    };
    use crate::discovery::scan_workspace;
    use crate::domain::{PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition};
    use sha2::Digest;

    struct MockServer {
        url: String,
        requests: Arc<Mutex<Vec<String>>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn mock_server(responses: Vec<(u16, &'static str)>) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let url = format!(
            "http://{}/webhook/test",
            listener.local_addr().expect("addr")
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut buffer = [0_u8; 8192];
                let read = stream.read(&mut buffer).expect("read request");
                captured
                    .lock()
                    .expect("lock requests")
                    .push(String::from_utf8_lossy(&buffer[..read]).to_string());
                let status_text = if (200..=299).contains(&status) {
                    "OK"
                } else {
                    "ERROR"
                };
                let response = format!(
                    "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write response");
            }
        });
        MockServer {
            url,
            requests,
            handle: Some(handle),
        }
    }

    fn test_config(workflow_url: &str, max_attempts: u64, retry_delay_sec: u64) -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            runtime: RuntimeConfig {
                scan_interval_sec: 5,
                max_parallel_tasks: 3,
                stuck_task_timeout_sec: 1,
                request_timeout_sec: 5,
                file_stability_delay_ms: 0,
            },
            stages: vec![
                StageDefinition {
                    id: "incoming".to_string(),
                    input_folder: "stages/incoming".to_string(),
                    output_folder: "stages/incoming-out".to_string(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts,
                    retry_delay_sec,
                    next_stage: Some("normalized".to_string()),
                },
                StageDefinition {
                    id: "normalized".to_string(),
                    input_folder: "stages/normalized".to_string(),
                    output_folder: "stages/normalized-out".to_string(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts,
                    retry_delay_sec,
                    next_stage: None,
                },
            ],
        }
    }

    fn stage(
        id: &str,
        workflow_url: &str,
        next_stage: Option<&str>,
        active_input: &str,
    ) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: active_input.to_string(),
            output_folder: format!("{active_input}-out"),
            workflow_url: workflow_url.to_string(),
            max_attempts: 3,
            retry_delay_sec: 0,
            next_stage: next_stage.map(ToOwned::to_owned),
        }
    }

    fn prepare_workdir(
        workflow_url: &str,
        max_attempts: u64,
        retry_delay_sec: u64,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        bootstrap_database(
            &database_path,
            &test_config(workflow_url, max_attempts, retry_delay_sec),
        )
        .expect("bootstrap");
        std::fs::create_dir_all(source_path.parent().expect("source parent"))
            .expect("create source parent");
        std::fs::write(
            &source_path,
            r#"{"id":"entity-1","current_stage":"incoming","next_stage":"normalized","status":"pending","payload":{"title":"hello beehive"},"meta":{"source":"manual"}}"#,
        )
        .expect("write source");
        scan_workspace(&workdir, &database_path).expect("scan");
        (tempdir, workdir, database_path, source_path)
    }

    fn stage_status(database_path: &std::path::Path, stage_id: &str) -> String {
        let detail = get_entity_detail(database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");
        detail
            .stage_states
            .into_iter()
            .find(|state| state.stage_id == stage_id)
            .expect("state exists")
            .status
    }

    fn stage_state(
        database_path: &std::path::Path,
        stage_id: &str,
    ) -> crate::domain::EntityStageStateRecord {
        let detail = get_entity_detail(database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");
        detail
            .stage_states
            .into_iter()
            .find(|state| state.stage_id == stage_id)
            .expect("state exists")
    }

    #[test]
    fn successful_n8n_execution_marks_done_and_creates_next_stage_file_from_response() {
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"entity_id":"entity-1","stage_id":"incoming","payload":{"title":"hello beehive","title_processed":"HELLO BEEHIVE"},"meta":{"n8n":{"workflow":"mock"}}}"#,
        )]);
        let (_tempdir, workdir, database_path, source_path) = prepare_workdir(&server.url, 3, 0);
        let source_before = std::fs::read(&source_path).expect("read source before");

        let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("run tasks");
        let detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");
        let target_path = workdir
            .join("stages")
            .join("normalized")
            .join("entity-1.json");
        let target_json =
            serde_json::from_slice::<Value>(&std::fs::read(&target_path).expect("read target"))
                .expect("parse target");
        let target_file = detail
            .files
            .iter()
            .find(|file| file.stage_id == "normalized")
            .expect("target db file");
        let target_checksum = format!(
            "{:x}",
            sha2::Sha256::digest(&std::fs::read(&target_path).expect("target bytes"))
        );

        assert_eq!(summary.claimed, 1);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "done");
        assert_eq!(stage_status(&database_path, "normalized"), "pending");
        assert_eq!(
            target_json
                .get("payload")
                .and_then(|payload| payload.get("title_processed"))
                .and_then(Value::as_str),
            Some("HELLO BEEHIVE")
        );
        assert_eq!(
            target_json
                .get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("beehive"))
                .and_then(Value::as_object)
                .and_then(|beehive| beehive.get("created_by"))
                .and_then(Value::as_str),
            Some("stage4_n8n_execution")
        );
        assert_eq!(
            std::fs::read(&source_path).expect("read source after"),
            source_before
        );
        assert_eq!(runs.len(), 1);
        assert!(runs[0].success);
        assert_eq!(runs[0].attempt_no, 1);
        assert_eq!(runs[0].http_status, Some(200));
        assert!(runs[0].request_json.contains("\"entity_file_id\""));
        assert_eq!(target_file.checksum, target_checksum);
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
    }

    #[test]
    fn success_then_scan_does_not_regress_done_or_send_second_http_request() {
        let server = mock_server(vec![(200, r#"{"success":true,"meta":{}}"#)]);
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        bootstrap_database(
            &database_path,
            &PipelineConfig {
                project: ProjectConfig {
                    name: "beehive".to_string(),
                    workdir: ".".to_string(),
                },
                runtime: RuntimeConfig::default(),
                stages: vec![stage("incoming", &server.url, None, "stages/incoming")],
            },
        )
        .expect("bootstrap");
        std::fs::create_dir_all(source_path.parent().expect("source parent")).expect("parent");
        std::fs::write(
            &source_path,
            r#"{"id":"entity-1","current_stage":"incoming","status":"pending","payload":{"title":"hello"},"meta":{}}"#,
        )
        .expect("source");
        scan_workspace(&workdir, &database_path).expect("initial scan");

        let first = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("first run");
        scan_workspace(&workdir, &database_path).expect("scan after success");
        let second = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("second run");

        assert_eq!(first.succeeded, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "done");
        assert_eq!(second.claimed, 0);
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
    }

    #[test]
    fn http_non_2xx_retries_then_fails_after_max_attempts() {
        let server = mock_server(vec![
            (500, r#"{"success":false}"#),
            (500, r#"{"success":false}"#),
        ]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 2, 0);

        let first = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("first run");
        let second = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("second run");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(first.retry_scheduled, 1);
        assert_eq!(second.failed, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "failed");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].error_type.as_deref(), Some("http_status"));
    }

    #[test]
    fn contract_errors_are_failed_attempts() {
        let server = mock_server(vec![(200, r#"{"success":true,"meta":{}}"#)]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 1, 0);

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run tasks");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.failed, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "failed");
        assert_eq!(runs[0].error_type.as_deref(), Some("contract"));
    }

    #[test]
    fn retry_wait_not_due_is_skipped_and_due_retry_executes() {
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"payload":{"title":"retry ok"},"meta":{}}"#,
        )]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let connection = crate::database::open_connection(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'retry_wait', attempts = 1, next_retry_at = ?1 WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                rusqlite::params![(Utc::now() + Duration::hours(1)).to_rfc3339()],
            )
            .expect("future retry");

        let skipped = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("skipped");
        connection
            .execute(
                "UPDATE entity_stage_states SET next_retry_at = ?1 WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                rusqlite::params![(Utc::now() - Duration::hours(1)).to_rfc3339()],
            )
            .expect("past retry");
        let due = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("due");

        assert_eq!(skipped.claimed, 0);
        assert_eq!(due.succeeded, 1);
    }

    #[test]
    fn duplicate_claim_of_same_state_is_noop_after_first_claim() {
        let server = mock_server(Vec::new());
        let (_tempdir, _workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let mut first_connection = open_connection(&database_path).expect("first connection");
        let mut second_connection = open_connection(&database_path).expect("second connection");
        let now = Utc::now().to_rfc3339();

        let first =
            claim_eligible_runtime_tasks(&mut first_connection, &now, 1).expect("first claim");
        let second =
            claim_eligible_runtime_tasks(&mut second_connection, &now, 1).expect("second claim");
        let state = stage_state(&database_path, "incoming");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
        assert_eq!(state.status, "queued");
        assert_eq!(state.attempts, 0);
        assert!(runs.is_empty());
    }

    #[test]
    fn stale_queued_claim_is_released_without_attempt_increment_or_stage_run() {
        let server = mock_server(Vec::new());
        let (_tempdir, _workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let connection = open_connection(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'queued', updated_at = ?1 WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                rusqlite::params![(Utc::now() - Duration::hours(1)).to_rfc3339()],
            )
            .expect("stale queued");

        let reconciled = reconcile_stuck_tasks(&database_path, 1).expect("reconcile");
        let state = stage_state(&database_path, "incoming");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(reconciled, 1);
        assert_eq!(state.status, "pending");
        assert_eq!(state.attempts, 0);
        assert!(runs.is_empty());
    }

    #[test]
    fn stuck_retry_wait_gets_due_next_retry_at_and_can_execute() {
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"payload":{"title":"recovered"},"meta":{}}"#,
        )]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let connection = crate::database::open_connection(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'in_progress', attempts = 1, last_started_at = ?1 WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                rusqlite::params![(Utc::now() - Duration::hours(1)).to_rfc3339()],
            )
            .expect("stuck state");

        let reconciled = reconcile_stuck_tasks(&database_path, 1).expect("reconcile stuck");
        let state_after_reconcile = stage_state(&database_path, "incoming");
        let next_retry_at = state_after_reconcile
            .next_retry_at
            .as_deref()
            .expect("next_retry_at should be set");
        let due_at = chrono::DateTime::parse_from_rfc3339(next_retry_at)
            .expect("parse next_retry_at")
            .with_timezone(&Utc);
        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run tasks");

        assert_eq!(reconciled, 1);
        assert_eq!(state_after_reconcile.status, "retry_wait");
        assert!(due_at <= Utc::now());
        assert_eq!(summary.succeeded, 1);
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
    }

    #[test]
    fn done_state_is_not_executed_again() {
        let server = mock_server(Vec::new());
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let connection = crate::database::open_connection(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'done' WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                [],
            )
            .expect("done state");

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run tasks");

        assert_eq!(summary.claimed, 0);
        assert_eq!(server.requests.lock().expect("requests").len(), 0);
    }

    #[test]
    fn source_file_changed_after_scan_is_not_sent_or_counted_as_attempt() {
        let server = mock_server(Vec::new());
        let (_tempdir, workdir, database_path, source_path) = prepare_workdir(&server.url, 3, 0);
        std::fs::write(
            &source_path,
            r#"{"id":"entity-1","current_stage":"incoming","next_stage":"normalized","status":"pending","payload":{"title":"changed"},"meta":{}}"#,
        )
        .expect("mutate source after scan");

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run tasks");
        let state = stage_state(&database_path, "incoming");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.claimed, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(state.status, "pending");
        assert_eq!(state.attempts, 0);
        assert!(runs.is_empty());
        assert_eq!(server.requests.lock().expect("requests").len(), 0);
    }

    #[test]
    fn blocked_missing_next_stage_after_http_does_not_retry() {
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"payload":{"title":"blocked"},"meta":{}}"#,
        )]);
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        let config = PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            runtime: RuntimeConfig::default(),
            stages: vec![stage(
                "incoming",
                &server.url,
                Some("missing"),
                "stages/incoming",
            )],
        };
        bootstrap_database(&database_path, &config).expect("bootstrap");
        std::fs::create_dir_all(source_path.parent().expect("source parent")).expect("parent");
        std::fs::write(
            &source_path,
            r#"{"id":"entity-1","current_stage":"incoming","next_stage":"missing","status":"pending","payload":{"title":"hello"},"meta":{}}"#,
        )
        .expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");
        let state = stage_state(&database_path, "incoming");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.blocked, 1);
        assert_eq!(state.status, "blocked");
        assert!(state.next_retry_at.is_none());
        assert!(!runs[0].success);
        assert_eq!(runs[0].error_type.as_deref(), Some("copy_blocked"));
    }

    #[test]
    fn blocked_inactive_next_stage_after_http_does_not_retry() {
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"payload":{"title":"blocked"},"meta":{}}"#,
        )]);
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        let config = PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            runtime: RuntimeConfig::default(),
            stages: vec![
                stage(
                    "incoming",
                    &server.url,
                    Some("normalized"),
                    "stages/incoming",
                ),
                stage("normalized", &server.url, None, "stages/normalized"),
            ],
        };
        bootstrap_database(&database_path, &config).expect("bootstrap");
        bootstrap_database(
            &database_path,
            &PipelineConfig {
                stages: vec![stage(
                    "incoming",
                    &server.url,
                    Some("normalized"),
                    "stages/incoming",
                )],
                ..config.clone()
            },
        )
        .expect("archive normalized");
        std::fs::create_dir_all(source_path.parent().expect("source parent")).expect("parent");
        std::fs::write(
            &source_path,
            r#"{"id":"entity-1","current_stage":"incoming","next_stage":"normalized","status":"pending","payload":{"title":"hello"},"meta":{}}"#,
        )
        .expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");
        let state = stage_state(&database_path, "incoming");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.blocked, 1);
        assert_eq!(state.status, "blocked");
        assert!(state.next_retry_at.is_none());
        assert!(!runs[0].success);
        assert_eq!(runs[0].error_type.as_deref(), Some("copy_blocked"));
    }

    #[test]
    fn run_entity_stage_can_bypass_retry_delay_for_debugging() {
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"payload":{"title":"debug run"},"meta":{}}"#,
        )]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let connection = crate::database::open_connection(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'retry_wait', attempts = 1, next_retry_at = ?1 WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                rusqlite::params![(Utc::now() + Duration::hours(1)).to_rfc3339()],
            )
            .expect("future retry");

        let due_summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("due run");
        let debug_summary =
            run_entity_stage(&workdir, &database_path, "entity-1", "incoming", 5, 1, 0)
                .expect("debug run");

        assert_eq!(due_summary.claimed, 0);
        assert_eq!(debug_summary.succeeded, 1);
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
    }

    #[test]
    fn run_entity_stage_refuses_active_queued_state() {
        let server = mock_server(Vec::new());
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let connection = open_connection(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'queued', updated_at = ?1 WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                rusqlite::params![Utc::now().to_rfc3339()],
            )
            .expect("queued state");

        let summary = run_entity_stage(&workdir, &database_path, "entity-1", "incoming", 5, 1, 0)
            .expect("debug run");

        assert_eq!(summary.claimed, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(server.requests.lock().expect("requests").len(), 0);
    }

    #[test]
    fn terminal_stage_without_output_folder_succeeds_without_target_copy() {
        let server = mock_server(vec![(200, r#"{"success":true,"meta":{}}"#)]);
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        bootstrap_database(
            &database_path,
            &PipelineConfig {
                project: ProjectConfig {
                    name: "beehive".to_string(),
                    workdir: ".".to_string(),
                },
                runtime: RuntimeConfig::default(),
                stages: vec![StageDefinition {
                    id: "incoming".to_string(),
                    input_folder: "stages/incoming".to_string(),
                    output_folder: String::new(),
                    workflow_url: server.url.clone(),
                    max_attempts: 3,
                    retry_delay_sec: 0,
                    next_stage: None,
                }],
            },
        )
        .expect("bootstrap");
        std::fs::create_dir_all(source_path.parent().expect("source parent")).expect("parent");
        std::fs::write(
            &source_path,
            r#"{"id":"entity-1","current_stage":"incoming","status":"pending","payload":{"title":"terminal"},"meta":{}}"#,
        )
        .expect("source");
        scan_workspace(&workdir, &database_path).expect("scan");

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");
        let state = stage_state(&database_path, "incoming");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(state.status, "done");
        assert!(state.created_child_path.is_none());
        assert_eq!(runs.len(), 1);
        assert!(runs[0].success);
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
    }
}

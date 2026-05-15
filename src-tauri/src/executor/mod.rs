use std::path::Path;
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::database::{
    block_stage_state, claim_eligible_runtime_tasks, claim_specific_runtime_task,
    find_latest_entity_file_for_stage, find_stage_by_id, finish_stage_run, insert_app_event,
    load_active_stages_from_connection, load_setting, open_connection,
    reconcile_orphan_stage_runs_for_queued_state, register_s3_artifact_pointers,
    release_queued_claim, start_claimed_stage_run, update_stage_state_failure,
    update_stage_state_failure_with_reason, update_stage_state_success, FinishStageRunInput,
    NewStageRunInput, RegisterS3ArtifactPointerInput, RuntimeTaskRecord,
};
use crate::domain::{
    AppEventLevel, ArtifactLocation, CommandErrorInfo, EntityFileRecord, PipelineWaveSummary,
    RunDueTasksSummary, RunPipelineWavesSummary, S3StorageConfig, StageRecord, StageStatus,
    StorageProvider, DEFAULT_REQUEST_TIMEOUT_SEC,
};
use crate::file_ops;
use crate::file_safety::read_stable_file;
use crate::s3_control_envelope::{S3ControlEnvelope, S3ControlEnvelopeParts};
use crate::s3_manifest::{
    parse_and_validate_s3_manifest, S3ManifestStatus, S3ManifestValidationContext,
    S3ManifestValidationErrorKind,
};
use crate::save_path::parse_s3_uri;
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

pub fn run_pipeline_waves(
    workdir_path: &Path,
    database_path: &Path,
    max_waves: u64,
    max_tasks_per_wave: u64,
    stop_on_first_failure: bool,
    request_timeout_sec: u64,
    stuck_task_timeout_sec: u64,
    file_stability_delay_ms: u64,
) -> Result<RunPipelineWavesSummary, String> {
    let limited_max_waves = max_waves.clamp(1, 10);
    let limited_tasks_per_wave = max_tasks_per_wave.clamp(1, 5);
    let mut aggregate = RunPipelineWavesSummary {
        requested_max_waves: max_waves,
        requested_max_tasks_per_wave: max_tasks_per_wave,
        max_waves: limited_max_waves,
        max_tasks_per_wave: limited_tasks_per_wave,
        max_total_tasks: limited_max_waves * limited_tasks_per_wave,
        stop_on_first_failure,
        waves_executed: 0,
        total_claimed: 0,
        total_succeeded: 0,
        total_retry_scheduled: 0,
        total_failed: 0,
        total_blocked: 0,
        total_skipped: 0,
        total_stuck_reconciled: 0,
        total_errors: 0,
        stopped_reason: "max_waves_reached".to_string(),
        wave_summaries: Vec::new(),
        errors: Vec::new(),
    };

    for wave_index in 1..=limited_max_waves {
        let summary = match run_due_tasks(
            workdir_path,
            database_path,
            limited_tasks_per_wave,
            request_timeout_sec,
            stuck_task_timeout_sec,
            file_stability_delay_ms,
        ) {
            Ok(summary) => summary,
            Err(message) => {
                aggregate.stopped_reason = "runtime_error".to_string();
                aggregate.errors.push(CommandErrorInfo {
                    code: "pipeline_wave_runtime_error".to_string(),
                    message,
                    path: None,
                });
                return Ok(aggregate);
            }
        };

        let wave_claimed = summary.claimed;
        let wave_failed_or_blocked =
            summary.failed > 0 || summary.blocked > 0 || !summary.errors.is_empty();
        aggregate.waves_executed += 1;
        aggregate.total_claimed += summary.claimed;
        aggregate.total_succeeded += summary.succeeded;
        aggregate.total_retry_scheduled += summary.retry_scheduled;
        aggregate.total_failed += summary.failed;
        aggregate.total_blocked += summary.blocked;
        aggregate.total_skipped += summary.skipped;
        aggregate.total_stuck_reconciled += summary.stuck_reconciled;
        aggregate.total_errors += summary.errors.len() as u64;
        aggregate.wave_summaries.push(PipelineWaveSummary {
            wave_index,
            summary,
        });

        if wave_claimed == 0 {
            aggregate.stopped_reason = "idle".to_string();
            break;
        }
        if stop_on_first_failure && wave_failed_or_blocked {
            aggregate.stopped_reason = "failure_or_blocked".to_string();
            break;
        }
    }

    Ok(aggregate)
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

    let mut connection = open_connection(database_path)?;
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
    if source_file.storage_provider == StorageProvider::S3 {
        drop(connection);
        return execute_s3_task(database_path, task, source_file, request_timeout_sec);
    }
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
    let request_json = build_request_json(&source_file)?;
    let stage_run_input = NewStageRunInput {
        run_id: run_id.clone(),
        entity_id: task.entity_id.clone(),
        entity_file_id: source_file.id,
        stage_id: task.stage_id.clone(),
        attempt_no,
        workflow_url: task.workflow_url.clone(),
        request_json: serde_json::to_string(&request_json)
            .map_err(|error| format!("Failed to serialize n8n request JSON: {error}"))?,
        started_at: started_at_text.clone(),
    };
    start_claimed_stage_run(&mut connection, task.state_id, &stage_run_input)?;
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
            let mut created_child_paths = Vec::new();
            let output_count = success.output_payloads.len();
            if !success.output_payloads.is_empty() {
                let copy = match file_ops::create_next_stage_copies_from_response(
                    workdir_path,
                    database_path,
                    &task.entity_id,
                    &task.stage_id,
                    &success.output_payloads,
                    success.meta,
                    &run_id,
                ) {
                    Ok(copy) => copy,
                    Err(message) => {
                        let outcome = finish_failure(
                            &connection,
                            &task,
                            &run_id,
                            attempt_no,
                            AttemptFailure {
                                error_type: "copy_failed".to_string(),
                                message,
                                http_status: Some(success.http_status),
                                response_json: Some(success.response_json),
                            },
                            finished_at,
                            duration_ms,
                        )?;
                        return Ok(outcome);
                    }
                };
                match copy.status {
                    crate::domain::FileCopyStatus::Created
                    | crate::domain::FileCopyStatus::AlreadyExists => {
                        created_child_path = copy.target_file_paths.first().cloned();
                        created_child_paths = copy.target_file_paths.clone();
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
                Some(json!({
                    "entity_id": task.entity_id,
                    "stage_id": task.stage_id,
                    "run_id": run_id,
                    "output_count": output_count,
                    "created_child_paths": created_child_paths,
                })),
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

fn execute_s3_task(
    database_path: &Path,
    task: RuntimeTaskRecord,
    source_file: EntityFileRecord,
    request_timeout_sec: u64,
) -> Result<TaskOutcome, String> {
    let Some(source_bucket) = source_file.bucket.clone() else {
        return block_task(
            database_path,
            &task,
            "S3 source artifact is missing bucket metadata.",
        );
    };
    let Some(source_key) = source_file.key.clone() else {
        return block_task(
            database_path,
            &task,
            "S3 source artifact is missing key metadata.",
        );
    };

    let attempt_no = task.attempts + 1;
    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let started_at_text = started_at.to_rfc3339();
    let mut connection = open_connection(database_path)?;
    let workspace_id = load_setting(&connection, "project_name")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "beehive".to_string());
    let storage_bucket = load_setting(&connection, "storage_bucket")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| source_bucket.clone());
    let workspace_prefix = load_setting(&connection, "storage_workspace_prefix")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            source_key
                .split('/')
                .next()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("workspace")
                .to_string()
        });
    let storage = S3StorageConfig {
        bucket: storage_bucket,
        workspace_prefix,
        region: None,
        endpoint: None,
    };
    let manifest_prefix = format!(
        "{}/runs/{}/",
        storage.workspace_prefix.trim_end_matches('/'),
        run_id
    );
    let source_stage = find_stage_by_id(&connection, &task.stage_id)?
        .ok_or_else(|| format!("Source stage '{}' was not found.", task.stage_id))?;
    let active_stages = load_active_stages_from_connection(&connection)?;
    let Some(source_artifact_id) = source_file.artifact_id.clone() else {
        return block_task(
            database_path,
            &task,
            "S3 source artifact is missing artifact_id metadata.",
        );
    };
    let target_prefix = resolve_s3_control_target_prefix(&source_stage, &active_stages, &storage)?;
    let control_envelope = S3ControlEnvelope::from_parts(S3ControlEnvelopeParts {
        workspace_id: workspace_id.clone(),
        run_id: run_id.clone(),
        stage_id: task.stage_id.clone(),
        source_bucket: source_bucket.clone(),
        source_key: source_key.clone(),
        source_version_id: source_file.version_id.clone(),
        source_etag: source_file.etag.clone(),
        source_entity_id: source_file.entity_id.clone(),
        source_artifact_id,
        manifest_prefix: manifest_prefix.clone(),
        workspace_prefix: storage.workspace_prefix.clone(),
        target_prefix: target_prefix.clone(),
        save_path: target_prefix,
    });
    let request_json = serde_json::to_value(&control_envelope)
        .map_err(|error| format!("Failed to serialize S3 control envelope: {error}"))?;
    let stage_run_input = NewStageRunInput {
        run_id: run_id.clone(),
        entity_id: task.entity_id.clone(),
        entity_file_id: source_file.id,
        stage_id: task.stage_id.clone(),
        attempt_no,
        workflow_url: task.workflow_url.clone(),
        request_json: serde_json::to_string(&request_json)
            .map_err(|error| format!("Failed to serialize S3 n8n request audit JSON: {error}"))?,
        started_at: started_at_text.clone(),
    };
    start_claimed_stage_run(&mut connection, task.state_id, &stage_run_input)?;
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "task_started",
        &format!(
            "Started S3 artifact '{}' on stage '{}'.",
            source_file.file_path, task.stage_id
        ),
        Some(json!({
            "entity_id": task.entity_id.clone(),
            "stage_id": task.stage_id.clone(),
            "run_id": run_id.clone(),
            "source_bucket": source_file.bucket.clone(),
            "source_key": source_file.key.clone(),
        })),
        &started_at_text,
    )?;
    drop(connection);

    let timer = Instant::now();
    let http_result =
        call_s3_control_webhook(&task.workflow_url, &control_envelope, request_timeout_sec);
    let finished_at = Utc::now();
    let duration_ms = timer.elapsed().as_millis() as u64;
    let connection = open_connection(database_path)?;

    let response = match http_result {
        Ok(response) => response,
        Err(failure) => {
            return finish_failure(
                &connection,
                &task,
                &run_id,
                attempt_no,
                failure,
                finished_at,
                duration_ms,
            );
        }
    };
    if !(200..=299).contains(&response.status) {
        return finish_failure(
            &connection,
            &task,
            &run_id,
            attempt_no,
            AttemptFailure {
                error_type: "http_status".to_string(),
                message: format!("n8n webhook returned HTTP status {}.", response.status),
                http_status: Some(response.status),
                response_json: Some(response.body),
            },
            finished_at,
            duration_ms,
        );
    }

    let context = S3ManifestValidationContext {
        workspace_id: request_json["workspace_id"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        run_id: run_id.clone(),
        source_entity_id: source_file.entity_id.clone(),
        source: ArtifactLocation {
            provider: StorageProvider::S3,
            local_path: None,
            bucket: Some(control_envelope.source_bucket.clone()),
            key: Some(control_envelope.source_key.clone()),
            version_id: source_file.version_id.clone(),
            etag: source_file.etag.clone(),
            checksum_sha256: source_file.checksum_sha256.clone(),
            size: source_file.artifact_size,
        },
        storage,
        source_stage,
        active_stages,
    };
    let validated = match parse_and_validate_s3_manifest(&response.body, &context) {
        Ok(validated) => validated,
        Err(error) if error.kind == S3ManifestValidationErrorKind::BlockedRoute => {
            finish_manifest_blocked(
                &connection,
                &task,
                &run_id,
                error.message,
                response.status,
                response.body,
                finished_at,
                duration_ms,
            )?;
            return Ok(TaskOutcome::Blocked);
        }
        Err(error) => {
            return finish_failure(
                &connection,
                &task,
                &run_id,
                attempt_no,
                AttemptFailure {
                    error_type: "manifest_invalid".to_string(),
                    message: error.message,
                    http_status: Some(response.status),
                    response_json: Some(response.body),
                },
                finished_at,
                duration_ms,
            );
        }
    };

    if validated.manifest.status == S3ManifestStatus::Error {
        return finish_failure(
            &connection,
            &task,
            &run_id,
            attempt_no,
            AttemptFailure {
                error_type: validated
                    .manifest
                    .error_type
                    .clone()
                    .unwrap_or_else(|| "s3_manifest_error".to_string()),
                message: validated
                    .manifest
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "n8n returned an error manifest.".to_string()),
                http_status: Some(response.status),
                response_json: Some(response.body),
            },
            finished_at,
            duration_ms,
        );
    }

    let output_inputs = validated
        .outputs
        .iter()
        .map(|output| RegisterS3ArtifactPointerInput {
            entity_id: output.output.entity_id.clone(),
            artifact_id: output.output.artifact_id.clone(),
            relation_to_source: Some(output.output.relation_to_source.as_str().to_string()),
            stage_id: output.target_stage.id.clone(),
            bucket: output
                .location
                .bucket
                .clone()
                .unwrap_or_else(|| output.output.bucket.clone()),
            key: output
                .location
                .key
                .clone()
                .unwrap_or_else(|| output.output.key.clone()),
            version_id: output.location.version_id.clone(),
            etag: output.location.etag.clone(),
            checksum_sha256: output.location.checksum_sha256.clone(),
            size: output.location.size,
            last_modified: None,
            source_file_id: Some(source_file.id),
            producer_run_id: Some(run_id.clone()),
            status: StageStatus::Pending,
        })
        .collect::<Vec<_>>();
    let registered_files = match register_s3_artifact_pointers(database_path, &output_inputs) {
        Ok(files) => files,
        Err(message) => {
            return finish_failure(
                &connection,
                &task,
                &run_id,
                attempt_no,
                AttemptFailure {
                    error_type: "artifact_registration_failed".to_string(),
                    message,
                    http_status: Some(response.status),
                    response_json: Some(response.body),
                },
                finished_at,
                duration_ms,
            );
        }
    };
    let created_child_paths = registered_files
        .iter()
        .map(|file| file.file_path.clone())
        .collect::<Vec<_>>();

    finish_stage_run(
        &connection,
        &FinishStageRunInput {
            run_id: run_id.clone(),
            response_json: Some(response.body),
            http_status: Some(response.status),
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
        Some(response.status),
        &finished_at.to_rfc3339(),
        created_child_paths.first().map(String::as_str),
    )?;
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "task_succeeded",
        &format!(
            "S3 artifact '{}' succeeded on stage '{}'.",
            source_file.file_path, task.stage_id
        ),
        Some(json!({
            "entity_id": task.entity_id,
            "stage_id": task.stage_id,
            "run_id": run_id,
            "output_count": created_child_paths.len(),
            "created_child_paths": created_child_paths,
        })),
        &finished_at.to_rfc3339(),
    )?;
    Ok(TaskOutcome::Succeeded)
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

fn finish_manifest_blocked(
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
            error_type: Some("manifest_blocked".to_string()),
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
            "S3 entity '{}' on stage '{}' was blocked by manifest routing: {}",
            task.entity_id, task.stage_id, message
        ),
        Some(json!({
            "entity_id": task.entity_id,
            "stage_id": task.stage_id,
            "run_id": run_id,
            "error_type": "manifest_blocked",
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
        .timeout(std::time::Duration::from_secs(
            effective_webhook_timeout_sec(timeout_sec),
        ))
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

fn call_s3_control_webhook(
    workflow_url: &str,
    control_envelope: &S3ControlEnvelope,
    timeout_sec: u64,
) -> Result<HttpResponse, AttemptFailure> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(
            effective_webhook_timeout_sec(timeout_sec),
        ))
        .build()
        .map_err(|error| AttemptFailure {
            error_type: "network".to_string(),
            message: format!("Failed to build HTTP client: {error}"),
            http_status: None,
            response_json: None,
        })?;
    let response = client
        .post(workflow_url)
        .header(CONTENT_TYPE, "application/json; charset=utf-8")
        .header(ACCEPT, "application/json")
        .json(control_envelope)
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

fn effective_webhook_timeout_sec(timeout_sec: u64) -> u64 {
    timeout_sec.max(DEFAULT_REQUEST_TIMEOUT_SEC)
}

fn resolve_s3_control_target_prefix(
    source_stage: &StageRecord,
    active_stages: &[StageRecord],
    storage: &S3StorageConfig,
) -> Result<String, String> {
    if let Some(next_stage_id) = source_stage.next_stage.as_ref() {
        if let Some(target_stage) = active_stages
            .iter()
            .find(|stage| stage.id == *next_stage_id && stage.is_active)
        {
            if let Some(prefix) = s3_stage_input_prefix(target_stage, storage)? {
                return Ok(prefix);
            }
        }
    }

    if let Some(target_stage) = active_stages
        .iter()
        .filter(|stage| stage.id != source_stage.id && stage.is_active)
        .filter_map(|stage| {
            let prefix = s3_stage_input_prefix(stage, storage).ok().flatten()?;
            Some((stage, prefix))
        })
        .find(|(_, prefix)| {
            prefix.starts_with(storage.workspace_prefix.trim_end_matches('/'))
                && !prefix.ends_with("/raw")
        })
        .or_else(|| {
            active_stages
                .iter()
                .filter(|stage| stage.id != source_stage.id && stage.is_active)
                .filter_map(|stage| {
                    let prefix = s3_stage_input_prefix(stage, storage).ok().flatten()?;
                    Some((stage, prefix))
                })
                .next()
        })
    {
        return Ok(target_stage.1);
    }

    Ok(format!(
        "{}/processed",
        storage.workspace_prefix.trim_end_matches('/')
    ))
}

fn s3_stage_input_prefix(
    stage: &StageRecord,
    storage: &S3StorageConfig,
) -> Result<Option<String>, String> {
    let Some(input_uri) = stage.input_uri.as_deref() else {
        return Ok(None);
    };
    if !input_uri.starts_with("s3://") {
        return Ok(None);
    }
    let (bucket, prefix) = parse_s3_uri(input_uri).map_err(|error| {
        format!(
            "Invalid S3 input_uri on stage '{}': {}",
            stage.id, error.message
        )
    })?;
    if bucket != storage.bucket {
        return Ok(None);
    }
    Ok(Some(prefix))
}

struct HttpResponse {
    status: i64,
    body: String,
}

struct SuccessfulResponse {
    http_status: i64,
    response_json: String,
    output_payloads: Vec<Value>,
    meta: Option<Value>,
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
    let next_stage_required = resolved_next_stage.is_some();
    let mut meta = None;
    let output_payloads = match &value {
        Value::Array(items) => validate_output_items(items, response.status, &response.body)?,
        Value::Object(root) => {
            if root.get("success").and_then(Value::as_bool) == Some(false) {
                return Err(AttemptFailure {
                    error_type: "contract".to_string(),
                    message: "n8n response declared success=false.".to_string(),
                    http_status: Some(response.status),
                    response_json: Some(response.body),
                });
            }
            meta = root.get("meta").cloned();
            match root.get("payload") {
                Some(Value::Array(items)) => {
                    validate_output_items(items, response.status, &response.body)?
                }
                Some(Value::Object(_)) => vec![root.get("payload").cloned().unwrap()],
                Some(_) => {
                    return Err(AttemptFailure {
                        error_type: "contract".to_string(),
                        message: "n8n response payload must be an object or an array of objects."
                            .to_string(),
                        http_status: Some(response.status),
                        response_json: Some(response.body),
                    });
                }
                None if root.contains_key("success") => Vec::new(),
                None => vec![Value::Object(root.clone())],
            }
        }
        _ => {
            return Err(AttemptFailure {
                error_type: "contract".to_string(),
                message: "n8n response JSON must be an object or an array of objects.".to_string(),
                http_status: Some(response.status),
                response_json: Some(response.body),
            });
        }
    };
    if next_stage_required && output_payloads.is_empty() {
        return Err(AttemptFailure {
            error_type: "contract".to_string(),
            message: "n8n response must contain at least one output object when next_stage exists."
                .to_string(),
            http_status: Some(response.status),
            response_json: Some(response.body),
        });
    }
    Ok(SuccessfulResponse {
        http_status: response.status,
        response_json: response.body,
        output_payloads,
        meta,
    })
}

fn validate_output_items(
    items: &[Value],
    http_status: i64,
    response_body: &str,
) -> Result<Vec<Value>, AttemptFailure> {
    let mut outputs = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if !item.is_object() {
            return Err(AttemptFailure {
                error_type: "contract".to_string(),
                message: format!("n8n response output item at index {index} must be an object."),
                http_status: Some(http_status),
                response_json: Some(response_body.to_string()),
            });
        }
        outputs.push(item.clone());
    }
    Ok(outputs)
}

fn build_request_json(source_file: &crate::domain::EntityFileRecord) -> Result<Value, String> {
    serde_json::from_str::<Value>(&source_file.payload_json)
        .map_err(|error| format!("Failed to parse source payload JSON from DB: {error}"))
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
        let orphan_count =
            reconcile_orphan_stage_runs_for_queued_state(connection, *state_id, now)?;
        release_queued_claim(connection, *state_id, now)?;
        if orphan_count > 0 {
            insert_app_event(
                connection,
                AppEventLevel::Warning,
                "orphan_stage_run_reconciled",
                "An unfinished stage_run created before workflow start was marked reconciled.",
                Some(json!({"state_id": state_id, "orphan_stage_run_count": orphan_count})),
                now,
            )?;
        }
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
        bootstrap_database, claim_eligible_runtime_tasks, get_entity_detail, list_entity_files,
        list_stage_runs, open_connection, register_s3_artifact_pointer, start_claimed_stage_run,
        NewStageRunInput, RegisterS3ArtifactPointerInput,
    };
    use crate::discovery::scan_workspace;
    use crate::domain::{
        PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition, StorageConfig,
        StorageProvider,
    };
    use sha2::Digest;

    #[test]
    fn webhook_timeout_has_llm_friendly_floor() {
        assert_eq!(
            effective_webhook_timeout_sec(1),
            DEFAULT_REQUEST_TIMEOUT_SEC
        );
        assert_eq!(
            effective_webhook_timeout_sec(30),
            DEFAULT_REQUEST_TIMEOUT_SEC
        );
        assert_eq!(
            effective_webhook_timeout_sec(300),
            DEFAULT_REQUEST_TIMEOUT_SEC
        );
        assert_eq!(effective_webhook_timeout_sec(450), 450);
    }

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

    fn mock_server_dynamic<F>(request_count: usize, handler: F) -> MockServer
    where
        F: Fn(&str) -> (u16, String) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let url = format!(
            "http://{}/webhook/test",
            listener.local_addr().expect("addr")
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handler = Arc::new(handler);
        let handle = thread::spawn(move || {
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().expect("accept request");
                let mut buffer = [0_u8; 8192];
                let read = stream.read(&mut buffer).expect("read request");
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let (status, body) = handler(&request);
                captured.lock().expect("lock requests").push(request);
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
            storage: None,
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
                    input_uri: None,
                    output_folder: "stages/incoming-out".to_string(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts,
                    retry_delay_sec,
                    next_stage: Some("normalized".to_string()),
                    save_path_aliases: Vec::new(),
                    allow_empty_outputs: false,
                },
                StageDefinition {
                    id: "normalized".to_string(),
                    input_folder: "stages/normalized".to_string(),
                    input_uri: None,
                    output_folder: "stages/normalized-out".to_string(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts,
                    retry_delay_sec,
                    next_stage: None,
                    save_path_aliases: Vec::new(),
                    allow_empty_outputs: false,
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
            input_uri: None,
            output_folder: format!("{active_input}-out"),
            workflow_url: workflow_url.to_string(),
            max_attempts: 3,
            retry_delay_sec: 0,
            next_stage: next_stage.map(ToOwned::to_owned),
            save_path_aliases: Vec::new(),
            allow_empty_outputs: false,
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

    fn prepare_workdir_with_config(
        config: PipelineConfig,
        source_input_folder: &str,
        source_json: &str,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir.join(source_input_folder).join("entity-1.json");
        bootstrap_database(&database_path, &config).expect("bootstrap");
        std::fs::create_dir_all(source_path.parent().expect("source parent"))
            .expect("create source parent");
        std::fs::write(&source_path, source_json).expect("write source");
        scan_workspace(&workdir, &database_path).expect("scan");
        (tempdir, workdir, database_path, source_path)
    }

    fn request_body(request: &str) -> &str {
        request.split("\r\n\r\n").nth(1).unwrap_or_default()
    }

    fn header_value(request: &str, name: &str) -> Option<String> {
        request.lines().find_map(|line| {
            let (header, value) = line.split_once(':')?;
            if header.eq_ignore_ascii_case(name) {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
    }

    fn captured_request_json(server: &MockServer, index: usize) -> Value {
        let requests = server.requests.lock().expect("requests");
        serde_json::from_str(request_body(requests.get(index).expect("captured request")))
            .expect("request body json")
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

    fn files_for_stage(
        database_path: &std::path::Path,
        stage_id: &str,
    ) -> Vec<crate::domain::EntityFileRecord> {
        list_entity_files(database_path, None)
            .expect("files")
            .into_iter()
            .filter(|file| file.stage_id == stage_id)
            .collect()
    }

    fn s3_test_config_with_allow_empty_outputs(
        workflow_url: &str,
        raw_next_stage: Option<&str>,
        allow_empty_outputs: bool,
    ) -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive-s3-dev".to_string(),
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
            stages: vec![
                StageDefinition {
                    id: "raw".to_string(),
                    input_folder: String::new(),
                    input_uri: Some("s3://steos-s3-data/main_dir/raw".to_string()),
                    output_folder: String::new(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts: 1,
                    retry_delay_sec: 0,
                    next_stage: raw_next_stage.map(ToOwned::to_owned),
                    save_path_aliases: vec![
                        "main_dir/raw".to_string(),
                        "/main_dir/raw".to_string(),
                    ],
                    allow_empty_outputs,
                },
                StageDefinition {
                    id: "raw_entities".to_string(),
                    input_folder: String::new(),
                    input_uri: Some(
                        "s3://steos-s3-data/main_dir/processed/raw_entities".to_string(),
                    ),
                    output_folder: String::new(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts: 1,
                    retry_delay_sec: 0,
                    next_stage: None,
                    save_path_aliases: vec![
                        "main_dir/processed/raw_entities".to_string(),
                        "/main_dir/processed/raw_entities".to_string(),
                    ],
                    allow_empty_outputs: false,
                },
            ],
        }
    }

    fn prepare_s3_workdir(
        workflow_url: &str,
        raw_next_stage: Option<&str>,
    ) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        prepare_s3_workdir_with_allow_empty_outputs(workflow_url, raw_next_stage, false)
    }

    fn prepare_s3_workdir_with_allow_empty_outputs(
        workflow_url: &str,
        raw_next_stage: Option<&str>,
        allow_empty_outputs: bool,
    ) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let config = s3_test_config_with_allow_empty_outputs(
            workflow_url,
            raw_next_stage,
            allow_empty_outputs,
        );
        bootstrap_database(&database_path, &config).expect("bootstrap");
        register_s3_artifact_pointer(
            &database_path,
            &RegisterS3ArtifactPointerInput {
                entity_id: "entity-1".to_string(),
                artifact_id: "source-001".to_string(),
                relation_to_source: None,
                stage_id: "raw".to_string(),
                bucket: "steos-s3-data".to_string(),
                key: s3_source_key().to_string(),
                version_id: None,
                etag: Some("etag-source".to_string()),
                checksum_sha256: None,
                size: Some(123),
                last_modified: None,
                source_file_id: None,
                producer_run_id: None,
                status: StageStatus::Pending,
            },
        )
        .expect("register source");
        let connection = open_connection(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_files SET payload_json = ?1 WHERE stage_id = 'raw'",
                [r#"{"title":"hello beehive","body":"business payload must not be sent"}"#],
            )
            .expect("seed S3 business payload");
        (tempdir, workdir, database_path)
    }

    fn s3_source_key() -> &'static str {
        "main_dir/raw/smoke_entity_001__порфирия.json"
    }

    fn s3_success_manifest(run_id: &str, save_path: &str, key: &str) -> String {
        let source_key = s3_source_key();
        s3_success_manifest_for_source(
            run_id,
            source_key,
            save_path,
            key,
            "art_001",
            "entity-1-child",
        )
    }

    fn s3_success_manifest_for_source(
        run_id: &str,
        source_key: &str,
        save_path: &str,
        key: &str,
        artifact_id: &str,
        entity_id: &str,
    ) -> String {
        format!(
            r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-s3-dev",
  "run_id":"{run_id}",
  "source":{{"bucket":"steos-s3-data","key":"{source_key}","version_id":null,"etag":"etag-source"}},
  "status":"success",
  "outputs":[{{"artifact_id":"{artifact_id}","entity_id":"{entity_id}","relation_to_source":"child_entity","bucket":"steos-s3-data","key":"{key}","save_path":"{save_path}","content_type":"application/json","checksum_sha256":null,"size":456}}],
  "created_at":"2026-05-12T00:00:00Z"
}}"#
        )
    }

    fn s3_success_empty_manifest(run_id: &str, source_key: &str) -> String {
        format!(
            r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-s3-dev",
  "run_id":"{run_id}",
  "source":{{"bucket":"steos-s3-data","key":"{source_key}"}},
  "status":"success",
  "outputs":[],
  "created_at":"2026-05-12T00:00:00Z"
}}"#
        )
    }

    fn prepare_s3_terminal_sources(
        workflow_url: &str,
        count: u64,
    ) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let config = s3_test_config_with_allow_empty_outputs(workflow_url, None, true);
        bootstrap_database(&database_path, &config).expect("bootstrap");
        for index in 1..=count {
            register_s3_artifact_pointer(
                &database_path,
                &RegisterS3ArtifactPointerInput {
                    entity_id: format!("entity-{index}"),
                    artifact_id: format!("source-{index:03}"),
                    relation_to_source: None,
                    stage_id: "raw".to_string(),
                    bucket: "steos-s3-data".to_string(),
                    key: format!("main_dir/raw/entity-{index}.json"),
                    version_id: None,
                    etag: Some(format!("etag-{index}")),
                    checksum_sha256: None,
                    size: Some(100 + index),
                    last_modified: None,
                    source_file_id: None,
                    producer_run_id: None,
                    status: StageStatus::Pending,
                },
            )
            .expect("register terminal S3 source");
        }
        (tempdir, workdir, database_path)
    }

    fn s3_multistage_config(workflow_url: &str) -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive-s3-dev".to_string(),
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
            stages: vec![
                StageDefinition {
                    id: "raw".to_string(),
                    input_folder: String::new(),
                    input_uri: Some("s3://steos-s3-data/main_dir/raw".to_string()),
                    output_folder: String::new(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts: 1,
                    retry_delay_sec: 0,
                    next_stage: Some("raw_entities".to_string()),
                    save_path_aliases: vec!["main_dir/raw".to_string()],
                    allow_empty_outputs: false,
                },
                StageDefinition {
                    id: "raw_entities".to_string(),
                    input_folder: String::new(),
                    input_uri: Some(
                        "s3://steos-s3-data/main_dir/processed/raw_entities".to_string(),
                    ),
                    output_folder: String::new(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts: 1,
                    retry_delay_sec: 0,
                    next_stage: Some("final".to_string()),
                    save_path_aliases: vec!["main_dir/processed/raw_entities".to_string()],
                    allow_empty_outputs: false,
                },
                StageDefinition {
                    id: "final".to_string(),
                    input_folder: String::new(),
                    input_uri: Some("s3://steos-s3-data/main_dir/final".to_string()),
                    output_folder: String::new(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts: 1,
                    retry_delay_sec: 0,
                    next_stage: None,
                    save_path_aliases: vec!["main_dir/final".to_string()],
                    allow_empty_outputs: true,
                },
            ],
        }
    }

    fn s3_branching_config(workflow_url: &str) -> PipelineConfig {
        let mut config = s3_multistage_config(workflow_url);
        config.stages[0].next_stage = Some("raw_entities".to_string());
        config.stages[1].next_stage = None;
        config.stages[2] = StageDefinition {
            id: "raw_representations".to_string(),
            input_folder: String::new(),
            input_uri: Some(
                "s3://steos-s3-data/main_dir/processed/raw_representations".to_string(),
            ),
            output_folder: String::new(),
            workflow_url: workflow_url.to_string(),
            max_attempts: 1,
            retry_delay_sec: 0,
            next_stage: None,
            save_path_aliases: vec!["main_dir/processed/raw_representations".to_string()],
            allow_empty_outputs: false,
        };
        config
    }

    fn prepare_s3_config_workdir(
        workflow_url: &str,
        config: PipelineConfig,
    ) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(&database_path, &config).expect("bootstrap");
        register_s3_artifact_pointer(
            &database_path,
            &RegisterS3ArtifactPointerInput {
                entity_id: "entity-1".to_string(),
                artifact_id: "source-001".to_string(),
                relation_to_source: None,
                stage_id: "raw".to_string(),
                bucket: "steos-s3-data".to_string(),
                key: s3_source_key().to_string(),
                version_id: None,
                etag: Some("etag-source".to_string()),
                checksum_sha256: None,
                size: Some(123),
                last_modified: None,
                source_file_id: None,
                producer_run_id: None,
                status: StageStatus::Pending,
            },
        )
        .expect("register source");
        assert!(
            workflow_url.starts_with("http"),
            "mock workflow URL should be HTTP"
        );
        (tempdir, workdir, database_path)
    }

    #[test]
    fn run_pipeline_waves_stops_when_no_tasks_are_claimed() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config("http://127.0.0.1/unused", 1, 0),
        )
        .expect("bootstrap");

        let summary =
            run_pipeline_waves(&workdir, &database_path, 5, 3, true, 5, 1, 0).expect("waves");

        assert_eq!(summary.waves_executed, 1);
        assert_eq!(summary.total_claimed, 0);
        assert_eq!(summary.stopped_reason, "idle");
        assert_eq!(summary.wave_summaries.len(), 1);
        assert_eq!(summary.wave_summaries[0].summary.claimed, 0);
    }

    #[test]
    fn run_pipeline_waves_clamps_limits_and_aggregates_per_wave_summaries() {
        let server = mock_server_dynamic(3, |request| {
            let control = serde_json::from_str::<Value>(request_body(request))
                .expect("S3 control envelope JSON");
            let run_id = control["run_id"].as_str().expect("run id");
            let source_key = control["source_key"].as_str().expect("source key");
            (200, s3_success_empty_manifest(run_id, source_key))
        });
        let (_tempdir, workdir, database_path) = prepare_s3_terminal_sources(&server.url, 3);

        let summary =
            run_pipeline_waves(&workdir, &database_path, 99, 99, true, 5, 1, 0).expect("waves");

        assert_eq!(summary.requested_max_waves, 99);
        assert_eq!(summary.requested_max_tasks_per_wave, 99);
        assert_eq!(summary.max_waves, 10);
        assert_eq!(summary.max_tasks_per_wave, 5);
        assert_eq!(summary.max_total_tasks, 50);
        assert_eq!(summary.waves_executed, 2);
        assert_eq!(summary.total_claimed, 3);
        assert_eq!(summary.total_succeeded, 3);
        assert_eq!(summary.stopped_reason, "idle");
        assert_eq!(summary.wave_summaries[0].summary.claimed, 3);
        assert_eq!(summary.wave_summaries[0].summary.succeeded, 3);
        assert_eq!(summary.wave_summaries[1].summary.claimed, 0);
        assert_eq!(server.requests.lock().expect("requests").len(), 3);
    }

    #[test]
    fn run_pipeline_waves_stops_on_failed_or_blocked_when_requested() {
        let server = mock_server(vec![(500, r#"{"error":"upstream"}"#)]);
        let (_tempdir, workdir, database_path) = prepare_s3_terminal_sources(&server.url, 1);

        let summary =
            run_pipeline_waves(&workdir, &database_path, 5, 1, true, 5, 1, 0).expect("waves");

        assert_eq!(summary.waves_executed, 1);
        assert_eq!(summary.total_claimed, 1);
        assert_eq!(summary.total_failed, 1);
        assert_eq!(summary.stopped_reason, "failure_or_blocked");
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
    }

    #[test]
    fn mock_s3_multistage_pipeline_moves_one_artifact_through_two_stages() {
        let server = mock_server_dynamic(2, |request| {
            let control = serde_json::from_str::<Value>(request_body(request))
                .expect("S3 control envelope JSON");
            let run_id = control["run_id"].as_str().expect("run id");
            let source_key = control["source_key"].as_str().expect("source key");
            match control["stage_id"].as_str() {
                Some("raw") => (
                    200,
                    s3_success_manifest_for_source(
                        run_id,
                        source_key,
                        "main_dir/processed/raw_entities",
                        "main_dir/processed/raw_entities/stage-a.json",
                        "stage-a-artifact",
                        "entity-1-stage-a",
                    ),
                ),
                Some("raw_entities") => (
                    200,
                    s3_success_manifest_for_source(
                        run_id,
                        source_key,
                        "main_dir/final",
                        "main_dir/final/stage-b.json",
                        "stage-b-artifact",
                        "entity-1-stage-b",
                    ),
                ),
                other => panic!("unexpected stage {other:?}"),
            }
        });
        let (_tempdir, workdir, database_path) =
            prepare_s3_config_workdir(&server.url, s3_multistage_config(&server.url));

        let summary =
            run_pipeline_waves(&workdir, &database_path, 2, 1, true, 5, 1, 0).expect("waves");
        let stage_a_files = files_for_stage(&database_path, "raw_entities");
        let final_files = files_for_stage(&database_path, "final");

        assert_eq!(summary.waves_executed, 2, "summary: {summary:?}");
        assert_eq!(summary.total_claimed, 2, "summary: {summary:?}");
        assert_eq!(summary.total_succeeded, 2, "summary: {summary:?}");
        assert_eq!(summary.stopped_reason, "max_waves_reached");
        assert_eq!(stage_a_files.len(), 1);
        assert_eq!(final_files.len(), 1);
        assert_eq!(
            final_files[0].key.as_deref(),
            Some("main_dir/final/stage-b.json")
        );
        assert_eq!(
            captured_request_json(&server, 0)["stage_id"].as_str(),
            Some("raw")
        );
        assert_eq!(
            captured_request_json(&server, 1)["stage_id"].as_str(),
            Some("raw_entities")
        );
    }

    #[test]
    fn mock_s3_branching_response_registers_outputs_by_save_path() {
        let server = mock_server_dynamic(1, |request| {
            let control = serde_json::from_str::<Value>(request_body(request))
                .expect("S3 control envelope JSON");
            let run_id = control["run_id"].as_str().expect("run id");
            let source_key = control["source_key"].as_str().expect("source key");
            (
                200,
                format!(
                    r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-s3-dev",
  "run_id":"{run_id}",
  "source":{{"bucket":"steos-s3-data","key":"{source_key}"}},
  "status":"success",
  "outputs":[
    {{"artifact_id":"entity-artifact","entity_id":"entity-branch","relation_to_source":"child_entity","bucket":"steos-s3-data","key":"main_dir/processed/raw_entities/entity.json","save_path":"main_dir/processed/raw_entities","content_type":"application/json","checksum_sha256":null,"size":100}},
    {{"artifact_id":"representation-artifact","entity_id":"representation-branch","relation_to_source":"representation_of","bucket":"steos-s3-data","key":"main_dir/processed/raw_representations/representation.json","save_path":"main_dir/processed/raw_representations","content_type":"application/json","checksum_sha256":null,"size":200}}
  ],
  "created_at":"2026-05-12T00:00:00Z"
}}"#
                ),
            )
        });
        let (_tempdir, workdir, database_path) =
            prepare_s3_config_workdir(&server.url, s3_branching_config(&server.url));

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");
        let entity_files = files_for_stage(&database_path, "raw_entities");
        let representation_files = files_for_stage(&database_path, "raw_representations");

        assert_eq!(summary.succeeded, 1, "summary: {summary:?}; runs: {runs:?}");
        assert_eq!(entity_files.len(), 1);
        assert_eq!(representation_files.len(), 1);
        assert_eq!(
            entity_files[0].key.as_deref(),
            Some("main_dir/processed/raw_entities/entity.json")
        );
        assert_eq!(
            representation_files[0].key.as_deref(),
            Some("main_dir/processed/raw_representations/representation.json")
        );
    }

    #[test]
    fn s3_mode_sends_json_control_body_and_registers_output_pointer() {
        let server = mock_server_dynamic(1, |request| {
            let control = serde_json::from_str::<Value>(request_body(request))
                .expect("S3 control envelope JSON");
            let run_id = control["run_id"].as_str().expect("run id").to_string();
            (
                200,
                s3_success_manifest(
                    &run_id,
                    "main_dir/processed/raw_entities",
                    "main_dir/processed/raw_entities/art_001.json",
                ),
            )
        });
        let (_tempdir, workdir, database_path) =
            prepare_s3_workdir(&server.url, Some("raw_entities"));

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");
        let requests = server.requests.lock().expect("requests");
        let request = requests.first().expect("request");
        let target_files = files_for_stage(&database_path, "raw_entities");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");
        let control = serde_json::from_str::<Value>(request_body(request)).expect("control JSON");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(
            header_value(request, "Content-Type").as_deref(),
            Some("application/json; charset=utf-8")
        );
        assert_eq!(
            control["schema"].as_str(),
            Some("beehive.s3_control_envelope.v1")
        );
        assert_eq!(control["source_bucket"].as_str(), Some("steos-s3-data"));
        assert_eq!(control["source_key"].as_str(), Some(s3_source_key()));
        assert!(request_body(request).contains("порфирия"));
        assert_eq!(control["source_entity_id"].as_str(), Some("entity-1"));
        assert_eq!(control["source_artifact_id"].as_str(), Some("source-001"));
        assert_eq!(
            control["target_prefix"].as_str(),
            Some("main_dir/processed/raw_entities")
        );
        assert_eq!(
            control["save_path"].as_str(),
            Some("main_dir/processed/raw_entities")
        );
        assert!(header_value(request, "X-Beehive-Source-Key").is_none());
        assert_eq!(target_files.len(), 1);
        assert_eq!(target_files[0].storage_provider, StorageProvider::S3);
        assert_eq!(
            target_files[0].key.as_deref(),
            Some("main_dir/processed/raw_entities/art_001.json")
        );
        assert!(target_files[0].producer_run_id.is_some());
        assert_eq!(stage_status(&database_path, "raw"), "done");
        assert!(runs[0]
            .request_json
            .contains("beehive.s3_control_envelope.v1"));
        assert!(runs[0].request_json.contains("порфирия"));
        assert!(!runs[0].request_json.contains("hello beehive"));
        assert!(!request_body(request).contains("hello beehive"));
        assert!(!request_body(request).contains("business payload must not be sent"));
    }

    #[test]
    fn s3_error_manifest_fails_without_child_outputs() {
        let server = mock_server_dynamic(1, |request| {
            let control = serde_json::from_str::<Value>(request_body(request))
                .expect("S3 control envelope JSON");
            let run_id = control["run_id"].as_str().expect("run id");
            let source_key = control["source_key"].as_str().expect("source key");
            (
                200,
                format!(
                    r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-s3-dev",
  "run_id":"{run_id}",
  "source":{{"bucket":"steos-s3-data","key":"{source_key}"}},
  "status":"error",
  "error_type":"llm_invalid_json",
  "error_message":"Model returned invalid JSON",
  "outputs":[],
  "created_at":"2026-05-12T00:00:00Z"
}}"#
                ),
            )
        });
        let (_tempdir, workdir, database_path) =
            prepare_s3_workdir(&server.url, Some("raw_entities"));

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.failed, 1);
        assert_eq!(stage_status(&database_path, "raw"), "failed");
        assert_eq!(runs[0].error_type.as_deref(), Some("llm_invalid_json"));
        assert!(files_for_stage(&database_path, "raw_entities").is_empty());
    }

    #[test]
    fn s3_invalid_save_path_manifest_blocks_run() {
        let server = mock_server_dynamic(1, |request| {
            let control = serde_json::from_str::<Value>(request_body(request))
                .expect("S3 control envelope JSON");
            let run_id = control["run_id"].as_str().expect("run id");
            (
                200,
                s3_success_manifest(
                    &run_id,
                    "main_dir/processed/unknown",
                    "main_dir/processed/unknown/art_001.json",
                ),
            )
        });
        let (_tempdir, workdir, database_path) =
            prepare_s3_workdir(&server.url, Some("raw_entities"));

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.blocked, 1);
        assert_eq!(stage_status(&database_path, "raw"), "blocked");
        assert_eq!(runs[0].error_type.as_deref(), Some("manifest_blocked"));
        assert!(files_for_stage(&database_path, "raw_entities").is_empty());
    }

    #[test]
    fn s3_success_with_no_outputs_requires_stage_opt_in() {
        let server = mock_server_dynamic(1, |request| {
            let control = serde_json::from_str::<Value>(request_body(request))
                .expect("S3 control envelope JSON");
            let run_id = control["run_id"].as_str().expect("run id");
            let source_key = control["source_key"].as_str().expect("source key");
            (
                200,
                format!(
                    r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-s3-dev",
  "run_id":"{run_id}",
  "source":{{"bucket":"steos-s3-data","key":"{source_key}"}},
  "status":"success",
  "outputs":[],
  "created_at":"2026-05-12T00:00:00Z"
}}"#
                ),
            )
        });
        let (_tempdir, workdir, database_path) = prepare_s3_workdir(&server.url, None);

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.failed, 1);
        assert_eq!(stage_status(&database_path, "raw"), "failed");
        assert_eq!(runs[0].error_type.as_deref(), Some("manifest_invalid"));
        assert!(runs[0]
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("allow_empty_outputs"));
    }

    #[test]
    fn s3_terminal_success_with_no_outputs_marks_done() {
        let server = mock_server_dynamic(1, |request| {
            let control = serde_json::from_str::<Value>(request_body(request))
                .expect("S3 control envelope JSON");
            let run_id = control["run_id"].as_str().expect("run id");
            let source_key = control["source_key"].as_str().expect("source key");
            (
                200,
                format!(
                    r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-s3-dev",
  "run_id":"{run_id}",
  "source":{{"bucket":"steos-s3-data","key":"{source_key}"}},
  "status":"success",
  "outputs":[],
  "created_at":"2026-05-12T00:00:00Z"
}}"#
                ),
            )
        });
        let (_tempdir, workdir, database_path) =
            prepare_s3_workdir_with_allow_empty_outputs(&server.url, None, true);

        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(stage_status(&database_path, "raw"), "done");
        assert!(files_for_stage(&database_path, "raw_entities").is_empty());
    }

    fn canonical_hash8(value: &Value) -> String {
        fn normalize(value: &Value) -> Value {
            match value {
                Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
                Value::Object(object) => {
                    let mut entries = object.iter().collect::<Vec<_>>();
                    entries.sort_by(|left, right| left.0.cmp(right.0));
                    let mut normalized = serde_json::Map::new();
                    for (key, value) in entries {
                        normalized.insert(key.clone(), normalize(value));
                    }
                    Value::Object(normalized)
                }
                other => other.clone(),
            }
        }
        let bytes = serde_json::to_vec(&normalize(value)).expect("canonical payload");
        let hash = format!("{:x}", sha2::Sha256::digest(&bytes));
        hash[..8].to_string()
    }

    fn stage_run_input(run_id: &str, attempt_no: u64) -> NewStageRunInput {
        NewStageRunInput {
            run_id: run_id.to_string(),
            entity_id: "entity-1".to_string(),
            entity_file_id: 1,
            stage_id: "incoming".to_string(),
            attempt_no,
            workflow_url: "http://localhost/webhook".to_string(),
            request_json: "{}".to_string(),
            started_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn atomic_start_creates_run_and_moves_queued_to_in_progress() {
        let server = mock_server(Vec::new());
        let (_tempdir, _workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let mut connection = open_connection(&database_path).expect("open db");
        let claimed = claim_eligible_runtime_tasks(&mut connection, &Utc::now().to_rfc3339(), 1)
            .expect("claim");
        let task = claimed.first().expect("claimed task");
        let input = stage_run_input("run-atomic-start", 1);

        start_claimed_stage_run(&mut connection, task.state_id, &input).expect("atomic start");
        let state = stage_state(&database_path, "incoming");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(state.status, "in_progress");
        assert_eq!(state.attempts, 1);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, "run-atomic-start");
    }

    #[test]
    fn atomic_start_failure_does_not_insert_partial_stage_run() {
        let server = mock_server(Vec::new());
        let (_tempdir, _workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let mut connection = open_connection(&database_path).expect("open db");
        let state = stage_state(&database_path, "incoming");
        let input = stage_run_input("run-should-not-exist", 1);

        let result = start_claimed_stage_run(&mut connection, state.id, &input);
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");
        let after = stage_state(&database_path, "incoming");

        assert!(result.is_err());
        assert!(runs.is_empty());
        assert_eq!(after.status, "pending");
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
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");
        let files = list_entity_files(&database_path, None).expect("files");
        let target_file = files
            .iter()
            .find(|file| file.stage_id == "normalized" && file.copy_source_file_id.is_some())
            .expect("target db file");
        let target_path = std::path::PathBuf::from(&target_file.file_path);
        let target_json =
            serde_json::from_slice::<Value>(&std::fs::read(&target_path).expect("read target"))
                .expect("parse target");
        let target_detail = get_entity_detail(&database_path, &target_file.entity_id)
            .expect("target detail result")
            .expect("target detail exists");
        let target_checksum = format!(
            "{:x}",
            sha2::Sha256::digest(&std::fs::read(&target_path).expect("target bytes"))
        );

        assert_eq!(summary.claimed, 1);
        assert_eq!(summary.succeeded, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "done");
        assert_eq!(
            target_detail
                .stage_states
                .iter()
                .find(|state| state.stage_id == "normalized")
                .map(|state| state.status.as_str()),
            Some("pending")
        );
        assert_ne!(
            target_json.get("id").and_then(Value::as_str),
            Some("entity-1")
        );
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
            Some("n8n_response")
        );
        assert_eq!(
            target_json
                .get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("beehive"))
                .and_then(Value::as_object)
                .and_then(|beehive| beehive.get("source_entity_id"))
                .and_then(Value::as_str),
            Some("entity-1")
        );
        assert_eq!(
            std::fs::read(&source_path).expect("read source after"),
            source_before
        );
        assert_eq!(runs.len(), 1);
        assert!(runs[0].success);
        assert_eq!(runs[0].attempt_no, 1);
        assert_eq!(runs[0].http_status, Some(200));
        let stored_request_json =
            serde_json::from_str::<Value>(&runs[0].request_json).expect("stored request json");
        assert_eq!(stored_request_json, json!({"title": "hello beehive"}));
        assert_eq!(captured_request_json(&server, 0), stored_request_json);
        let captured_body = {
            let requests = server.requests.lock().expect("requests");
            request_body(requests.first().expect("first request")).to_string()
        };
        for forbidden in [
            "entity_id",
            "stage_id",
            "entity_file_id",
            "attempt",
            "run_id",
        ] {
            assert!(
                !captured_body.contains(forbidden),
                "payload-only body should not contain {forbidden}"
            );
        }
        assert_eq!(target_file.checksum, target_checksum);
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
    }

    #[test]
    fn root_array_response_creates_multiple_child_entities() {
        let server = mock_server(vec![(
            200,
            r#"[{"entity_name":"child one"},{"entity_name":"child two"},{"entity_name":"child three"}]"#,
        )]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);

        let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("run tasks");
        let target_files = files_for_stage(&database_path, "normalized");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "done");
        assert_eq!(target_files.len(), 3);
        let ids = target_files
            .iter()
            .map(|file| file.entity_id.clone())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 3);
        for (index, file) in target_files.iter().enumerate() {
            let target_json =
                serde_json::from_slice::<Value>(&std::fs::read(&file.file_path).expect("target"))
                    .expect("target json");
            assert_eq!(
                target_json.get("id").and_then(Value::as_str),
                Some(file.entity_id.as_str())
            );
            assert_eq!(
                target_json.get("current_stage").and_then(Value::as_str),
                Some("normalized")
            );
            assert_eq!(
                target_json.get("status").and_then(Value::as_str),
                Some("pending")
            );
            assert!(target_json.get("payload").is_some_and(Value::is_object));
            let beehive = target_json
                .get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("beehive"))
                .and_then(Value::as_object)
                .expect("beehive meta");
            assert_eq!(
                beehive.get("created_by").and_then(Value::as_str),
                Some("n8n_response")
            );
            assert_eq!(
                beehive.get("source_entity_id").and_then(Value::as_str),
                Some("entity-1")
            );
            assert_eq!(beehive.get("output_count").and_then(Value::as_u64), Some(3));
            assert!(beehive
                .get("output_index")
                .and_then(Value::as_u64)
                .is_some_and(|value| value < 3));
            assert!(
                file.copy_source_file_id.is_some(),
                "target {index} should be managed"
            );
        }
    }

    #[test]
    fn wrapper_payload_array_response_creates_multiple_child_entities() {
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"payload":[{"id":"explicit-child","entity_name":"child one"},{"entity_name":"child two"}],"meta":{"workflow":"mock-array"}}"#,
        )]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);

        let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("run tasks");
        let target_files = files_for_stage(&database_path, "normalized");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(target_files.len(), 2);
        assert!(target_files
            .iter()
            .any(|file| file.entity_id == "explicit-child"));
        assert!(target_files
            .iter()
            .all(|file| file.copy_source_file_id.is_some()));
    }

    #[test]
    fn save_path_routes_array_outputs_to_multiple_stages() {
        let server = mock_server(vec![(
            200,
            r#"[
                {"entity_name":"castle","save_path":"main_dir/processed/raw_entities"},
                {"target_entity_name":"phone","save_path":"main_dir/processed/raw_representations"}
            ]"#,
        )]);
        let config = PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            storage: None,
            runtime: RuntimeConfig::default(),
            stages: vec![
                stage("incoming", &server.url, None, "stages/incoming"),
                stage(
                    "raw_entities",
                    &server.url,
                    None,
                    "main_dir/processed/raw_entities",
                ),
                stage(
                    "raw_representations",
                    &server.url,
                    None,
                    "main_dir/processed/raw_representations",
                ),
            ],
        };
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir_with_config(
            config,
            "stages/incoming",
            r#"{"id":"entity-1","current_stage":"incoming","status":"pending","payload":{"title":"route me"},"meta":{"source":"manual"}}"#,
        );

        let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("run tasks");
        let entity_files = files_for_stage(&database_path, "raw_entities");
        let representation_files = files_for_stage(&database_path, "raw_representations");
        let entity_json = serde_json::from_slice::<Value>(
            &std::fs::read(&entity_files[0].file_path).expect("entity target"),
        )
        .expect("entity json");
        let representation_json = serde_json::from_slice::<Value>(
            &std::fs::read(&representation_files[0].file_path).expect("representation target"),
        )
        .expect("representation json");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "done");
        assert_eq!(entity_files.len(), 1);
        assert_eq!(representation_files.len(), 1);
        assert_eq!(
            entity_json.get("current_stage").and_then(Value::as_str),
            Some("raw_entities")
        );
        assert_eq!(
            representation_json
                .get("current_stage")
                .and_then(Value::as_str),
            Some("raw_representations")
        );
        assert_eq!(
            entity_json
                .get("payload")
                .and_then(|payload| payload.get("save_path"))
                .and_then(Value::as_str),
            Some("main_dir/processed/raw_entities")
        );
        assert_eq!(
            get_entity_detail(&database_path, &entity_files[0].entity_id)
                .expect("target detail")
                .expect("target exists")
                .stage_states
                .iter()
                .find(|state| state.stage_id == "raw_entities")
                .map(|state| state.status.as_str()),
            Some("pending")
        );
    }

    #[test]
    fn save_path_routes_direct_object_response() {
        let server = mock_server(vec![(
            200,
            r#"{"entity_name":"castle","save_path":"main_dir/processed/raw_entities"}"#,
        )]);
        let config = PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            storage: None,
            runtime: RuntimeConfig::default(),
            stages: vec![
                stage("incoming", &server.url, None, "stages/incoming"),
                stage(
                    "raw_entities",
                    &server.url,
                    None,
                    "main_dir/processed/raw_entities",
                ),
            ],
        };
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir_with_config(
            config,
            "stages/incoming",
            r#"{"id":"entity-1","current_stage":"incoming","status":"pending","payload":{"title":"direct"},"meta":{}}"#,
        );

        let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("run tasks");
        let target_files = files_for_stage(&database_path, "raw_entities");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(target_files.len(), 1);
        assert_eq!(stage_status(&database_path, "incoming"), "done");
    }

    #[test]
    fn legacy_main_dir_save_path_is_logical_not_os_absolute() {
        let server = mock_server(vec![(
            200,
            r#"{"entity_name":"castle","save_path":"/main_dir/processed/raw_entities"}"#,
        )]);
        let config = PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            storage: None,
            runtime: RuntimeConfig::default(),
            stages: vec![
                stage("incoming", &server.url, None, "stages/incoming"),
                stage(
                    "raw_entities",
                    &server.url,
                    None,
                    "main_dir/processed/raw_entities",
                ),
            ],
        };
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir_with_config(
            config,
            "stages/incoming",
            r#"{"id":"entity-1","current_stage":"incoming","status":"pending","payload":{"title":"legacy"},"meta":{}}"#,
        );

        let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("run tasks");
        let target_files = files_for_stage(&database_path, "raw_entities");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(target_files.len(), 1);
        assert!(std::path::Path::new(&target_files[0].file_path).starts_with(&workdir));
    }

    #[test]
    fn unsafe_save_path_is_rejected_without_target_writes() {
        for save_path in [
            "",
            "../outside",
            "/etc/passwd",
            "C:\\Users\\bad\\file",
            "\\\\server\\share",
        ] {
            let response = json!({"entity_name": "bad", "save_path": save_path}).to_string();
            let leaked_response: &'static str = Box::leak(response.into_boxed_str());
            let server = mock_server(vec![(200, leaked_response)]);
            let config = PipelineConfig {
                project: ProjectConfig {
                    name: "beehive".to_string(),
                    workdir: ".".to_string(),
                },
                storage: None,
                runtime: RuntimeConfig::default(),
                stages: vec![
                    stage("incoming", &server.url, None, "stages/incoming"),
                    stage("raw_entities", &server.url, None, "stages/raw_entities"),
                ],
            };
            let (tempdir, workdir, database_path, _source_path) = prepare_workdir_with_config(
                config,
                "stages/incoming",
                r#"{"id":"entity-1","current_stage":"incoming","status":"pending","payload":{"title":"unsafe"},"meta":{}}"#,
            );

            let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0)
                .unwrap_or_else(|error| panic!("run tasks for save_path {save_path:?}: {error}"));
            let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

            assert_eq!(summary.blocked, 1, "save_path {save_path:?}");
            assert_eq!(stage_status(&database_path, "incoming"), "blocked");
            assert_eq!(files_for_stage(&database_path, "raw_entities").len(), 0);
            assert!(!tempdir.path().join("outside").exists());
            assert!(!workdir.join("etc").exists());
            assert!(!runs[0].success);
            assert_eq!(runs[0].error_type.as_deref(), Some("copy_blocked"));
        }
    }

    #[test]
    fn invalid_output_item_fails_without_creating_target_files() {
        let server = mock_server(vec![(200, r#"[{"entity_name":"ok"}, 5]"#)]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 1, 0);

        let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("run tasks");
        let target_files = files_for_stage(&database_path, "normalized");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.failed, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "failed");
        assert!(target_files.is_empty());
        assert_eq!(runs[0].error_type.as_deref(), Some("contract"));
    }

    #[test]
    fn duplicate_multi_output_rerun_reuses_compatible_existing_files() {
        let response = r#"[{"entity_name":"child one"},{"entity_name":"child two"}]"#;
        let server = mock_server(vec![(200, response), (200, response)]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);

        let first = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("first run");
        let first_targets = files_for_stage(&database_path, "normalized");
        let first_paths = first_targets
            .iter()
            .map(|file| file.file_path.clone())
            .collect::<Vec<_>>();
        let connection = open_connection(&database_path).expect("open db");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'pending', attempts = 0, next_retry_at = NULL WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                [],
            )
            .expect("reset source");

        let second = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("second run");
        let second_targets = files_for_stage(&database_path, "normalized");
        let second_paths = second_targets
            .iter()
            .map(|file| file.file_path.clone())
            .collect::<Vec<_>>();

        assert_eq!(first.succeeded, 1);
        assert_eq!(second.succeeded, 1);
        assert_eq!(first_targets.len(), 2);
        assert_eq!(second_targets.len(), 2);
        assert_eq!(first_paths, second_paths);
        assert_eq!(server.requests.lock().expect("requests").len(), 2);
    }

    #[test]
    fn incompatible_generated_target_collision_fails_without_marking_source_done() {
        let payload = json!({"entity_name":"collision"});
        let child_id = format!("entity-1__normalized__0_{}", canonical_hash8(&payload));
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"payload":{"entity_name":"collision"},"meta":{}}"#,
        )]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 1, 0);
        let collision_path = workdir
            .join("stages")
            .join("normalized")
            .join(format!("{child_id}.json"));
        std::fs::create_dir_all(collision_path.parent().expect("collision parent"))
            .expect("collision parent");
        std::fs::write(
            &collision_path,
            format!(
                r#"{{"id":"{child_id}","current_stage":"normalized","status":"pending","payload":{{"entity_name":"different"}},"meta":{{"beehive":{{"source_entity_id":"entity-1","source_entity_file_id":1,"source_stage_id":"incoming","target_stage_id":"normalized","output_index":0}}}}}}"#
            ),
        )
        .expect("write collision");

        let summary = run_due_tasks(&workdir, &database_path, 3, 5, 1, 0).expect("run tasks");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.failed, 1);
        assert_eq!(stage_status(&database_path, "incoming"), "failed");
        assert_eq!(runs[0].error_type.as_deref(), Some("copy_failed"));
        assert_eq!(files_for_stage(&database_path, "normalized").len(), 0);
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
                storage: None,
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
    fn legacy_orphan_stage_run_is_reconciled_with_stale_queued_claim() {
        let server = mock_server(Vec::new());
        let (_tempdir, _workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let connection = open_connection(&database_path).expect("open db");
        let state = stage_state(&database_path, "incoming");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'queued', updated_at = ?1 WHERE id = ?2",
                rusqlite::params![(Utc::now() - Duration::hours(1)).to_rfc3339(), state.id],
            )
            .expect("stale queued");
        let input = stage_run_input("orphan-before-start", 1);
        crate::database::insert_stage_run(&connection, &input).expect("seed orphan run");

        let reconciled = reconcile_stuck_tasks(&database_path, 1).expect("reconcile");
        let after = stage_state(&database_path, "incoming");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");
        let events = crate::database::list_app_events(&database_path, 50).expect("events");

        assert_eq!(reconciled, 1);
        assert_eq!(after.status, "pending");
        assert_eq!(after.attempts, 0);
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].success);
        assert_eq!(
            runs[0].error_type.as_deref(),
            Some("claim_recovered_before_start")
        );
        assert!(runs[0].finished_at.is_some());
        assert_eq!(runs[0].duration_ms, Some(0));
        assert!(events
            .iter()
            .any(|event| event.code == "orphan_stage_run_reconciled"));
    }

    #[test]
    fn next_run_after_orphan_recovery_sends_one_http_request_and_keeps_clear_audit_history() {
        let server = mock_server(vec![(
            200,
            r#"{"success":true,"payload":{"title":"actual run"},"meta":{}}"#,
        )]);
        let (_tempdir, workdir, database_path, _source_path) = prepare_workdir(&server.url, 3, 0);
        let connection = open_connection(&database_path).expect("open db");
        let state = stage_state(&database_path, "incoming");
        connection
            .execute(
                "UPDATE entity_stage_states SET status = 'queued', updated_at = ?1 WHERE id = ?2",
                rusqlite::params![(Utc::now() - Duration::hours(1)).to_rfc3339(), state.id],
            )
            .expect("stale queued");
        let input = stage_run_input("orphan-before-start", 1);
        crate::database::insert_stage_run(&connection, &input).expect("seed orphan run");

        reconcile_stuck_tasks(&database_path, 1).expect("reconcile");
        let summary = run_due_tasks(&workdir, &database_path, 1, 5, 1, 0).expect("run due");
        let runs = list_stage_runs(&database_path, Some("entity-1")).expect("runs");

        assert_eq!(summary.succeeded, 1);
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs.iter()
                .filter(|run| run.error_type.as_deref() == Some("claim_recovered_before_start"))
                .count(),
            1
        );
        assert_eq!(runs.iter().filter(|run| run.success).count(), 1);
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
            storage: None,
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
            storage: None,
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
    fn output_without_route_is_not_silently_lost() {
        let server = mock_server(vec![(200, r#"[{"entity_name":"terminal child"}]"#)]);
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
                storage: None,
                runtime: RuntimeConfig::default(),
                stages: vec![StageDefinition {
                    id: "incoming".to_string(),
                    input_folder: "stages/incoming".to_string(),
                    input_uri: None,
                    output_folder: String::new(),
                    workflow_url: server.url.clone(),
                    max_attempts: 3,
                    retry_delay_sec: 0,
                    next_stage: None,
                    save_path_aliases: Vec::new(),
                    allow_empty_outputs: false,
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

        assert_eq!(summary.blocked, 1);
        assert_eq!(state.status, "blocked");
        assert!(state.created_child_path.is_none());
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].success);
        assert_eq!(runs[0].error_type.as_deref(), Some("copy_blocked"));
        assert_eq!(server.requests.lock().expect("requests").len(), 1);
    }
}

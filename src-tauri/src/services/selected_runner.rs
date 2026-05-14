use std::collections::{HashMap, HashSet, VecDeque};

use crate::database;
use crate::domain::{
    CommandErrorInfo, EntityFileRecord, RunDueTasksSummary, RunSelectedPipelineWavesRequest,
    RunSelectedPipelineWavesSummary, SelectedPipelineOutputNode, SelectedPipelineRootResult,
    SelectedPipelineWaveSummary, StageRunOutputArtifact, StageRunRecord, StorageProvider,
};
use crate::executor;
use crate::services::{artifacts, runtime};

#[derive(Debug, Clone)]
struct FrontierItem {
    entity_file_id: i64,
    root_entity_file_id: i64,
}

pub(crate) fn run_selected_pipeline_waves(
    workspace_id: &str,
    input: &RunSelectedPipelineWavesRequest,
) -> Result<RunSelectedPipelineWavesSummary, String> {
    log_selected_batch(
        "selected_batch_started",
        workspace_id,
        input.root_entity_file_ids.len(),
        input.max_waves.unwrap_or(5),
        input.max_tasks_per_wave.unwrap_or(3),
    );
    let context = runtime::load_workspace_context(workspace_id)?;
    let summary = run_selected_pipeline_waves_with_context(&context, input)?;
    log_selected_batch_finished(workspace_id, &summary);
    Ok(summary)
}

pub(crate) fn run_selected_pipeline_waves_with_context(
    context: &runtime::WorkspaceRuntimeContext,
    input: &RunSelectedPipelineWavesRequest,
) -> Result<RunSelectedPipelineWavesSummary, String> {
    let requested_max_waves = input.max_waves.unwrap_or(5);
    let requested_max_tasks_per_wave = input.max_tasks_per_wave.unwrap_or(3);
    let max_waves = requested_max_waves.clamp(1, 10);
    let max_tasks_per_wave = requested_max_tasks_per_wave.clamp(1, 5);
    let stop_on_first_failure = input.stop_on_first_failure.unwrap_or(true);
    let root_ids = validate_root_ids(&input.root_entity_file_ids)?;
    let root_files = load_and_validate_roots(&context.database_path, &root_ids)?;

    let mut root_results = root_files
        .iter()
        .map(|file| SelectedPipelineRootResult {
            root_entity_file_id: file.id,
            entity_id: file.entity_id.clone(),
            stage_id: file.stage_id.clone(),
            artifact_id: file.artifact_id.clone(),
            bucket: file.bucket.clone(),
            key: file.key.clone(),
            s3_uri: s3_uri(file),
            status_before: file.status.clone(),
            status_after: None,
            run_ids: Vec::new(),
            output_count: 0,
            errors: Vec::new(),
        })
        .collect::<Vec<_>>();

    let mut frontier = root_files
        .iter()
        .map(|file| FrontierItem {
            entity_file_id: file.id,
            root_entity_file_id: file.id,
        })
        .collect::<VecDeque<_>>();

    let mut aggregate = RunSelectedPipelineWavesSummary {
        root_entity_file_ids: root_ids.clone(),
        requested_max_waves,
        requested_max_tasks_per_wave,
        max_waves,
        max_tasks_per_wave,
        stop_on_first_failure,
        waves_executed: 0,
        stopped_reason: "max_waves_reached".to_string(),
        total_claimed: 0,
        total_succeeded: 0,
        total_failed: 0,
        total_blocked: 0,
        total_retry_scheduled: 0,
        total_skipped: 0,
        total_errors: 0,
        root_results: Vec::new(),
        wave_summaries: Vec::new(),
        output_tree: Vec::new(),
        errors: Vec::new(),
    };

    for wave_index in 1..=max_waves {
        if frontier.is_empty() {
            aggregate.stopped_reason = "idle".to_string();
            break;
        }

        let mut wave_inputs = Vec::new();
        for _ in 0..max_tasks_per_wave {
            let Some(item) = frontier.pop_front() else {
                break;
            };
            wave_inputs.push(item);
        }

        let mut wave_summary = RunDueTasksSummary::default();
        let mut wave_run_ids = Vec::new();
        let mut next_items = Vec::new();
        let mut wave_output_count = 0_u64;

        for item in &wave_inputs {
            let files = database::list_entity_files(&context.database_path, None)?;
            let Some(file) = files.iter().find(|file| file.id == item.entity_file_id) else {
                let error = command_error(
                    "selected_file_missing",
                    format!(
                        "Selected entity_file_id '{}' disappeared before execution.",
                        item.entity_file_id
                    ),
                );
                wave_summary.errors.push(error.clone());
                push_root_error(&mut root_results, item.root_entity_file_id, error);
                continue;
            };
            if !is_runnable_status(&file.status) {
                wave_summary.skipped += 1;
                continue;
            }

            let before_run_ids = stage_run_ids(&context.database_path, &file.entity_id)?;
            let summary = executor::run_entity_stage(
                &context.workdir_path,
                &context.database_path,
                &file.entity_id,
                &file.stage_id,
                context.config.runtime.request_timeout_sec,
                context.config.runtime.stuck_task_timeout_sec,
                context.config.runtime.file_stability_delay_ms,
            )?;
            merge_due_summary(&mut wave_summary, &summary);

            let after_runs =
                database::list_stage_runs(&context.database_path, Some(&file.entity_id))?;
            let new_runs = new_stage_runs_for_file(&after_runs, &before_run_ids, file);
            for run in new_runs {
                wave_run_ids.push(run.run_id.clone());
                push_root_run_id(&mut root_results, item.root_entity_file_id, &run.run_id);
                let outputs =
                    artifacts::list_stage_run_outputs(&context.database_path, &run.run_id)?;
                wave_output_count += outputs.output_count;
                for output in outputs.outputs {
                    push_root_output_count(&mut root_results, item.root_entity_file_id);
                    aggregate.output_tree.push(output_node(
                        item.root_entity_file_id,
                        item.entity_file_id,
                        &output,
                    ));
                    if let Some(child_file) =
                        file_by_id(&context.database_path, output.entity_file_id)?
                    {
                        if is_runnable_status(&child_file.status) {
                            next_items.push(FrontierItem {
                                entity_file_id: child_file.id,
                                root_entity_file_id: item.root_entity_file_id,
                            });
                        }
                    }
                }
            }
        }

        let failed_or_blocked =
            wave_summary.failed > 0 || wave_summary.blocked > 0 || !wave_summary.errors.is_empty();
        aggregate.waves_executed += 1;
        aggregate.total_claimed += wave_summary.claimed;
        aggregate.total_succeeded += wave_summary.succeeded;
        aggregate.total_failed += wave_summary.failed;
        aggregate.total_blocked += wave_summary.blocked;
        aggregate.total_retry_scheduled += wave_summary.retry_scheduled;
        aggregate.total_skipped += wave_summary.skipped;
        aggregate.total_errors += wave_summary.errors.len() as u64;
        aggregate.wave_summaries.push(SelectedPipelineWaveSummary {
            wave_index,
            input_entity_file_ids: wave_inputs.iter().map(|item| item.entity_file_id).collect(),
            run_ids: wave_run_ids,
            summary: wave_summary,
            output_count: wave_output_count,
        });

        for item in next_items {
            frontier.push_back(item);
        }

        if aggregate
            .wave_summaries
            .last()
            .is_some_and(|wave| wave.summary.claimed == 0)
        {
            aggregate.stopped_reason = "idle".to_string();
            break;
        }
        if stop_on_first_failure && failed_or_blocked {
            aggregate.stopped_reason = "failure_or_blocked".to_string();
            break;
        }
        if frontier.is_empty() {
            aggregate.stopped_reason = "idle".to_string();
            break;
        }
    }

    refresh_root_statuses(&context.database_path, &mut root_results)?;
    aggregate.root_results = root_results;
    Ok(aggregate)
}

fn validate_root_ids(ids: &[i64]) -> Result<Vec<i64>, String> {
    if ids.is_empty() {
        return Err("root_entity_file_ids must contain at least one id.".to_string());
    }
    if ids.len() > 10 {
        return Err("root_entity_file_ids may contain at most 10 ids for B7.".to_string());
    }
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for id in ids {
        if *id <= 0 {
            return Err(format!("entity_file_id '{id}' must be positive."));
        }
        if !seen.insert(*id) {
            return Err(format!(
                "entity_file_id '{id}' was selected more than once."
            ));
        }
        normalized.push(*id);
    }
    Ok(normalized)
}

fn load_and_validate_roots(
    database_path: &std::path::Path,
    root_ids: &[i64],
) -> Result<Vec<EntityFileRecord>, String> {
    let files = database::list_entity_files(database_path, None)?;
    let by_id = files
        .into_iter()
        .map(|file| (file.id, file))
        .collect::<HashMap<_, _>>();
    let mut roots = Vec::new();
    for id in root_ids {
        let file = by_id
            .get(id)
            .ok_or_else(|| format!("entity_file_id '{id}' is not present in this workspace."))?;
        if file.storage_provider != StorageProvider::S3 {
            return Err(format!(
                "entity_file_id '{}' is not an S3 artifact and cannot be used for the B7 S3 web pilot.",
                file.id
            ));
        }
        if !file.file_exists {
            return Err(format!(
                "entity_file_id '{}' is missing and cannot be selected.",
                file.id
            ));
        }
        let status =
            database::get_stage_state_status(database_path, &file.entity_id, &file.stage_id)?
                .ok_or_else(|| {
                    format!(
                        "entity_file_id '{}' has no runtime state for entity '{}' stage '{}'.",
                        file.id, file.entity_id, file.stage_id
                    )
                })?;
        if !is_runnable_status(&status) {
            return Err(format!(
                "entity_file_id '{}' has status '{}' and must be reset before selected execution.",
                file.id, status
            ));
        }
        roots.push(file.clone());
    }
    Ok(roots)
}

fn is_runnable_status(status: &str) -> bool {
    matches!(status, "pending" | "retry_wait")
}

fn stage_run_ids(
    database_path: &std::path::Path,
    entity_id: &str,
) -> Result<HashSet<String>, String> {
    Ok(database::list_stage_runs(database_path, Some(entity_id))?
        .into_iter()
        .map(|run| run.run_id)
        .collect())
}

fn new_stage_runs_for_file(
    after_runs: &[StageRunRecord],
    before_run_ids: &HashSet<String>,
    file: &EntityFileRecord,
) -> Vec<StageRunRecord> {
    let mut runs = after_runs
        .iter()
        .filter(|run| {
            !before_run_ids.contains(&run.run_id)
                && run.stage_id == file.stage_id
                && run.entity_file_id == Some(file.id)
        })
        .cloned()
        .collect::<Vec<_>>();
    runs.sort_by(|left, right| left.id.cmp(&right.id));
    runs
}

fn file_by_id(
    database_path: &std::path::Path,
    entity_file_id: i64,
) -> Result<Option<EntityFileRecord>, String> {
    Ok(database::list_entity_files(database_path, None)?
        .into_iter()
        .find(|file| file.id == entity_file_id))
}

fn output_node(
    root_entity_file_id: i64,
    source_entity_file_id: i64,
    output: &StageRunOutputArtifact,
) -> SelectedPipelineOutputNode {
    SelectedPipelineOutputNode {
        root_entity_file_id,
        source_entity_file_id,
        producer_run_id: output.producer_run_id.clone(),
        entity_file_id: output.entity_file_id,
        entity_id: output.entity_id.clone(),
        artifact_id: output.artifact_id.clone(),
        target_stage_id: output.target_stage_id.clone(),
        relation_to_source: output.relation_to_source.clone(),
        storage_provider: output.storage_provider.clone(),
        bucket: output.bucket.clone(),
        key: output.key.clone(),
        s3_uri: output.s3_uri.clone(),
        size: output.size,
        runtime_status: output.runtime_status.clone(),
    }
}

fn merge_due_summary(total: &mut RunDueTasksSummary, summary: &RunDueTasksSummary) {
    total.claimed += summary.claimed;
    total.succeeded += summary.succeeded;
    total.retry_scheduled += summary.retry_scheduled;
    total.failed += summary.failed;
    total.blocked += summary.blocked;
    total.skipped += summary.skipped;
    total.stuck_reconciled += summary.stuck_reconciled;
    total.errors.extend(summary.errors.clone());
}

fn push_root_run_id(
    roots: &mut [SelectedPipelineRootResult],
    root_entity_file_id: i64,
    run_id: &str,
) {
    if let Some(root) = roots
        .iter_mut()
        .find(|root| root.root_entity_file_id == root_entity_file_id)
    {
        root.run_ids.push(run_id.to_string());
    }
}

fn push_root_output_count(roots: &mut [SelectedPipelineRootResult], root_entity_file_id: i64) {
    if let Some(root) = roots
        .iter_mut()
        .find(|root| root.root_entity_file_id == root_entity_file_id)
    {
        root.output_count += 1;
    }
}

fn push_root_error(
    roots: &mut [SelectedPipelineRootResult],
    root_entity_file_id: i64,
    error: CommandErrorInfo,
) {
    if let Some(root) = roots
        .iter_mut()
        .find(|root| root.root_entity_file_id == root_entity_file_id)
    {
        root.errors.push(error);
    }
}

fn refresh_root_statuses(
    database_path: &std::path::Path,
    roots: &mut [SelectedPipelineRootResult],
) -> Result<(), String> {
    for root in roots {
        root.status_after =
            database::get_stage_state_status(database_path, &root.entity_id, &root.stage_id)?;
    }
    Ok(())
}

fn s3_uri(file: &EntityFileRecord) -> Option<String> {
    match (file.bucket.as_deref(), file.key.as_deref()) {
        (Some(bucket), Some(key)) => Some(format!("s3://{bucket}/{key}")),
        _ => None,
    }
}

fn command_error(code: &str, message: impl Into<String>) -> CommandErrorInfo {
    CommandErrorInfo {
        code: code.to_string(),
        message: message.into(),
        path: None,
    }
}

fn log_selected_batch(
    event: &str,
    workspace_id: &str,
    root_count: usize,
    max_waves: u64,
    max_tasks_per_wave: u64,
) {
    println!(
        "{}",
        serde_json::json!({
            "event": event,
            "ts": chrono::Utc::now().to_rfc3339(),
            "payload": {
                "workspace_id": workspace_id,
                "root_count": root_count,
                "max_waves": max_waves,
                "max_tasks_per_wave": max_tasks_per_wave,
            }
        })
    );
}

fn log_selected_batch_finished(workspace_id: &str, summary: &RunSelectedPipelineWavesSummary) {
    println!(
        "{}",
        serde_json::json!({
            "event": "selected_batch_finished",
            "ts": chrono::Utc::now().to_rfc3339(),
            "payload": {
                "workspace_id": workspace_id,
                "root_count": summary.root_entity_file_ids.len(),
                "waves_executed": summary.waves_executed,
                "stopped_reason": summary.stopped_reason,
                "total_claimed": summary.total_claimed,
                "total_succeeded": summary.total_succeeded,
                "total_failed": summary.total_failed,
                "total_blocked": summary.total_blocked,
                "output_count": summary.output_tree.len(),
            }
        })
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use chrono::Utc;
    use serde_json::Value;

    use crate::database::{
        bootstrap_database, list_entity_files, register_s3_artifact_pointer,
        RegisterS3ArtifactPointerInput,
    };
    use crate::domain::{
        PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition, StageStatus, StorageConfig,
    };

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

    fn s3_config(workflow_url: &str) -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive-selected-test".to_string(),
                workdir: ".".to_string(),
            },
            storage: Some(StorageConfig {
                provider: StorageProvider::S3,
                bucket: Some("bucket".to_string()),
                workspace_prefix: Some("prefix".to_string()),
                region: None,
                endpoint: None,
            }),
            runtime: RuntimeConfig {
                scan_interval_sec: 5,
                max_parallel_tasks: 3,
                stuck_task_timeout_sec: 1,
                request_timeout_sec: 5,
                file_stability_delay_ms: 0,
            },
            stages: vec![
                StageDefinition {
                    id: "raw".to_string(),
                    input_folder: String::new(),
                    input_uri: Some("s3://bucket/prefix/raw".to_string()),
                    output_folder: String::new(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts: 1,
                    retry_delay_sec: 0,
                    next_stage: Some("processed".to_string()),
                    save_path_aliases: vec!["prefix/raw".to_string()],
                    allow_empty_outputs: false,
                },
                StageDefinition {
                    id: "processed".to_string(),
                    input_folder: String::new(),
                    input_uri: Some("s3://bucket/prefix/processed".to_string()),
                    output_folder: String::new(),
                    workflow_url: workflow_url.to_string(),
                    max_attempts: 1,
                    retry_delay_sec: 0,
                    next_stage: None,
                    save_path_aliases: vec![
                        "prefix/processed".to_string(),
                        "/prefix/processed".to_string(),
                        "s3://bucket/prefix/processed".to_string(),
                    ],
                    allow_empty_outputs: true,
                },
            ],
        }
    }

    fn setup_context(workflow_url: &str) -> (tempfile::TempDir, runtime::WorkspaceRuntimeContext) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let config = s3_config(workflow_url);
        bootstrap_database(&database_path, &config).expect("bootstrap");
        (
            tempdir,
            runtime::WorkspaceRuntimeContext {
                workdir_path: workdir,
                database_path,
                config,
            },
        )
    }

    fn register_source(
        database_path: &std::path::Path,
        entity_id: &str,
        key: &str,
    ) -> EntityFileRecord {
        register_s3_artifact_pointer(
            database_path,
            &RegisterS3ArtifactPointerInput {
                entity_id: entity_id.to_string(),
                artifact_id: format!("{entity_id}-source"),
                relation_to_source: None,
                stage_id: "raw".to_string(),
                bucket: "bucket".to_string(),
                key: key.to_string(),
                version_id: None,
                etag: Some(format!("{entity_id}-etag")),
                checksum_sha256: None,
                size: Some(100),
                last_modified: Some(Utc::now().to_rfc3339()),
                source_file_id: None,
                producer_run_id: None,
                status: StageStatus::Pending,
            },
        )
        .expect("register source")
    }

    fn request_body(request: &str) -> &str {
        request.split("\r\n\r\n").nth(1).unwrap_or_default()
    }

    fn manifest_for_request(request: &str) -> String {
        let body: Value = serde_json::from_str(request_body(request)).expect("body");
        let run_id = body["run_id"].as_str().expect("run_id");
        let stage_id = body["stage_id"].as_str().expect("stage_id");
        let source_key = body["source_key"].as_str().expect("source_key");
        if stage_id == "raw" {
            format!(
                r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-selected-test",
  "run_id":"{run_id}",
  "source":{{"bucket":"bucket","key":"{source_key}","version_id":null,"etag":null}},
  "status":"success",
  "outputs":[
    {{"artifact_id":"child-a","entity_id":"{run_id}-child-a","relation_to_source":"child_entity","bucket":"bucket","key":"prefix/processed/{run_id}-a.json","save_path":"prefix/processed","content_type":"application/json","checksum_sha256":null,"size":11}},
    {{"artifact_id":"child-b","entity_id":"{run_id}-child-b","relation_to_source":"child_entity","bucket":"bucket","key":"prefix/processed/{run_id}-b.json","save_path":"prefix/processed","content_type":"application/json","checksum_sha256":null,"size":12}}
  ],
  "created_at":"2026-05-14T00:00:00Z"
}}"#
            )
        } else {
            format!(
                r#"{{
  "schema":"beehive.s3_artifact_manifest.v1",
  "workspace_id":"beehive-selected-test",
  "run_id":"{run_id}",
  "source":{{"bucket":"bucket","key":"{source_key}","version_id":null,"etag":null}},
  "status":"success",
  "outputs":[],
  "created_at":"2026-05-14T00:00:00Z"
}}"#
            )
        }
    }

    #[test]
    fn selected_runner_rejects_missing_roots() {
        let server = mock_server_dynamic(0, |_| (200, "{}".to_string()));
        let (_tempdir, context) = setup_context(&server.url);

        let result = run_selected_pipeline_waves_with_context(
            &context,
            &RunSelectedPipelineWavesRequest {
                root_entity_file_ids: vec![999],
                max_waves: Some(1),
                max_tasks_per_wave: Some(1),
                stop_on_first_failure: Some(true),
            },
        );

        assert!(result.is_err());
        assert!(result
            .expect_err("missing root")
            .contains("entity_file_id '999'"));
    }

    #[test]
    fn selected_runner_executes_only_selected_root_and_not_unrelated_pending_source() {
        let server = mock_server_dynamic(1, |request| (200, manifest_for_request(request)));
        let (_tempdir, context) = setup_context(&server.url);
        let selected = register_source(
            &context.database_path,
            "selected-entity",
            "prefix/raw/selected.json",
        );
        let unrelated = register_source(
            &context.database_path,
            "unrelated-entity",
            "prefix/raw/unrelated.json",
        );

        let summary = run_selected_pipeline_waves_with_context(
            &context,
            &RunSelectedPipelineWavesRequest {
                root_entity_file_ids: vec![selected.id],
                max_waves: Some(1),
                max_tasks_per_wave: Some(1),
                stop_on_first_failure: Some(true),
            },
        )
        .expect("selected run");

        assert_eq!(summary.total_claimed, 1);
        assert_eq!(summary.total_succeeded, 1);
        assert_eq!(summary.output_tree.len(), 2);
        assert_eq!(
            database::get_stage_state_status(&context.database_path, "selected-entity", "raw")
                .expect("selected status")
                .as_deref(),
            Some("done")
        );
        assert_eq!(
            database::get_stage_state_status(&context.database_path, "unrelated-entity", "raw")
                .expect("unrelated status")
                .as_deref(),
            Some("pending")
        );
        let request_count = server.requests.lock().expect("requests").len();
        assert_eq!(request_count, 1);
        assert!(list_entity_files(&context.database_path, None)
            .expect("files")
            .iter()
            .any(|file| file.id == unrelated.id && file.status == "pending"));
    }

    #[test]
    fn selected_runner_follows_two_child_outputs_without_global_queue() {
        let server = mock_server_dynamic(3, |request| (200, manifest_for_request(request)));
        let (_tempdir, context) = setup_context(&server.url);
        let selected = register_source(
            &context.database_path,
            "selected-entity",
            "prefix/raw/selected.json",
        );

        let summary = run_selected_pipeline_waves_with_context(
            &context,
            &RunSelectedPipelineWavesRequest {
                root_entity_file_ids: vec![selected.id],
                max_waves: Some(2),
                max_tasks_per_wave: Some(5),
                stop_on_first_failure: Some(true),
            },
        )
        .expect("selected run");

        assert_eq!(summary.waves_executed, 2);
        assert_eq!(summary.total_claimed, 3);
        assert_eq!(summary.total_succeeded, 3);
        assert_eq!(summary.output_tree.len(), 2);
        assert_eq!(summary.root_results[0].output_count, 2);
        assert_eq!(summary.wave_summaries[0].output_count, 2);
        assert_eq!(summary.wave_summaries[1].input_entity_file_ids.len(), 2);
        assert_eq!(server.requests.lock().expect("requests").len(), 3);
    }
}

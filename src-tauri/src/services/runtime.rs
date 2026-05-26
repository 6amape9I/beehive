use std::path::PathBuf;

use crate::bootstrap;
use crate::config;
use crate::database;
use crate::domain::{
    BootstrapResult, PipelineConfig, RegisterS3SourceArtifactPayload,
    RegisterS3SourceArtifactRequest, RegisterS3SourceArtifactResult, RunDueTasksSummary,
    RunPipelineWavesSummary, S3ReconciliationSummary, WorkspaceExplorerResult,
};
use crate::executor;
use crate::s3_reconciliation;
use crate::services::workers;
use crate::services::workspaces::get_workspace;
use crate::workdir;

#[derive(Debug)]
pub(crate) struct WorkspaceRuntimeContext {
    pub(crate) workdir_path: PathBuf,
    pub(crate) database_path: PathBuf,
    pub(crate) config: PipelineConfig,
}

pub(crate) fn open_registered_workspace(workspace_id: &str) -> Result<BootstrapResult, String> {
    let workspace = get_workspace(workspace_id)?;
    let path = workspace.workdir_path.to_string_lossy().to_string();
    let mut result = bootstrap::open_workdir(&path);
    result.state.selected_workspace_id = Some(workspace.id);
    Ok(result)
}

pub(crate) fn load_workspace_context(
    workspace_id: &str,
) -> Result<WorkspaceRuntimeContext, String> {
    let (workdir_path, database_path, config) = load_workspace_context_parts(workspace_id)?;
    database::bootstrap_database(&database_path, &config)?;

    Ok(WorkspaceRuntimeContext {
        workdir_path,
        database_path,
        config,
    })
}

pub(crate) fn load_worker_runtime_context(
    workspace_id: &str,
) -> Result<WorkspaceRuntimeContext, String> {
    let (workdir_path, database_path, config) = load_workspace_context_parts(workspace_id)?;
    database::verify_worker_runtime_database(&database_path)?;

    Ok(WorkspaceRuntimeContext {
        workdir_path,
        database_path,
        config,
    })
}

fn load_workspace_context_parts(
    workspace_id: &str,
) -> Result<(PathBuf, PathBuf, PipelineConfig), String> {
    let workspace = get_workspace(workspace_id)?;
    let workdir_path = workdir::resolve_user_path(&workspace.workdir_path.to_string_lossy())?;
    let workdir_state = workdir::inspect(&workdir_path, false);

    if !workdir_state.exists {
        return Err(format!(
            "Registered workspace '{}' workdir does not exist: {}",
            workspace.id, workdir_state.workdir_path
        ));
    }
    if !workdir_state.pipeline_config_exists {
        return Err(format!(
            "Registered workspace '{}' has no pipeline.yaml at {}",
            workspace.id, workdir_state.pipeline_config_path
        ));
    }

    let loaded = config::load_pipeline_config(&workspace.pipeline_path);
    if !loaded.validation.is_valid {
        return Err(format!(
            "Registered workspace '{}' pipeline.yaml is invalid.",
            workspace.id
        ));
    }
    let config = loaded.config.ok_or_else(|| {
        format!(
            "Registered workspace '{}' pipeline.yaml could not be converted into runtime config.",
            workspace.id
        )
    })?;
    Ok((workdir_path, workspace.database_path, config))
}

pub(crate) fn workspace_explorer(workspace_id: &str) -> Result<WorkspaceExplorerResult, String> {
    let context = load_workspace_context(workspace_id)?;
    database::get_workspace_explorer(&context.workdir_path, &context.database_path)
}

pub(crate) fn reconcile_s3_workspace(
    workspace_id: &str,
) -> Result<S3ReconciliationSummary, String> {
    let context = load_workspace_context(workspace_id)?;
    s3_reconciliation::reconcile_s3_workspace(&context.database_path, &context.config)
}

pub(crate) fn register_s3_source_artifact(
    workspace_id: &str,
    input: &RegisterS3SourceArtifactRequest,
) -> RegisterS3SourceArtifactResult {
    match load_workspace_context(workspace_id) {
        Ok(context) => {
            match s3_reconciliation::register_s3_source_artifact(&context.database_path, input) {
                Ok(file) => RegisterS3SourceArtifactResult {
                    payload: Some(RegisterS3SourceArtifactPayload { file }),
                    errors: Vec::new(),
                },
                Err(message) => RegisterS3SourceArtifactResult {
                    payload: None,
                    errors: vec![command_error("register_s3_source_artifact_failed", message)],
                },
            }
        }
        Err(message) => RegisterS3SourceArtifactResult {
            payload: None,
            errors: vec![command_error("workspace_runtime_failed", message)],
        },
    }
}

pub(crate) fn run_small_batch(
    workspace_id: &str,
    max_tasks: u64,
) -> Result<RunDueTasksSummary, String> {
    ensure_broad_run_allowed(workspace_id)?;
    let context = load_workspace_context(workspace_id)?;
    executor::run_due_tasks(
        &context.workdir_path,
        &context.database_path,
        max_tasks.clamp(1, 5),
        context.config.runtime.request_timeout_sec,
        context.config.runtime.stuck_task_timeout_sec,
        context.config.runtime.file_stability_delay_ms,
    )
}

pub(crate) fn run_pipeline_waves(
    workspace_id: &str,
    max_waves: u64,
    max_tasks_per_wave: u64,
    stop_on_first_failure: bool,
) -> Result<RunPipelineWavesSummary, String> {
    ensure_broad_run_allowed(workspace_id)?;
    let context = load_workspace_context(workspace_id)?;
    executor::run_pipeline_waves(
        &context.workdir_path,
        &context.database_path,
        max_waves,
        max_tasks_per_wave,
        stop_on_first_failure,
        context.config.runtime.request_timeout_sec,
        context.config.runtime.stuck_task_timeout_sec,
        context.config.runtime.file_stability_delay_ms,
    )
}

fn ensure_broad_run_allowed(workspace_id: &str) -> Result<(), String> {
    if workers::workspace_workers_enabled(workspace_id) {
        return Err(workers::BROAD_RUN_DISABLED_MESSAGE.to_string());
    }
    Ok(())
}

fn command_error(code: &str, message: impl Into<String>) -> crate::domain::CommandErrorInfo {
    crate::domain::CommandErrorInfo {
        code: code.to_string(),
        message: message.into(),
        path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::domain::WorkerStartRequest;
    use crate::services::{workers, workspaces};
    use rusqlite::Connection;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;

    fn with_test_env<F>(registry_path: &Path, root: &Path, run: F)
    where
        F: FnOnce(),
    {
        let _guard = workspaces::env_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_registry = std::env::var_os("BEEHIVE_WORKSPACES_CONFIG");
        let previous_root = std::env::var_os("BEEHIVE_WORKSPACES_ROOT");
        std::env::set_var("BEEHIVE_WORKSPACES_CONFIG", registry_path);
        std::env::set_var("BEEHIVE_WORKSPACES_ROOT", root);
        run();
        restore_env_var("BEEHIVE_WORKSPACES_CONFIG", previous_registry.as_deref());
        restore_env_var("BEEHIVE_WORKSPACES_ROOT", previous_root.as_deref());
    }

    fn restore_env_var(name: &str, value: Option<&OsStr>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn write_registry(registry_path: &Path, workdir: &Path) {
        fs::write(
            registry_path,
            format!(
                r#"
workspaces:
  - id: smoke
    name: Smoke
    provider: s3
    bucket: steos-s3-data
    workspace_prefix: smoke
    region: ru-1
    endpoint: https://s3.example
    workdir_path: {}
    pipeline_path: {}
    database_path: {}
"#,
                workdir.display(),
                workdir.join("pipeline.yaml").display(),
                workdir.join("app.db").display()
            ),
        )
        .expect("registry");
    }

    fn write_pipeline(workdir: &Path, workflow_url: &str) {
        fs::create_dir_all(workdir).expect("workdir");
        fs::write(
            workdir.join("pipeline.yaml"),
            format!(
                r#"
project:
  name: smoke
  workdir: .
runtime:
  request_timeout_sec: 5
  worker_pools:
    default:
      concurrency: 1
    local_llm:
      concurrency: 0
stages:
  - id: stage_0
    input_folder: stages/stage_0
    workflow_url: {workflow_url}
"#
            ),
        )
        .expect("pipeline");
    }

    fn stage_workflow_url(database_path: &Path) -> String {
        database::list_stages(database_path)
            .expect("stages")
            .into_iter()
            .find(|stage| stage.id == "stage_0")
            .expect("stage_0")
            .workflow_url
    }

    #[test]
    fn worker_runtime_context_requires_bootstrapped_database() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path().join("root");
        let workdir = root.join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        write_pipeline(&workdir, "http://127.0.0.1:9999/webhook");
        write_registry(&registry_path, &workdir);

        with_test_env(&registry_path, &root, || {
            let error = load_worker_runtime_context("smoke").expect_err("missing db");
            assert!(error.contains("workspace_not_bootstrapped_for_workers"));
        });
    }

    #[test]
    fn worker_runtime_context_does_not_sync_stage_rows() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path().join("root");
        let workdir = root.join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        let old_url = "http://127.0.0.1:9001/old";
        let new_url = "http://127.0.0.1:9001/new";
        write_pipeline(&workdir, old_url);
        write_registry(&registry_path, &workdir);

        with_test_env(&registry_path, &root, || {
            let heavy = load_workspace_context("smoke").expect("heavy context");
            assert_eq!(stage_workflow_url(&heavy.database_path), old_url);

            write_pipeline(&workdir, new_url);
            let light = load_worker_runtime_context("smoke").expect("light context");
            assert_eq!(light.config.stages[0].workflow_url, new_url);
            assert_eq!(stage_workflow_url(&heavy.database_path), old_url);

            load_workspace_context("smoke").expect("heavy resync");
            assert_eq!(stage_workflow_url(&heavy.database_path), new_url);
        });
    }

    #[test]
    fn worker_summary_and_start_are_read_mostly_for_stage_rows() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path().join("root");
        let workdir = root.join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        let old_url = "http://127.0.0.1:9002/old";
        let new_url = "http://127.0.0.1:9002/new";
        write_pipeline(&workdir, old_url);
        write_registry(&registry_path, &workdir);

        with_test_env(&registry_path, &root, || {
            let context = load_workspace_context("smoke").expect("heavy context");
            write_pipeline(&workdir, new_url);

            workers::worker_summary("smoke").expect("summary");
            workers::start_workers(
                "smoke",
                &WorkerStartRequest {
                    default_workers: 1,
                    local_llm_workers: 0,
                },
            )
            .expect("start workers");

            assert_eq!(stage_workflow_url(&context.database_path), old_url);
        });
    }

    #[test]
    fn worker_runtime_context_loads_while_write_transaction_is_open() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path().join("root");
        let workdir = root.join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        write_pipeline(&workdir, "http://127.0.0.1:9003/webhook");
        write_registry(&registry_path, &workdir);

        with_test_env(&registry_path, &root, || {
            let context = load_workspace_context("smoke").expect("heavy context");
            let connection = Connection::open(&context.database_path).expect("open db");
            connection
                .execute_batch("BEGIN IMMEDIATE; UPDATE settings SET value = value WHERE key = 'schema_version';")
                .expect("hold write transaction");

            let light = load_worker_runtime_context("smoke").expect("light context under writer");
            assert_eq!(light.database_path, context.database_path);

            connection.execute_batch("ROLLBACK;").expect("rollback");
        });
    }
}

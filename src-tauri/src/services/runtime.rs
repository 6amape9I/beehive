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
use crate::services::workspaces::get_workspace;
use crate::workdir;

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
    database::bootstrap_database(&workspace.database_path, &config)?;

    Ok(WorkspaceRuntimeContext {
        workdir_path,
        database_path: PathBuf::from(workdir_state.database_path),
        config,
    })
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

fn command_error(code: &str, message: impl Into<String>) -> crate::domain::CommandErrorInfo {
    crate::domain::CommandErrorInfo {
        code: code.to_string(),
        message: message.into(),
        path: None,
    }
}

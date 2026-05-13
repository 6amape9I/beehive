use std::path::{Path, PathBuf};

use crate::bootstrap;
use crate::config;
use crate::dashboard;
use crate::database;
use crate::discovery;
use crate::domain::{
    AppEventsResult, BootstrapResult, CommandErrorInfo, ConfigValidationIssue,
    DashboardOverviewResult, EntityDetailResult, EntityFilesResult, EntityListQuery,
    EntityListResult, FileCopyResult, ManualEntityStageActionResult, OpenEntityPathPayload,
    OpenEntityPathResult, PipelineConfigDraft, PipelineEditorStateResult,
    ReconcileStuckTasksResult, RegisterS3SourceArtifactPayload, RegisterS3SourceArtifactRequest,
    RegisterS3SourceArtifactResult, RunDueTasksResult, RunEntityStageResult,
    RunPipelineWavesResult, RuntimeSummaryResult, S3ReconciliationResult, SaveEntityFileJsonResult,
    SavePipelineConfigResult, ScanWorkspaceResult, StageDirectoryProvisionResult, StageListResult,
    StageRunsResult, ValidatePipelineConfigDraftResult, ValidationSeverity,
    WorkspaceExplorerResult, WorkspaceExplorerTotals,
};
use crate::executor;
use crate::file_open::{self, OpenEntityPathKind};
use crate::file_ops;
use crate::pipeline_editor;
use crate::s3_reconciliation;
use crate::workdir;

#[tauri::command]
pub fn initialize_workdir(path: String) -> BootstrapResult {
    bootstrap::initialize_workdir(&path)
}

#[tauri::command]
pub fn open_workdir(path: String) -> BootstrapResult {
    bootstrap::open_workdir(&path)
}

#[tauri::command]
pub fn reload_workdir(path: String) -> BootstrapResult {
    bootstrap::reload_workdir(&path)
}

#[tauri::command]
pub fn get_dashboard_overview(path: String) -> DashboardOverviewResult {
    match load_runtime_context(&path) {
        Ok(context) => match dashboard::get_dashboard_overview(
            &context.database_path,
            &context.config.project.name,
            &context.workdir_path,
        ) {
            Ok(overview) => DashboardOverviewResult {
                overview: Some(overview),
                errors: Vec::new(),
            },
            Err(message) => DashboardOverviewResult {
                overview: None,
                errors: vec![command_error("dashboard_overview_failed", message, None)],
            },
        },
        Err(error) => DashboardOverviewResult {
            overview: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn scan_workspace(path: String) -> ScanWorkspaceResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            match discovery::scan_workspace_with_stability_delay(
                &context.workdir_path,
                &context.database_path,
                context.config.runtime.file_stability_delay_ms,
            ) {
                Ok(summary) => ScanWorkspaceResult {
                    summary: Some(summary),
                    errors: Vec::new(),
                },
                Err(message) => scan_error("scan_workspace_failed", message),
            }
        }
        Err(error) => ScanWorkspaceResult {
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn reconcile_s3_workspace(path: String) -> S3ReconciliationResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            match s3_reconciliation::reconcile_s3_workspace(&context.database_path, &context.config)
            {
                Ok(summary) => S3ReconciliationResult {
                    summary: Some(summary),
                    errors: Vec::new(),
                },
                Err(message) => S3ReconciliationResult {
                    summary: None,
                    errors: vec![command_error("s3_reconciliation_failed", message, None)],
                },
            }
        }
        Err(error) => S3ReconciliationResult {
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn register_s3_source_artifact(
    path: String,
    input: RegisterS3SourceArtifactRequest,
) -> RegisterS3SourceArtifactResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            match s3_reconciliation::register_s3_source_artifact(&context.database_path, &input) {
                Ok(file) => RegisterS3SourceArtifactResult {
                    payload: Some(RegisterS3SourceArtifactPayload { file }),
                    errors: Vec::new(),
                },
                Err(message) => RegisterS3SourceArtifactResult {
                    payload: None,
                    errors: vec![command_error(
                        "register_s3_source_artifact_failed",
                        message,
                        None,
                    )],
                },
            }
        }
        Err(error) => RegisterS3SourceArtifactResult {
            payload: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn ensure_stage_directories(path: String) -> StageDirectoryProvisionResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            match discovery::ensure_stage_directories(&context.workdir_path, &context.database_path)
            {
                Ok(summary) => StageDirectoryProvisionResult {
                    summary: Some(summary),
                    errors: Vec::new(),
                },
                Err(message) => StageDirectoryProvisionResult {
                    summary: None,
                    errors: vec![command_error(
                        "ensure_stage_directories_failed",
                        message,
                        None,
                    )],
                },
            }
        }
        Err(error) => StageDirectoryProvisionResult {
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn get_runtime_summary(path: String) -> RuntimeSummaryResult {
    match load_runtime_context(&path) {
        Ok(context) => match database::get_runtime_summary(&context.database_path) {
            Ok(summary) => RuntimeSummaryResult {
                summary: Some(summary),
                errors: Vec::new(),
            },
            Err(message) => RuntimeSummaryResult {
                summary: None,
                errors: vec![command_error("runtime_summary_failed", message, None)],
            },
        },
        Err(error) => RuntimeSummaryResult {
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn list_stages(path: String) -> StageListResult {
    match load_runtime_context(&path) {
        Ok(context) => match database::list_stages(&context.database_path) {
            Ok(stages) => StageListResult {
                stages,
                errors: Vec::new(),
            },
            Err(message) => StageListResult {
                stages: Vec::new(),
                errors: vec![command_error("list_stages_failed", message, None)],
            },
        },
        Err(error) => StageListResult {
            stages: Vec::new(),
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn get_pipeline_editor_state(path: String) -> PipelineEditorStateResult {
    match pipeline_editor::get_pipeline_editor_state(&path) {
        Ok(state) => PipelineEditorStateResult {
            state: Some(state),
            errors: Vec::new(),
        },
        Err(message) => PipelineEditorStateResult {
            state: None,
            errors: vec![command_error("pipeline_editor_state_failed", message, None)],
        },
    }
}

#[tauri::command]
pub fn validate_pipeline_config_draft(
    path: String,
    draft: PipelineConfigDraft,
) -> ValidatePipelineConfigDraftResult {
    match pipeline_editor::validate_pipeline_config_draft(&path, &draft) {
        Ok(result) => result,
        Err(message) => ValidatePipelineConfigDraftResult {
            validation: crate::domain::ConfigValidationResult::from_issues(vec![
                ConfigValidationIssue {
                    severity: ValidationSeverity::Error,
                    code: "pipeline_draft_validation_failed".to_string(),
                    path: "pipeline".to_string(),
                    message: message.clone(),
                },
            ]),
            normalized_config: None,
            yaml_preview: None,
            stage_usages: Vec::new(),
            errors: vec![command_error(
                "pipeline_draft_validation_failed",
                message,
                None,
            )],
        },
    }
}

#[tauri::command]
pub fn save_pipeline_config(
    path: String,
    draft: PipelineConfigDraft,
    operator_comment: Option<String>,
) -> SavePipelineConfigResult {
    match pipeline_editor::save_pipeline_config(&path, &draft, operator_comment.as_deref()) {
        Ok(result) => result,
        Err(message) => SavePipelineConfigResult {
            state: None,
            backup_path: None,
            errors: vec![command_error("save_pipeline_config_failed", message, None)],
        },
    }
}

#[tauri::command]
pub fn list_entities(path: String, query: Option<EntityListQuery>) -> EntityListResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            let query = query.unwrap_or_default();
            match (
                database::list_entity_table_page(&context.database_path, &query),
                database::list_stages(&context.database_path),
            ) {
                (Ok(page), Ok(stages)) => EntityListResult {
                    total: page.total,
                    page: page.page,
                    page_size: page.page_size,
                    available_stages: stages.into_iter().map(|stage| stage.id).collect(),
                    available_statuses: page.available_statuses,
                    entities: page.entities,
                    errors: Vec::new(),
                },
                (Err(message), _) | (_, Err(message)) => EntityListResult {
                    entities: Vec::new(),
                    total: 0,
                    page: 1,
                    page_size: 50,
                    available_stages: Vec::new(),
                    available_statuses: Vec::new(),
                    errors: vec![command_error("list_entities_failed", message, None)],
                },
            }
        }
        Err(error) => EntityListResult {
            entities: Vec::new(),
            total: 0,
            page: 1,
            page_size: 50,
            available_stages: Vec::new(),
            available_statuses: Vec::new(),
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn list_entity_files(path: String, entity_id: Option<String>) -> EntityFilesResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            match database::list_entity_files(&context.database_path, entity_id.as_deref()) {
                Ok(files) => EntityFilesResult {
                    files,
                    errors: Vec::new(),
                },
                Err(message) => EntityFilesResult {
                    files: Vec::new(),
                    errors: vec![command_error("list_entity_files_failed", message, None)],
                },
            }
        }
        Err(error) => EntityFilesResult {
            files: Vec::new(),
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn get_entity(
    path: String,
    entity_id: String,
    selected_file_id: Option<i64>,
) -> EntityDetailResult {
    match load_runtime_context(&path) {
        Ok(context) => match database::get_entity_detail_with_selection(
            &context.database_path,
            &entity_id,
            selected_file_id,
        ) {
            Ok(Some(detail)) => EntityDetailResult {
                detail: Some(detail),
                errors: Vec::new(),
            },
            Ok(None) => EntityDetailResult {
                detail: None,
                errors: vec![command_error(
                    "entity_not_found",
                    format!("Entity '{entity_id}' was not found."),
                    None,
                )],
            },
            Err(message) => EntityDetailResult {
                detail: None,
                errors: vec![command_error("get_entity_failed", message, None)],
            },
        },
        Err(error) => EntityDetailResult {
            detail: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn create_next_stage_copy(
    path: String,
    entity_id: String,
    source_stage_id: String,
) -> FileCopyResult {
    match load_runtime_context(&path) {
        Ok(context) => match file_ops::create_next_stage_copy(
            &context.workdir_path,
            &context.database_path,
            &entity_id,
            &source_stage_id,
        ) {
            Ok(payload) => FileCopyResult {
                payload: Some(payload),
                errors: Vec::new(),
            },
            Err(message) => FileCopyResult {
                payload: None,
                errors: vec![command_error(
                    "create_next_stage_copy_failed",
                    message,
                    None,
                )],
            },
        },
        Err(error) => FileCopyResult {
            payload: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn run_due_tasks(path: String) -> RunDueTasksResult {
    match load_runtime_context(&path) {
        Ok(context) => match executor::run_due_tasks(
            &context.workdir_path,
            &context.database_path,
            context.config.runtime.max_parallel_tasks,
            context.config.runtime.request_timeout_sec,
            context.config.runtime.stuck_task_timeout_sec,
            context.config.runtime.file_stability_delay_ms,
        ) {
            Ok(summary) => RunDueTasksResult {
                summary: Some(summary),
                errors: Vec::new(),
            },
            Err(message) => RunDueTasksResult {
                summary: None,
                errors: vec![command_error("run_due_tasks_failed", message, None)],
            },
        },
        Err(error) => RunDueTasksResult {
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn run_due_tasks_limited(path: String, max_tasks: u64) -> RunDueTasksResult {
    let limited_max_tasks = max_tasks.clamp(1, 5);
    match load_runtime_context(&path) {
        Ok(context) => match executor::run_due_tasks(
            &context.workdir_path,
            &context.database_path,
            limited_max_tasks,
            context.config.runtime.request_timeout_sec,
            context.config.runtime.stuck_task_timeout_sec,
            context.config.runtime.file_stability_delay_ms,
        ) {
            Ok(summary) => RunDueTasksResult {
                summary: Some(summary),
                errors: Vec::new(),
            },
            Err(message) => RunDueTasksResult {
                summary: None,
                errors: vec![command_error("run_due_tasks_limited_failed", message, None)],
            },
        },
        Err(error) => RunDueTasksResult {
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn run_pipeline_waves(
    path: String,
    max_waves: u64,
    max_tasks_per_wave: u64,
    stop_on_first_failure: bool,
) -> RunPipelineWavesResult {
    match load_runtime_context(&path) {
        Ok(context) => match executor::run_pipeline_waves(
            &context.workdir_path,
            &context.database_path,
            max_waves,
            max_tasks_per_wave,
            stop_on_first_failure,
            context.config.runtime.request_timeout_sec,
            context.config.runtime.stuck_task_timeout_sec,
            context.config.runtime.file_stability_delay_ms,
        ) {
            Ok(summary) => RunPipelineWavesResult {
                summary: Some(summary),
                errors: Vec::new(),
            },
            Err(message) => RunPipelineWavesResult {
                summary: None,
                errors: vec![command_error("run_pipeline_waves_failed", message, None)],
            },
        },
        Err(error) => RunPipelineWavesResult {
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn run_entity_stage(path: String, entity_id: String, stage_id: String) -> RunEntityStageResult {
    match load_runtime_context(&path) {
        Ok(context) => match executor::run_entity_stage(
            &context.workdir_path,
            &context.database_path,
            &entity_id,
            &stage_id,
            context.config.runtime.request_timeout_sec,
            context.config.runtime.stuck_task_timeout_sec,
            context.config.runtime.file_stability_delay_ms,
        ) {
            Ok(summary) => RunEntityStageResult {
                summary: Some(summary),
                errors: Vec::new(),
            },
            Err(message) => RunEntityStageResult {
                summary: None,
                errors: vec![command_error("run_entity_stage_failed", message, None)],
            },
        },
        Err(error) => RunEntityStageResult {
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn retry_entity_stage_now(
    path: String,
    entity_id: String,
    stage_id: String,
    operator_comment: Option<String>,
) -> ManualEntityStageActionResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            let previous_status = match database::get_stage_state_status(
                &context.database_path,
                &entity_id,
                &stage_id,
            ) {
                Ok(Some(status)) => status,
                Ok(None) => {
                    return manual_action_error(
                        "stage_state_missing",
                        format!(
                            "No stage state exists for entity '{entity_id}' on stage '{stage_id}'."
                        ),
                    )
                }
                Err(message) => return manual_action_error("manual_retry_failed", message),
            };

            if !matches!(previous_status.as_str(), "pending" | "retry_wait") {
                return manual_action_error(
                    "manual_retry_not_allowed",
                    format!(
                        "Retry now is allowed only for pending or retry_wait states; current status is '{}'.",
                        previous_status
                    ),
                );
            }

            let summary = match executor::run_entity_stage(
                &context.workdir_path,
                &context.database_path,
                &entity_id,
                &stage_id,
                context.config.runtime.request_timeout_sec,
                context.config.runtime.stuck_task_timeout_sec,
                context.config.runtime.file_stability_delay_ms,
            ) {
                Ok(summary) => summary,
                Err(message) => return manual_action_error("manual_retry_failed", message),
            };
            let new_status =
                database::get_stage_state_status(&context.database_path, &entity_id, &stage_id)
                    .ok()
                    .flatten();
            if let Err(message) = database::record_manual_retry_event(
                &context.database_path,
                &entity_id,
                &stage_id,
                Some(&previous_status),
                new_status.as_deref(),
                operator_comment.as_deref(),
            ) {
                return manual_action_error("manual_retry_event_failed", message);
            }
            refreshed_manual_detail(&context.database_path, &entity_id, Some(summary))
        }
        Err(error) => ManualEntityStageActionResult {
            detail: None,
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn reset_entity_stage_to_pending(
    path: String,
    entity_id: String,
    stage_id: String,
    operator_comment: Option<String>,
) -> ManualEntityStageActionResult {
    match load_runtime_context(&path) {
        Ok(context) => match database::reset_entity_stage_to_pending(
            &context.database_path,
            &entity_id,
            &stage_id,
            operator_comment.as_deref(),
        ) {
            Ok(()) => refreshed_manual_detail(&context.database_path, &entity_id, None),
            Err(message) => manual_action_error("manual_reset_failed", message),
        },
        Err(error) => ManualEntityStageActionResult {
            detail: None,
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn skip_entity_stage(
    path: String,
    entity_id: String,
    stage_id: String,
    operator_comment: Option<String>,
) -> ManualEntityStageActionResult {
    match load_runtime_context(&path) {
        Ok(context) => match database::skip_entity_stage(
            &context.database_path,
            &entity_id,
            &stage_id,
            operator_comment.as_deref(),
        ) {
            Ok(()) => refreshed_manual_detail(&context.database_path, &entity_id, None),
            Err(message) => manual_action_error("manual_skip_failed", message),
        },
        Err(error) => ManualEntityStageActionResult {
            detail: None,
            summary: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn open_entity_file(path: String, entity_file_id: i64) -> OpenEntityPathResult {
    open_entity_path(path, entity_file_id, OpenEntityPathKind::File)
}

#[tauri::command]
pub fn open_entity_folder(path: String, entity_file_id: i64) -> OpenEntityPathResult {
    open_entity_path(path, entity_file_id, OpenEntityPathKind::Folder)
}

#[tauri::command]
pub fn save_entity_file_business_json(
    path: String,
    entity_file_id: i64,
    payload_json: String,
    meta_json: String,
    operator_comment: Option<String>,
) -> SaveEntityFileJsonResult {
    match load_runtime_context(&path) {
        Ok(context) => match file_ops::save_entity_file_business_json(
            &context.workdir_path,
            &context.database_path,
            entity_file_id,
            &payload_json,
            &meta_json,
            operator_comment.as_deref(),
            context.config.runtime.file_stability_delay_ms,
        ) {
            Ok(detail) => SaveEntityFileJsonResult {
                detail: Some(detail),
                errors: Vec::new(),
            },
            Err(message) => SaveEntityFileJsonResult {
                detail: None,
                errors: vec![command_error("save_entity_file_json_failed", message, None)],
            },
        },
        Err(error) => SaveEntityFileJsonResult {
            detail: None,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn list_stage_runs(path: String, entity_id: Option<String>) -> StageRunsResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            match database::list_stage_runs(&context.database_path, entity_id.as_deref()) {
                Ok(runs) => StageRunsResult {
                    runs,
                    errors: Vec::new(),
                },
                Err(message) => StageRunsResult {
                    runs: Vec::new(),
                    errors: vec![command_error("list_stage_runs_failed", message, None)],
                },
            }
        }
        Err(error) => StageRunsResult {
            runs: Vec::new(),
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn reconcile_stuck_tasks(path: String) -> ReconcileStuckTasksResult {
    match load_runtime_context(&path) {
        Ok(context) => match executor::reconcile_stuck_tasks(
            &context.database_path,
            context.config.runtime.stuck_task_timeout_sec,
        ) {
            Ok(reconciled) => ReconcileStuckTasksResult {
                reconciled,
                errors: Vec::new(),
            },
            Err(message) => ReconcileStuckTasksResult {
                reconciled: 0,
                errors: vec![command_error("reconcile_stuck_tasks_failed", message, None)],
            },
        },
        Err(error) => ReconcileStuckTasksResult {
            reconciled: 0,
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn list_app_events(path: String, limit: Option<u32>) -> AppEventsResult {
    match load_runtime_context(&path) {
        Ok(context) => match database::list_app_events(&context.database_path, limit.unwrap_or(50))
        {
            Ok(events) => AppEventsResult {
                events,
                errors: Vec::new(),
            },
            Err(message) => AppEventsResult {
                events: Vec::new(),
                errors: vec![command_error("list_app_events_failed", message, None)],
            },
        },
        Err(error) => AppEventsResult {
            events: Vec::new(),
            errors: vec![error],
        },
    }
}

#[tauri::command]
pub fn get_workspace_explorer(path: String) -> WorkspaceExplorerResult {
    match load_readonly_runtime_context(&path) {
        Ok(context) => {
            match database::get_workspace_explorer(&context.workdir_path, &context.database_path) {
                Ok(result) => result,
                Err(message) => empty_workspace_explorer_result(vec![command_error(
                    "workspace_explorer_failed",
                    message,
                    None,
                )]),
            }
        }
        Err(error) => empty_workspace_explorer_result(vec![error]),
    }
}

struct RuntimeContext {
    workdir_path: PathBuf,
    database_path: PathBuf,
    config: crate::domain::PipelineConfig,
}

struct ReadonlyRuntimeContext {
    workdir_path: PathBuf,
    database_path: PathBuf,
}

fn load_runtime_context(path: &str) -> Result<RuntimeContext, CommandErrorInfo> {
    let workdir_path = workdir::resolve_user_path(path).map_err(|message| {
        let code = if message.contains("outside the application directory") {
            "workdir_inside_application_directory"
        } else {
            "invalid_workdir_path"
        };
        command_error(code, message, None)
    })?;
    let workdir_state = workdir::inspect(&workdir_path, false);

    if !workdir_state.exists {
        return Err(command_error(
            "workdir_missing",
            "The selected workdir does not exist.",
            Some(workdir_state.workdir_path),
        ));
    }

    if !workdir_state.pipeline_config_exists {
        return Err(command_error(
            "pipeline_config_missing",
            "pipeline.yaml is required to open this workdir.",
            Some(workdir_state.pipeline_config_path),
        ));
    }

    let loaded_config =
        config::load_pipeline_config(Path::new(&workdir_state.pipeline_config_path));
    if !loaded_config.validation.is_valid {
        return Err(command_error(
            "config_invalid",
            "pipeline.yaml is invalid; fix validation errors before runtime operations.",
            Some(workdir_state.pipeline_config_path),
        ));
    }

    let Some(config) = loaded_config.config else {
        return Err(command_error(
            "config_unavailable",
            "pipeline.yaml could not be converted into a runtime configuration.",
            Some(workdir_state.pipeline_config_path),
        ));
    };

    let database_path = PathBuf::from(&workdir_state.database_path);
    database::bootstrap_database(&database_path, &config).map_err(|message| {
        command_error(
            "database_bootstrap_failed",
            message,
            Some(workdir_state.database_path),
        )
    })?;

    Ok(RuntimeContext {
        workdir_path,
        database_path,
        config,
    })
}

fn load_readonly_runtime_context(path: &str) -> Result<ReadonlyRuntimeContext, CommandErrorInfo> {
    let workdir_path = workdir::resolve_user_path(path).map_err(|message| {
        let code = if message.contains("outside the application directory") {
            "workdir_inside_application_directory"
        } else {
            "invalid_workdir_path"
        };
        command_error(code, message, None)
    })?;
    let workdir_state = workdir::inspect(&workdir_path, false);

    if !workdir_state.exists {
        return Err(command_error(
            "workdir_missing",
            "The selected workdir does not exist.",
            Some(workdir_state.workdir_path),
        ));
    }

    if !workdir_state.pipeline_config_exists {
        return Err(command_error(
            "pipeline_config_missing",
            "pipeline.yaml is required to open this workdir.",
            Some(workdir_state.pipeline_config_path),
        ));
    }

    let loaded_config =
        config::load_pipeline_config(Path::new(&workdir_state.pipeline_config_path));
    if !loaded_config.validation.is_valid {
        return Err(command_error(
            "config_invalid",
            "pipeline.yaml is invalid; fix validation errors before workspace explorer reads.",
            Some(workdir_state.pipeline_config_path),
        ));
    }

    Ok(ReadonlyRuntimeContext {
        workdir_path,
        database_path: PathBuf::from(&workdir_state.database_path),
    })
}

fn empty_workspace_explorer_result(errors: Vec<CommandErrorInfo>) -> WorkspaceExplorerResult {
    WorkspaceExplorerResult {
        generated_at: chrono::Utc::now().to_rfc3339(),
        workdir_path: String::new(),
        last_scan_at: None,
        stages: Vec::new(),
        entity_trails: Vec::new(),
        totals: WorkspaceExplorerTotals::default(),
        errors,
    }
}

fn command_error(code: &str, message: impl Into<String>, path: Option<String>) -> CommandErrorInfo {
    CommandErrorInfo {
        code: code.to_string(),
        message: message.into(),
        path,
    }
}

fn refreshed_manual_detail(
    database_path: &Path,
    entity_id: &str,
    summary: Option<crate::domain::RunDueTasksSummary>,
) -> ManualEntityStageActionResult {
    match database::get_entity_detail_with_selection(database_path, entity_id, None) {
        Ok(detail) => ManualEntityStageActionResult {
            detail,
            summary,
            errors: Vec::new(),
        },
        Err(message) => manual_action_error("manual_action_refresh_failed", message),
    }
}

fn manual_action_error(code: &str, message: impl Into<String>) -> ManualEntityStageActionResult {
    ManualEntityStageActionResult {
        detail: None,
        summary: None,
        errors: vec![command_error(code, message, None)],
    }
}

fn open_entity_path(
    path: String,
    entity_file_id: i64,
    kind: OpenEntityPathKind,
) -> OpenEntityPathResult {
    match load_runtime_context(&path) {
        Ok(context) => match file_open::open_entity_path(
            &context.workdir_path,
            &context.database_path,
            entity_file_id,
            kind,
        ) {
            Ok(opened_path) => OpenEntityPathResult {
                payload: Some(OpenEntityPathPayload { opened_path }),
                errors: Vec::new(),
            },
            Err(message) => OpenEntityPathResult {
                payload: None,
                errors: vec![command_error("open_entity_path_failed", message, None)],
            },
        },
        Err(error) => OpenEntityPathResult {
            payload: None,
            errors: vec![error],
        },
    }
}

fn scan_error(code: &str, message: impl Into<String>) -> ScanWorkspaceResult {
    ScanWorkspaceResult {
        summary: None,
        errors: vec![command_error(code, message, None)],
    }
}

use std::path::{Path, PathBuf};

use crate::bootstrap;
use crate::config;
use crate::dashboard;
use crate::database;
use crate::discovery;
use crate::domain::{
    AppEventsResult, BootstrapResult, CommandErrorInfo, DashboardOverviewResult,
    EntityDetailResult, EntityFilesResult, EntityFilters, EntityListResult, FileCopyResult,
    ReconcileStuckTasksResult, RunDueTasksResult, RunEntityStageResult, RuntimeSummaryResult,
    ScanWorkspaceResult, StageDirectoryProvisionResult, StageListResult, StageRunsResult,
    WorkspaceExplorerResult,
};
use crate::executor;
use crate::file_ops;
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
            match discovery::scan_workspace(&context.workdir_path, &context.database_path) {
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
pub fn list_entities(path: String, filters: Option<EntityFilters>) -> EntityListResult {
    match load_runtime_context(&path) {
        Ok(context) => {
            let filters = filters.unwrap_or_default();
            match (
                database::list_entities(&context.database_path, &filters),
                database::list_stages(&context.database_path),
            ) {
                (Ok(entities), Ok(stages)) => EntityListResult {
                    total: entities.len() as u64,
                    available_stages: stages.into_iter().map(|stage| stage.id).collect(),
                    entities,
                    errors: Vec::new(),
                },
                (Err(message), _) | (_, Err(message)) => EntityListResult {
                    entities: Vec::new(),
                    total: 0,
                    available_stages: Vec::new(),
                    errors: vec![command_error("list_entities_failed", message, None)],
                },
            }
        }
        Err(error) => EntityListResult {
            entities: Vec::new(),
            total: 0,
            available_stages: Vec::new(),
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
pub fn get_entity(path: String, entity_id: String) -> EntityDetailResult {
    match load_runtime_context(&path) {
        Ok(context) => match database::get_entity_detail(&context.database_path, &entity_id) {
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
pub fn run_entity_stage(path: String, entity_id: String, stage_id: String) -> RunEntityStageResult {
    match load_runtime_context(&path) {
        Ok(context) => match executor::run_entity_stage(
            &context.workdir_path,
            &context.database_path,
            &entity_id,
            &stage_id,
            context.config.runtime.request_timeout_sec,
            context.config.runtime.stuck_task_timeout_sec,
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
    match load_runtime_context(&path) {
        Ok(context) => match database::get_workspace_explorer(&context.database_path) {
            Ok(result) => result,
            Err(message) => WorkspaceExplorerResult {
                groups: Vec::new(),
                errors: vec![command_error("workspace_explorer_failed", message, None)],
            },
        },
        Err(error) => WorkspaceExplorerResult {
            groups: Vec::new(),
            errors: vec![error],
        },
    }
}

struct RuntimeContext {
    workdir_path: PathBuf,
    database_path: PathBuf,
    config: crate::domain::PipelineConfig,
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

fn command_error(code: &str, message: impl Into<String>, path: Option<String>) -> CommandErrorInfo {
    CommandErrorInfo {
        code: code.to_string(),
        message: message.into(),
        path,
    }
}

fn scan_error(code: &str, message: impl Into<String>) -> ScanWorkspaceResult {
    ScanWorkspaceResult {
        summary: None,
        errors: vec![command_error(code, message, None)],
    }
}

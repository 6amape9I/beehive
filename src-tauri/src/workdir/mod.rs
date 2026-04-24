use std::fs;
use std::path::{Path, PathBuf};

use crate::config::default_pipeline_yaml;
use crate::domain::{WorkdirHealthIssue, WorkdirHealthSeverity, WorkdirState};

pub fn initialize(path: &Path) -> Result<WorkdirState, String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("Failed to create workdir '{}': {error}", path.display()))?;

    let stages_dir = path.join("stages");
    let logs_dir = path.join("logs");
    fs::create_dir_all(&stages_dir).map_err(|error| {
        format!(
            "Failed to create stages directory '{}': {error}",
            stages_dir.display()
        )
    })?;
    fs::create_dir_all(&logs_dir).map_err(|error| {
        format!(
            "Failed to create logs directory '{}': {error}",
            logs_dir.display()
        )
    })?;

    let pipeline_path = path.join("pipeline.yaml");
    if !pipeline_path.exists() {
        fs::write(&pipeline_path, default_pipeline_yaml()).map_err(|error| {
            format!(
                "Failed to write default pipeline config '{}': {error}",
                pipeline_path.display()
            )
        })?;
    }

    Ok(inspect(path, true))
}

pub fn inspect(path: &Path, initialization_flow: bool) -> WorkdirState {
    let pipeline_path = path.join("pipeline.yaml");
    let database_path = path.join("app.db");
    let stages_dir = path.join("stages");
    let logs_dir = path.join("logs");

    let exists = path.exists() && path.is_dir();
    let pipeline_config_exists = pipeline_path.exists() && pipeline_path.is_file();
    let database_exists = database_path.exists() && database_path.is_file();
    let stages_dir_exists = stages_dir.exists() && stages_dir.is_dir();
    let logs_dir_exists = logs_dir.exists() && logs_dir.is_dir();

    let mut health_issues = Vec::new();
    if !exists {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Error,
            "workdir_missing",
            path,
            "The selected workdir does not exist.",
        ));
    }
    if exists && !pipeline_config_exists {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Error,
            "pipeline_config_missing",
            &pipeline_path,
            "pipeline.yaml is required to open an existing workdir.",
        ));
    }
    if exists && !database_exists {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Info,
            "database_missing",
            &database_path,
            "app.db is missing and will be created during bootstrap.",
        ));
    }
    if exists && !stages_dir_exists && !initialization_flow {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Warning,
            "stages_dir_missing",
            &stages_dir,
            "stages/ is missing.",
        ));
    }
    if exists && !logs_dir_exists && !initialization_flow {
        health_issues.push(health_issue(
            WorkdirHealthSeverity::Warning,
            "logs_dir_missing",
            &logs_dir,
            "logs/ is missing.",
        ));
    }

    WorkdirState {
        workdir_path: path_string(path),
        pipeline_config_path: path_string(&pipeline_path),
        database_path: path_string(&database_path),
        stages_dir_path: path_string(&stages_dir),
        logs_dir_path: path_string(&logs_dir),
        exists,
        pipeline_config_exists,
        database_exists,
        stages_dir_exists,
        logs_dir_exists,
        health_issues,
    }
}

pub fn normalize_path(path: &str) -> PathBuf {
    PathBuf::from(path.trim())
}

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn health_issue(
    severity: WorkdirHealthSeverity,
    code: impl Into<String>,
    path: &Path,
    message: impl Into<String>,
) -> WorkdirHealthIssue {
    WorkdirHealthIssue {
        severity,
        code: code.into(),
        path: path_string(path),
        message: message.into(),
    }
}

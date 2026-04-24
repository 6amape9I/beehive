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

pub fn parse_user_path(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Workdir path cannot be empty.".to_string());
    }

    let candidate = PathBuf::from(trimmed);
    if !candidate.is_absolute() {
        return Err(
            "Workdir path must be absolute. Use Browse or enter a full path outside the application directory."
                .to_string(),
        );
    }

    Ok(candidate)
}

pub fn validate_runtime_location(path: &Path) -> Result<(), String> {
    let runtime_dir = std::env::current_dir()
        .map_err(|error| format!("Failed to determine the application directory: {error}"))?;

    validate_runtime_location_with_base(path, &runtime_dir)
}

fn validate_runtime_location_with_base(path: &Path, runtime_dir: &Path) -> Result<(), String> {
    if path.starts_with(runtime_dir) {
        return Err(format!(
            "Workdir must be outside the application directory '{}'. Choose a separate folder to avoid dev-mode restarts and mutable runtime files inside the app tree.",
            runtime_dir.display()
        ));
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn initialize_creates_required_stage_one_files_and_directories() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("new-workdir");

        let state = initialize(&workdir).expect("initialize workdir");

        assert_eq!(state.workdir_path, path_string(&workdir));
        assert!(workdir.join("pipeline.yaml").exists());
        assert!(workdir.join("stages").is_dir());
        assert!(workdir.join("logs").is_dir());
        assert!(!workdir.join("app.db").exists());
        assert!(state.pipeline_config_exists);
        assert!(!state.database_exists);
        assert!(state.stages_dir_exists);
        assert!(state.logs_dir_exists);
    }

    #[test]
    fn inspect_existing_workdir_reports_healthy_state() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("existing-workdir");
        fs::create_dir_all(workdir.join("stages")).expect("create stages");
        fs::create_dir_all(workdir.join("logs")).expect("create logs");
        fs::write(workdir.join("pipeline.yaml"), default_pipeline_yaml()).expect("write pipeline");
        fs::write(workdir.join("app.db"), []).expect("write db placeholder");

        let state = inspect(&workdir, false);

        assert!(state.exists);
        assert!(state.pipeline_config_exists);
        assert!(state.database_exists);
        assert!(state.stages_dir_exists);
        assert!(state.logs_dir_exists);
        assert!(state.health_issues.is_empty());
    }

    #[test]
    fn parse_user_path_rejects_relative_paths() {
        let error = parse_user_path("relative-workdir").expect_err("relative path should fail");

        assert!(error.contains("must be absolute"));
    }

    #[test]
    fn validate_runtime_location_rejects_paths_inside_application_directory() {
        let runtime_dir = env::temp_dir().join("beehive-runtime-dir");
        let workdir = runtime_dir.join("nested-workdir");

        let error = validate_runtime_location_with_base(&workdir, &runtime_dir)
            .expect_err("nested workdir should fail");

        assert!(error.contains("outside the application directory"));
    }
}

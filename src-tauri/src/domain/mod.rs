use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Queued,
    InProgress,
    RetryWait,
    Done,
    Failed,
    Blocked,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigValidationIssue {
    pub severity: ValidationSeverity,
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigValidationResult {
    pub is_valid: bool,
    pub issues: Vec<ConfigValidationIssue>,
}

impl ConfigValidationResult {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            issues: Vec::new(),
        }
    }

    pub fn from_issues(issues: Vec<ConfigValidationIssue>) -> Self {
        let is_valid = !issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Error);
        Self { is_valid, issues }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfig {
    pub name: String,
    pub workdir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub scan_interval_sec: u64,
    pub max_parallel_tasks: u64,
    pub stuck_task_timeout_sec: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            scan_interval_sec: 5,
            max_parallel_tasks: 3,
            stuck_task_timeout_sec: 900,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageDefinition {
    pub id: String,
    pub input_folder: String,
    pub output_folder: String,
    pub workflow_url: String,
    pub max_attempts: u64,
    pub retry_delay_sec: u64,
    pub next_stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineConfig {
    pub project: ProjectConfig,
    pub runtime: RuntimeConfig,
    pub stages: Vec<StageDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkdirHealthSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkdirHealthIssue {
    pub severity: WorkdirHealthSeverity,
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkdirState {
    pub workdir_path: String,
    pub pipeline_config_path: String,
    pub database_path: String,
    pub stages_dir_path: String,
    pub logs_dir_path: String,
    pub exists: bool,
    pub pipeline_config_exists: bool,
    pub database_exists: bool,
    pub stages_dir_exists: bool,
    pub logs_dir_exists: bool,
    pub health_issues: Vec<WorkdirHealthIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseState {
    pub database_path: String,
    pub is_ready: bool,
    pub schema_version: u32,
    pub stage_count: u64,
    pub synced_stage_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppInitializationPhase {
    AppNotConfigured,
    WorkdirSelected,
    ConfigLoaded,
    ConfigInvalid,
    DatabaseReady,
    BootstrapFailed,
    FullyInitialized,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapErrorInfo {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppInitializationState {
    pub phase: AppInitializationPhase,
    pub message: String,
    pub selected_workdir_path: Option<String>,
    pub project_name: Option<String>,
    pub config_path: Option<String>,
    pub database_path: Option<String>,
    pub config_loaded: bool,
    pub config_status: String,
    pub database_status: String,
    pub stage_count: u64,
    pub stage_ids: Vec<String>,
    pub last_config_load_at: Option<String>,
    pub validation: ConfigValidationResult,
    pub workdir_state: Option<WorkdirState>,
    pub database_state: Option<DatabaseState>,
    pub config: Option<PipelineConfig>,
    pub errors: Vec<BootstrapErrorInfo>,
}

impl AppInitializationState {
    pub fn not_configured() -> Self {
        Self {
            phase: AppInitializationPhase::AppNotConfigured,
            message: "No workdir is selected.".to_string(),
            selected_workdir_path: None,
            project_name: None,
            config_path: None,
            database_path: None,
            config_loaded: false,
            config_status: "not_loaded".to_string(),
            database_status: "not_ready".to_string(),
            stage_count: 0,
            stage_ids: Vec::new(),
            last_config_load_at: None,
            validation: ConfigValidationResult::valid(),
            workdir_state: None,
            database_state: None,
            config: None,
            errors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapResult {
    pub state: AppInitializationState,
}

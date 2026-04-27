use serde::{Deserialize, Serialize};
use serde_json::Value;

#[allow(dead_code)]
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
    pub request_timeout_sec: u64,
    pub file_stability_delay_ms: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            scan_interval_sec: 5,
            max_parallel_tasks: 3,
            stuck_task_timeout_sec: 900,
            request_timeout_sec: 30,
            file_stability_delay_ms: 1000,
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
    pub active_stage_count: u64,
    pub inactive_stage_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppInitializationPhase {
    AppNotConfigured,
    ConfigInvalid,
    BootstrapFailed,
    FullyInitialized,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandErrorInfo {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

pub type BootstrapErrorInfo = CommandErrorInfo;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppInitializationState {
    pub phase: AppInitializationPhase,
    pub message: String,
    pub selected_workdir_path: Option<String>,
    pub project_name: Option<String>,
    pub config_path: Option<String>,
    pub database_path: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapResult {
    pub state: AppInitializationState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageRecord {
    pub id: String,
    pub input_folder: String,
    pub output_folder: String,
    pub workflow_url: String,
    pub max_attempts: u64,
    pub retry_delay_sec: u64,
    pub next_stage: Option<String>,
    pub is_active: bool,
    pub archived_at: Option<String>,
    pub last_seen_in_config_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub entity_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntityValidationStatus {
    Valid,
    Warning,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityRecord {
    pub entity_id: String,
    pub current_stage_id: Option<String>,
    pub current_status: String,
    pub latest_file_path: Option<String>,
    pub latest_file_id: Option<i64>,
    pub file_count: u64,
    pub validation_status: EntityValidationStatus,
    pub validation_errors: Vec<ConfigValidationIssue>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityFileRecord {
    pub id: i64,
    pub entity_id: String,
    pub stage_id: String,
    pub file_path: String,
    pub file_name: String,
    pub checksum: String,
    pub file_mtime: String,
    pub file_size: u64,
    pub payload_json: String,
    pub meta_json: String,
    pub current_stage: Option<String>,
    pub next_stage: Option<String>,
    pub status: String,
    pub validation_status: EntityValidationStatus,
    pub validation_errors: Vec<ConfigValidationIssue>,
    pub is_managed_copy: bool,
    pub copy_source_file_id: Option<i64>,
    pub file_exists: bool,
    pub missing_since: Option<String>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityStageStateRecord {
    pub id: i64,
    pub entity_id: String,
    pub stage_id: String,
    pub file_path: String,
    pub file_instance_id: Option<i64>,
    pub file_exists: bool,
    pub status: String,
    pub attempts: u64,
    pub max_attempts: u64,
    pub last_error: Option<String>,
    pub last_http_status: Option<i64>,
    pub next_retry_at: Option<String>,
    pub last_started_at: Option<String>,
    pub last_finished_at: Option<String>,
    pub created_child_path: Option<String>,
    pub discovered_at: String,
    pub last_seen_at: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppEventLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppEventRecord {
    pub id: i64,
    pub level: AppEventLevel,
    pub code: String,
    pub message: String,
    pub context: Option<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusCount {
    pub status: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeSummary {
    pub schema_version: u32,
    pub active_stage_count: u64,
    pub inactive_stage_count: u64,
    pub total_entities: u64,
    pub present_file_count: u64,
    pub missing_file_count: u64,
    pub managed_copy_count: u64,
    pub invalid_file_count: u64,
    pub entities_by_status: Vec<StatusCount>,
    pub execution_status_counts: Vec<StatusCount>,
    pub last_reconciliation_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DashboardStageHealth {
    Ok,
    Warning,
    Error,
    Inactive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardProjectContext {
    pub name: String,
    pub workdir_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardTotals {
    pub entities_total: u64,
    pub entity_files_total: u64,
    pub stages_total: u64,
    pub active_stages_total: u64,
    pub inactive_stages_total: u64,
    pub active_tasks_total: u64,
    pub errors_total: u64,
    pub warnings_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardRuntimeOverview {
    pub last_scan_at: Option<String>,
    pub last_run_at: Option<String>,
    pub last_successful_run_at: Option<String>,
    pub last_error_at: Option<String>,
    pub due_tasks_count: u64,
    pub in_progress_count: u64,
    pub retry_wait_count: u64,
    pub failed_count: u64,
    pub blocked_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardStageNode {
    pub id: String,
    pub label: String,
    pub input_folder: String,
    pub output_folder: Option<String>,
    pub workflow_url: Option<String>,
    pub is_active: bool,
    pub archived_at: Option<String>,
    pub next_stage: Option<String>,
    pub position_index: u64,
    pub health: DashboardStageHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardStageEdge {
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub is_valid: bool,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardStageGraph {
    pub nodes: Vec<DashboardStageNode>,
    pub edges: Vec<DashboardStageEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardStageCounters {
    pub stage_id: String,
    pub stage_label: String,
    pub is_active: bool,
    pub total: u64,
    pub pending: u64,
    pub queued: u64,
    pub in_progress: u64,
    pub retry_wait: u64,
    pub done: u64,
    pub failed: u64,
    pub blocked: u64,
    pub skipped: u64,
    pub unknown: u64,
    pub missing_files: u64,
    pub existing_files: u64,
    pub last_started_at: Option<String>,
    pub last_finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardActiveTask {
    pub entity_id: String,
    pub stage_id: String,
    pub status: String,
    pub attempts: u64,
    pub max_attempts: u64,
    pub next_retry_at: Option<String>,
    pub last_started_at: Option<String>,
    pub updated_at: Option<String>,
    pub file_path: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardErrorItem {
    pub id: i64,
    pub level: String,
    pub event_type: String,
    pub message: String,
    pub entity_id: Option<String>,
    pub stage_id: Option<String>,
    pub run_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardRunItem {
    pub run_id: String,
    pub entity_id: String,
    pub stage_id: String,
    pub success: bool,
    pub http_status: Option<i64>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardOverview {
    pub generated_at: String,
    pub project: DashboardProjectContext,
    pub totals: DashboardTotals,
    pub runtime: DashboardRuntimeOverview,
    pub stage_graph: DashboardStageGraph,
    pub stage_counters: Vec<DashboardStageCounters>,
    pub active_tasks: Vec<DashboardActiveTask>,
    pub last_errors: Vec<DashboardErrorItem>,
    pub recent_runs: Vec<DashboardRunItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EntityFilters {
    pub stage_id: Option<String>,
    pub status: Option<String>,
    pub validation_status: Option<EntityValidationStatus>,
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanSummary {
    pub scan_id: String,
    pub scanned_file_count: u64,
    pub registered_file_count: u64,
    pub registered_entity_count: u64,
    pub updated_file_count: u64,
    pub unchanged_file_count: u64,
    pub missing_file_count: u64,
    pub restored_file_count: u64,
    pub invalid_count: u64,
    pub duplicate_count: u64,
    pub created_directory_count: u64,
    pub managed_copy_count: u64,
    pub elapsed_ms: u128,
    pub latest_discovery_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageDirectoryProvisionSummary {
    pub created_paths: Vec<String>,
    pub created_directory_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanWorkspaceResult {
    pub summary: Option<ScanSummary>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeSummaryResult {
    pub summary: Option<RuntimeSummary>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardOverviewResult {
    pub overview: Option<DashboardOverview>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageListResult {
    pub stages: Vec<StageRecord>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityListResult {
    pub entities: Vec<EntityRecord>,
    pub total: u64,
    pub available_stages: Vec<String>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityFilesResult {
    pub files: Vec<EntityFileRecord>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityDetailPayload {
    pub entity: EntityRecord,
    pub files: Vec<EntityFileRecord>,
    pub stage_states: Vec<EntityStageStateRecord>,
    pub latest_json_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityDetailResult {
    pub detail: Option<EntityDetailPayload>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppEventsResult {
    pub events: Vec<AppEventRecord>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceFileRecord {
    pub file_id: i64,
    pub entity_id: String,
    pub file_name: String,
    pub file_path: String,
    pub status: String,
    pub validation_status: EntityValidationStatus,
    pub updated_at: String,
    pub file_exists: bool,
    pub missing_since: Option<String>,
    pub is_managed_copy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidDiscoveryRecord {
    pub stage_id: Option<String>,
    pub file_name: String,
    pub file_path: String,
    pub code: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceStageGroup {
    pub stage: StageRecord,
    pub files: Vec<WorkspaceFileRecord>,
    pub invalid_files: Vec<InvalidDiscoveryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceExplorerResult {
    pub groups: Vec<WorkspaceStageGroup>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileCopyStatus {
    Created,
    AlreadyExists,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileCopyPayload {
    pub status: FileCopyStatus,
    pub entity_id: String,
    pub source_stage_id: String,
    pub target_stage_id: Option<String>,
    pub source_file_path: Option<String>,
    pub target_file_path: Option<String>,
    pub target_file: Option<EntityFileRecord>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileCopyResult {
    pub payload: Option<FileCopyPayload>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageDirectoryProvisionResult {
    pub summary: Option<StageDirectoryProvisionSummary>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageRunRecord {
    pub id: i64,
    pub run_id: String,
    pub entity_id: String,
    pub entity_file_id: Option<i64>,
    pub stage_id: String,
    pub attempt_no: u64,
    pub workflow_url: String,
    pub request_json: String,
    pub response_json: Option<String>,
    pub http_status: Option<i64>,
    pub success: bool,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RunDueTasksSummary {
    pub claimed: u64,
    pub succeeded: u64,
    pub retry_scheduled: u64,
    pub failed: u64,
    pub blocked: u64,
    pub skipped: u64,
    pub stuck_reconciled: u64,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunDueTasksResult {
    pub summary: Option<RunDueTasksSummary>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunEntityStageResult {
    pub summary: Option<RunDueTasksSummary>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageRunsResult {
    pub runs: Vec<StageRunRecord>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconcileStuckTasksResult {
    pub reconciled: u64,
    pub errors: Vec<CommandErrorInfo>,
}

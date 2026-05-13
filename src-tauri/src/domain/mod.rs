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
#[serde(rename_all = "snake_case")]
pub enum StorageProvider {
    Local,
    S3,
}

impl StorageProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::S3 => "s3",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactLocation {
    pub provider: StorageProvider,
    pub local_path: Option<String>,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub version_id: Option<String>,
    pub etag: Option<String>,
    pub checksum_sha256: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct S3StorageConfig {
    pub bucket: String,
    pub workspace_prefix: String,
    pub region: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageConfig {
    pub provider: StorageProvider,
    pub bucket: Option<String>,
    pub workspace_prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
}

impl StorageConfig {
    pub fn s3_config(&self) -> Option<S3StorageConfig> {
        if self.provider != StorageProvider::S3 {
            return None;
        }
        Some(S3StorageConfig {
            bucket: self.bucket.clone()?,
            workspace_prefix: self.workspace_prefix.clone()?,
            region: self.region.clone(),
            endpoint: self.endpoint.clone(),
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageStorageConfig {
    pub stage_id: String,
    pub input_uri: Option<String>,
    pub input_folder: Option<String>,
    pub save_path_aliases: Vec<String>,
    pub allow_empty_outputs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageDefinition {
    pub id: String,
    pub input_folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_uri: Option<String>,
    pub output_folder: String,
    pub workflow_url: String,
    pub max_attempts: u64,
    pub retry_delay_sec: u64,
    pub next_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save_path_aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_empty_outputs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineConfig {
    pub project: ProjectConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageConfig>,
    pub runtime: RuntimeConfig,
    pub stages: Vec<StageDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfigDraft {
    pub name: String,
    pub workdir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfigDraft {
    pub scan_interval_sec: i64,
    pub max_parallel_tasks: i64,
    pub stuck_task_timeout_sec: i64,
    pub request_timeout_sec: i64,
    pub file_stability_delay_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageDefinitionDraft {
    pub id: String,
    pub input_folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_uri: Option<String>,
    pub output_folder: String,
    pub workflow_url: String,
    pub max_attempts: i64,
    pub retry_delay_sec: i64,
    pub next_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save_path_aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_empty_outputs: bool,
    pub original_stage_id: Option<String>,
    pub is_new: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineConfigDraft {
    pub project: ProjectConfigDraft,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageConfig>,
    pub runtime: RuntimeConfigDraft,
    pub stages: Vec<StageDefinitionDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageUsageSummary {
    pub stage_id: String,
    pub is_active: bool,
    pub entity_count: u64,
    pub entity_file_count: u64,
    pub stage_state_count: u64,
    pub run_count: u64,
    pub last_seen_in_config_at: Option<String>,
    pub archived_at: Option<String>,
    pub can_remove_from_config: bool,
    pub can_rename: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineEditorState {
    pub config: Option<PipelineConfig>,
    pub draft: Option<PipelineConfigDraft>,
    pub yaml_text: String,
    pub yaml_preview: String,
    pub validation: ConfigValidationResult,
    pub stage_usages: Vec<StageUsageSummary>,
    pub loaded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineEditorStateResult {
    pub state: Option<PipelineEditorState>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatePipelineConfigDraftResult {
    pub validation: ConfigValidationResult,
    pub normalized_config: Option<PipelineConfig>,
    pub yaml_preview: Option<String>,
    pub stage_usages: Vec<StageUsageSummary>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavePipelineConfigResult {
    pub state: Option<PipelineEditorState>,
    pub backup_path: Option<String>,
    pub errors: Vec<CommandErrorInfo>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_workspace_id: Option<String>,
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
pub struct WorkspaceDescriptor {
    pub id: String,
    pub name: String,
    pub provider: StorageProvider,
    pub bucket: Option<String>,
    pub workspace_prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceRegistryListResult {
    pub workspaces: Vec<WorkspaceDescriptor>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceRegistryEntryResult {
    pub workspace: Option<WorkspaceDescriptor>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageRecord {
    pub id: String,
    pub input_folder: String,
    pub input_uri: Option<String>,
    pub output_folder: String,
    pub workflow_url: String,
    pub max_attempts: u64,
    pub retry_delay_sec: u64,
    pub next_stage: Option<String>,
    pub save_path_aliases: Vec<String>,
    pub allow_empty_outputs: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EntityListQuery {
    pub search: Option<String>,
    pub stage_id: Option<String>,
    pub status: Option<String>,
    pub validation_status: Option<EntityValidationStatus>,
    pub sort_by: Option<String>,
    pub sort_direction: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityTableRow {
    pub entity_id: String,
    pub display_name: Option<String>,
    pub current_stage_id: Option<String>,
    pub current_status: String,
    pub latest_file_path: Option<String>,
    pub latest_file_id: Option<i64>,
    pub file_count: u64,
    pub attempts: Option<u64>,
    pub max_attempts: Option<u64>,
    pub last_error: Option<String>,
    pub last_http_status: Option<i64>,
    pub next_retry_at: Option<String>,
    pub last_started_at: Option<String>,
    pub last_finished_at: Option<String>,
    pub validation_status: EntityValidationStatus,
    pub updated_at: String,
    pub last_seen_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityFileRecord {
    pub id: i64,
    pub entity_id: String,
    pub stage_id: String,
    pub file_path: String,
    pub file_name: String,
    pub artifact_id: Option<String>,
    pub relation_to_source: Option<String>,
    pub storage_provider: StorageProvider,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub version_id: Option<String>,
    pub etag: Option<String>,
    pub checksum_sha256: Option<String>,
    pub checksum: String,
    pub file_mtime: String,
    pub file_size: u64,
    pub artifact_size: Option<u64>,
    pub payload_json: String,
    pub meta_json: String,
    pub current_stage: Option<String>,
    pub next_stage: Option<String>,
    pub status: String,
    pub validation_status: EntityValidationStatus,
    pub validation_errors: Vec<ConfigValidationIssue>,
    pub is_managed_copy: bool,
    pub copy_source_file_id: Option<i64>,
    pub producer_run_id: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityTimelineItem {
    pub stage_id: String,
    pub status: String,
    pub attempts: u64,
    pub max_attempts: u64,
    pub file_path: Option<String>,
    pub file_exists: bool,
    pub last_error: Option<String>,
    pub last_http_status: Option<i64>,
    pub next_retry_at: Option<String>,
    pub last_started_at: Option<String>,
    pub last_finished_at: Option<String>,
    pub created_child_path: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityStageAllowedActions {
    pub stage_id: String,
    pub can_retry_now: bool,
    pub can_reset_to_pending: bool,
    pub can_skip: bool,
    pub can_run_this_stage: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityFileAllowedActions {
    pub entity_file_id: i64,
    pub can_edit_business_json: bool,
    pub can_open_file: bool,
    pub can_open_folder: bool,
    pub reasons: Vec<String>,
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

#[allow(dead_code)]
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
pub struct S3ReconciliationSummary {
    pub scan_id: String,
    pub stage_count: u64,
    pub listed_object_count: u64,
    pub metadata_tagged_count: u64,
    pub registered_file_count: u64,
    pub updated_file_count: u64,
    pub unchanged_file_count: u64,
    pub missing_file_count: u64,
    pub restored_file_count: u64,
    pub unmapped_object_count: u64,
    pub elapsed_ms: u128,
    pub latest_reconciliation_at: String,
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
pub struct S3ReconciliationResult {
    pub summary: Option<S3ReconciliationSummary>,
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
    pub entities: Vec<EntityTableRow>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
    pub available_stages: Vec<String>,
    pub available_statuses: Vec<String>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityFilesResult {
    pub files: Vec<EntityFileRecord>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterS3SourceArtifactRequest {
    pub stage_id: String,
    pub entity_id: String,
    pub artifact_id: String,
    pub bucket: String,
    pub key: String,
    pub version_id: Option<String>,
    pub etag: Option<String>,
    pub checksum_sha256: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterS3SourceArtifactPayload {
    pub file: EntityFileRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterS3SourceArtifactResult {
    pub payload: Option<RegisterS3SourceArtifactPayload>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateS3StageRequest {
    pub stage_id: String,
    pub workflow_url: String,
    pub next_stage: Option<String>,
    pub max_attempts: Option<u64>,
    pub retry_delay_sec: Option<u64>,
    pub allow_empty_outputs: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct S3StageRouteHints {
    pub input_uri: String,
    pub save_path_aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateS3StagePayload {
    pub stage: StageDefinition,
    pub route_hints: S3StageRouteHints,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateS3StageResult {
    pub payload: Option<CreateS3StagePayload>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityDetailPayload {
    pub entity: EntityRecord,
    pub files: Vec<EntityFileRecord>,
    pub stage_states: Vec<EntityStageStateRecord>,
    pub stage_runs: Vec<StageRunRecord>,
    pub timeline: Vec<EntityTimelineItem>,
    pub latest_json_preview: String,
    pub selected_file_json: Option<String>,
    pub allowed_actions: Vec<EntityStageAllowedActions>,
    pub file_allowed_actions: Vec<EntityFileAllowedActions>,
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
pub struct InvalidDiscoveryRecord {
    pub stage_id: Option<String>,
    pub file_name: String,
    pub file_path: String,
    pub code: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceFileNode {
    pub entity_file_id: i64,
    pub entity_id: String,
    pub stage_id: String,
    pub file_name: String,
    pub file_path: String,
    pub storage_provider: StorageProvider,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub artifact_id: Option<String>,
    pub relation_to_source: Option<String>,
    pub producer_run_id: Option<String>,
    pub file_exists: bool,
    pub missing_since: Option<String>,
    pub is_managed_copy: bool,
    pub copy_source_file_id: Option<i64>,
    pub copy_source_entity_id: Option<String>,
    pub copy_source_stage_id: Option<String>,
    pub runtime_status: Option<String>,
    pub file_status: String,
    pub validation_status: EntityValidationStatus,
    pub validation_errors: Vec<ConfigValidationIssue>,
    pub current_stage: Option<String>,
    pub next_stage: Option<String>,
    pub checksum: String,
    pub file_size: u64,
    pub file_mtime: String,
    pub updated_at: String,
    pub can_open_file: bool,
    pub can_open_folder: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorkspaceStageTreeCounters {
    pub registered_files: u64,
    pub present_files: u64,
    pub missing_files: u64,
    pub invalid_files: u64,
    pub managed_copies: u64,
    pub pending: u64,
    pub queued: u64,
    pub in_progress: u64,
    pub retry_wait: u64,
    pub done: u64,
    pub failed: u64,
    pub blocked: u64,
    pub skipped: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceStageTree {
    pub stage_id: String,
    pub input_folder: String,
    pub input_uri: Option<String>,
    pub storage_provider: StorageProvider,
    pub output_folder: Option<String>,
    pub workflow_url: Option<String>,
    pub next_stage: Option<String>,
    pub is_active: bool,
    pub archived_at: Option<String>,
    pub folder_path: String,
    pub folder_exists: bool,
    pub files: Vec<WorkspaceFileNode>,
    pub invalid_files: Vec<InvalidDiscoveryRecord>,
    pub counters: WorkspaceStageTreeCounters,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceEntityTrailNode {
    pub entity_file_id: i64,
    pub stage_id: String,
    pub file_name: String,
    pub file_path: String,
    pub file_exists: bool,
    pub runtime_status: Option<String>,
    pub is_managed_copy: bool,
    pub can_open_file: bool,
    pub can_open_folder: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceEntityTrailEdge {
    pub from_entity_file_id: i64,
    pub to_entity_file_id: i64,
    pub relation: String,
    pub created_child_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceEntityTrail {
    pub entity_id: String,
    pub file_count: u64,
    pub stages: Vec<WorkspaceEntityTrailNode>,
    pub edges: Vec<WorkspaceEntityTrailEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WorkspaceExplorerTotals {
    pub stages_total: u64,
    pub active_stages_total: u64,
    pub inactive_stages_total: u64,
    pub entities_total: u64,
    pub registered_files_total: u64,
    pub present_files_total: u64,
    pub missing_files_total: u64,
    pub invalid_files_total: u64,
    pub managed_copies_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceExplorerResult {
    pub generated_at: String,
    pub workdir_path: String,
    pub last_scan_at: Option<String>,
    pub stages: Vec<WorkspaceStageTree>,
    pub entity_trails: Vec<WorkspaceEntityTrail>,
    pub totals: WorkspaceExplorerTotals,
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
pub struct PipelineWaveSummary {
    pub wave_index: u64,
    pub summary: RunDueTasksSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunPipelineWavesSummary {
    pub requested_max_waves: u64,
    pub requested_max_tasks_per_wave: u64,
    pub max_waves: u64,
    pub max_tasks_per_wave: u64,
    pub max_total_tasks: u64,
    pub stop_on_first_failure: bool,
    pub waves_executed: u64,
    pub total_claimed: u64,
    pub total_succeeded: u64,
    pub total_retry_scheduled: u64,
    pub total_failed: u64,
    pub total_blocked: u64,
    pub total_skipped: u64,
    pub total_stuck_reconciled: u64,
    pub total_errors: u64,
    pub stopped_reason: String,
    pub wave_summaries: Vec<PipelineWaveSummary>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunPipelineWavesResult {
    pub summary: Option<RunPipelineWavesSummary>,
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
pub struct StageRunOutputArtifact {
    pub entity_file_id: i64,
    pub entity_id: String,
    pub artifact_id: Option<String>,
    pub target_stage_id: String,
    pub relation_to_source: Option<String>,
    pub storage_provider: StorageProvider,
    pub bucket: Option<String>,
    pub key: Option<String>,
    pub s3_uri: Option<String>,
    pub version_id: Option<String>,
    pub etag: Option<String>,
    pub checksum_sha256: Option<String>,
    pub size: Option<u64>,
    pub runtime_status: Option<String>,
    pub producer_run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageRunOutputsPayload {
    pub run_id: String,
    pub output_count: u64,
    pub outputs: Vec<StageRunOutputArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageRunOutputsResult {
    pub payload: Option<StageRunOutputsPayload>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconcileStuckTasksResult {
    pub reconciled: u64,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManualEntityStageActionResult {
    pub detail: Option<EntityDetailPayload>,
    pub summary: Option<RunDueTasksSummary>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenEntityPathPayload {
    pub opened_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenEntityPathResult {
    pub payload: Option<OpenEntityPathPayload>,
    pub errors: Vec<CommandErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveEntityFileJsonResult {
    pub detail: Option<EntityDetailPayload>,
    pub errors: Vec<CommandErrorInfo>,
}

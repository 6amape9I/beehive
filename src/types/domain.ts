export type StageStatus =
  | "pending"
  | "queued"
  | "in_progress"
  | "retry_wait"
  | "done"
  | "failed"
  | "blocked"
  | "skipped";

export type ValidationSeverity = "info" | "warning" | "error";
export type WorkdirHealthSeverity = "info" | "warning" | "error";
export type EntityValidationStatus = "valid" | "warning" | "invalid";
export type AppEventLevel = "info" | "warning" | "error";
export type FileCopyStatus = "created" | "already_exists" | "blocked" | "failed";

export interface ConfigValidationIssue {
  severity: ValidationSeverity;
  code: string;
  path: string;
  message: string;
}

export interface ConfigValidationResult {
  is_valid: boolean;
  issues: ConfigValidationIssue[];
}

export interface ProjectConfig {
  name: string;
  workdir: string;
}

export interface RuntimeConfig {
  scan_interval_sec: number;
  max_parallel_tasks: number;
  stuck_task_timeout_sec: number;
  request_timeout_sec: number;
  file_stability_delay_ms: number;
}

export type StorageProvider = "local" | "s3";

export interface StorageConfig {
  provider: StorageProvider;
  bucket: string | null;
  workspace_prefix: string | null;
  region: string | null;
  endpoint: string | null;
}

export interface StageDefinition {
  id: string;
  input_folder: string;
  input_uri?: string | null;
  output_folder: string;
  workflow_url: string;
  max_attempts: number;
  retry_delay_sec: number;
  next_stage: string | null;
  save_path_aliases?: string[];
  allow_empty_outputs?: boolean;
}

export interface PipelineConfig {
  project: ProjectConfig;
  storage?: StorageConfig | null;
  runtime: RuntimeConfig;
  stages: StageDefinition[];
}

export interface ProjectConfigDraft {
  name: string;
  workdir: string;
}

export interface RuntimeConfigDraft {
  scan_interval_sec: number;
  max_parallel_tasks: number;
  stuck_task_timeout_sec: number;
  request_timeout_sec: number;
  file_stability_delay_ms: number;
}

export interface StageDefinitionDraft {
  id: string;
  input_folder: string;
  input_uri?: string | null;
  output_folder: string;
  workflow_url: string;
  max_attempts: number;
  retry_delay_sec: number;
  next_stage: string | null;
  save_path_aliases?: string[];
  allow_empty_outputs?: boolean;
  original_stage_id: string | null;
  is_new: boolean;
}

export interface PipelineConfigDraft {
  project: ProjectConfigDraft;
  storage?: StorageConfig | null;
  runtime: RuntimeConfigDraft;
  stages: StageDefinitionDraft[];
}

export interface StageUsageSummary {
  stage_id: string;
  is_active: boolean;
  entity_count: number;
  entity_file_count: number;
  stage_state_count: number;
  run_count: number;
  last_seen_in_config_at: string | null;
  archived_at: string | null;
  can_remove_from_config: boolean;
  can_rename: boolean;
  warnings: string[];
}

export interface PipelineEditorState {
  config: PipelineConfig | null;
  draft: PipelineConfigDraft | null;
  yaml_text: string;
  yaml_preview: string;
  validation: ConfigValidationResult;
  stage_usages: StageUsageSummary[];
  loaded_at: string;
}

export interface PipelineEditorStateResult {
  state: PipelineEditorState | null;
  errors: CommandErrorInfo[];
}

export interface ValidatePipelineConfigDraftResult {
  validation: ConfigValidationResult;
  normalized_config: PipelineConfig | null;
  yaml_preview: string | null;
  stage_usages: StageUsageSummary[];
  errors: CommandErrorInfo[];
}

export interface SavePipelineConfigResult {
  state: PipelineEditorState | null;
  backup_path: string | null;
  errors: CommandErrorInfo[];
}

export interface WorkdirHealthIssue {
  severity: WorkdirHealthSeverity;
  code: string;
  path: string;
  message: string;
}

export interface WorkdirState {
  workdir_path: string;
  pipeline_config_path: string;
  database_path: string;
  stages_dir_path: string;
  logs_dir_path: string;
  exists: boolean;
  pipeline_config_exists: boolean;
  database_exists: boolean;
  stages_dir_exists: boolean;
  logs_dir_exists: boolean;
  health_issues: WorkdirHealthIssue[];
}

export interface DatabaseState {
  database_path: string;
  is_ready: boolean;
  schema_version: number;
  stage_count: number;
  synced_stage_ids: string[];
  active_stage_count: number;
  inactive_stage_count: number;
}

export type AppInitializationPhase =
  | "app_not_configured"
  | "config_invalid"
  | "bootstrap_failed"
  | "fully_initialized";

export interface CommandErrorInfo {
  code: string;
  message: string;
  path: string | null;
}

export interface AppInitializationState {
  phase: AppInitializationPhase;
  message: string;
  selected_workdir_path: string | null;
  project_name: string | null;
  config_path: string | null;
  database_path: string | null;
  config_status: string;
  database_status: string;
  stage_count: number;
  stage_ids: string[];
  last_config_load_at: string | null;
  validation: ConfigValidationResult;
  workdir_state: WorkdirState | null;
  database_state: DatabaseState | null;
  config: PipelineConfig | null;
  errors: CommandErrorInfo[];
}

export interface BootstrapResult {
  state: AppInitializationState;
}

export interface StageRecord {
  id: string;
  input_folder: string;
  input_uri: string | null;
  output_folder: string;
  workflow_url: string;
  max_attempts: number;
  retry_delay_sec: number;
  next_stage: string | null;
  save_path_aliases: string[];
  allow_empty_outputs: boolean;
  is_active: boolean;
  archived_at: string | null;
  last_seen_in_config_at: string | null;
  created_at: string;
  updated_at: string;
  entity_count: number;
}

export interface EntityRecord {
  entity_id: string;
  current_stage_id: string | null;
  current_status: string;
  latest_file_path: string | null;
  latest_file_id: number | null;
  file_count: number;
  validation_status: EntityValidationStatus;
  validation_errors: ConfigValidationIssue[];
  first_seen_at: string;
  last_seen_at: string;
  updated_at: string;
}

export type EntityListSortBy =
  | "entity_id"
  | "current_stage"
  | "status"
  | "updated_at"
  | "last_seen_at"
  | "attempts"
  | "last_error";

export type SortDirection = "asc" | "desc";

export interface EntityListQuery {
  search?: string | null;
  stage_id?: string | null;
  status?: string | null;
  validation_status?: EntityValidationStatus | null;
  sort_by?: EntityListSortBy | null;
  sort_direction?: SortDirection | null;
  page?: number;
  page_size?: number;
}

export interface EntityTableRow {
  entity_id: string;
  display_name: string | null;
  current_stage_id: string | null;
  current_status: string;
  latest_file_path: string | null;
  latest_file_id: number | null;
  file_count: number;
  attempts: number | null;
  max_attempts: number | null;
  last_error: string | null;
  last_http_status: number | null;
  next_retry_at: string | null;
  last_started_at: string | null;
  last_finished_at: string | null;
  validation_status: EntityValidationStatus;
  updated_at: string;
  last_seen_at: string;
}

export interface EntityFileRecord {
  id: number;
  entity_id: string;
  stage_id: string;
  file_path: string;
  file_name: string;
  artifact_id: string | null;
  relation_to_source: string | null;
  storage_provider: StorageProvider;
  bucket: string | null;
  key: string | null;
  version_id: string | null;
  etag: string | null;
  checksum_sha256: string | null;
  checksum: string;
  file_mtime: string;
  file_size: number;
  artifact_size: number | null;
  payload_json: string;
  meta_json: string;
  current_stage: string | null;
  next_stage: string | null;
  status: string;
  validation_status: EntityValidationStatus;
  validation_errors: ConfigValidationIssue[];
  is_managed_copy: boolean;
  copy_source_file_id: number | null;
  producer_run_id: string | null;
  file_exists: boolean;
  missing_since: string | null;
  first_seen_at: string;
  last_seen_at: string;
  updated_at: string;
}

export interface EntityStageStateRecord {
  id: number;
  entity_id: string;
  stage_id: string;
  file_path: string;
  file_instance_id: number | null;
  file_exists: boolean;
  status: string;
  attempts: number;
  max_attempts: number;
  last_error: string | null;
  last_http_status: number | null;
  next_retry_at: string | null;
  last_started_at: string | null;
  last_finished_at: string | null;
  created_child_path: string | null;
  discovered_at: string;
  last_seen_at: string | null;
  updated_at: string;
}

export interface EntityTimelineItem {
  stage_id: string;
  status: string;
  attempts: number;
  max_attempts: number;
  file_path: string | null;
  file_exists: boolean;
  last_error: string | null;
  last_http_status: number | null;
  next_retry_at: string | null;
  last_started_at: string | null;
  last_finished_at: string | null;
  created_child_path: string | null;
  updated_at: string;
}

export interface EntityStageAllowedActions {
  stage_id: string;
  can_retry_now: boolean;
  can_reset_to_pending: boolean;
  can_skip: boolean;
  can_run_this_stage: boolean;
  reasons: string[];
}

export interface EntityFileAllowedActions {
  entity_file_id: number;
  can_edit_business_json: boolean;
  can_open_file: boolean;
  can_open_folder: boolean;
  reasons: string[];
}

export interface AppEventRecord {
  id: number;
  level: AppEventLevel;
  code: string;
  message: string;
  context: Record<string, unknown> | null;
  created_at: string;
}

export interface StatusCount {
  status: string;
  count: number;
}

export interface RuntimeSummary {
  schema_version: number;
  active_stage_count: number;
  inactive_stage_count: number;
  total_entities: number;
  present_file_count: number;
  missing_file_count: number;
  managed_copy_count: number;
  invalid_file_count: number;
  entities_by_status: StatusCount[];
  execution_status_counts: StatusCount[];
  last_reconciliation_at: string | null;
}

export type DashboardStageHealth = "ok" | "warning" | "error" | "inactive";

export interface DashboardProjectContext {
  name: string;
  workdir_path: string;
}

export interface DashboardTotals {
  entities_total: number;
  entity_files_total: number;
  stages_total: number;
  active_stages_total: number;
  inactive_stages_total: number;
  active_tasks_total: number;
  errors_total: number;
  warnings_total: number;
}

export interface DashboardRuntimeOverview {
  last_scan_at: string | null;
  last_run_at: string | null;
  last_successful_run_at: string | null;
  last_error_at: string | null;
  due_tasks_count: number;
  in_progress_count: number;
  retry_wait_count: number;
  failed_count: number;
  blocked_count: number;
}

export interface DashboardStageNode {
  id: string;
  label: string;
  input_folder: string;
  output_folder: string | null;
  workflow_url: string | null;
  is_active: boolean;
  archived_at: string | null;
  next_stage: string | null;
  position_index: number;
  health: DashboardStageHealth;
}

export interface DashboardStageEdge {
  from_stage_id: string;
  to_stage_id: string;
  is_valid: boolean;
  problem: string | null;
}

export interface DashboardStageGraph {
  nodes: DashboardStageNode[];
  edges: DashboardStageEdge[];
}

export interface DashboardStageCounters {
  stage_id: string;
  stage_label: string;
  is_active: boolean;
  total: number;
  pending: number;
  queued: number;
  in_progress: number;
  retry_wait: number;
  done: number;
  failed: number;
  blocked: number;
  skipped: number;
  unknown: number;
  missing_files: number;
  existing_files: number;
  last_started_at: string | null;
  last_finished_at: string | null;
}

export interface DashboardActiveTask {
  entity_id: string;
  stage_id: string;
  status: string;
  attempts: number;
  max_attempts: number;
  next_retry_at: string | null;
  last_started_at: string | null;
  updated_at: string | null;
  file_path: string | null;
  reason: string | null;
}

export interface DashboardErrorItem {
  id: number;
  level: string;
  event_type: string;
  message: string;
  entity_id: string | null;
  stage_id: string | null;
  run_id: string | null;
  created_at: string;
}

export interface DashboardRunItem {
  run_id: string;
  entity_id: string;
  stage_id: string;
  success: boolean;
  http_status: number | null;
  error_type: string | null;
  error_message: string | null;
  started_at: string;
  finished_at: string | null;
  duration_ms: number | null;
}

export interface DashboardOverview {
  generated_at: string;
  project: DashboardProjectContext;
  totals: DashboardTotals;
  runtime: DashboardRuntimeOverview;
  stage_graph: DashboardStageGraph;
  stage_counters: DashboardStageCounters[];
  active_tasks: DashboardActiveTask[];
  last_errors: DashboardErrorItem[];
  recent_runs: DashboardRunItem[];
}

export interface EntityFilters {
  stage_id?: string | null;
  status?: string | null;
  validation_status?: EntityValidationStatus | null;
  search?: string | null;
}

export interface ScanSummary {
  scan_id: string;
  scanned_file_count: number;
  registered_file_count: number;
  registered_entity_count: number;
  updated_file_count: number;
  unchanged_file_count: number;
  missing_file_count: number;
  restored_file_count: number;
  invalid_count: number;
  duplicate_count: number;
  created_directory_count: number;
  managed_copy_count: number;
  elapsed_ms: number;
  latest_discovery_at: string;
}

export interface S3ReconciliationSummary {
  scan_id: string;
  stage_count: number;
  listed_object_count: number;
  metadata_tagged_count: number;
  registered_file_count: number;
  updated_file_count: number;
  unchanged_file_count: number;
  missing_file_count: number;
  restored_file_count: number;
  unmapped_object_count: number;
  elapsed_ms: number;
  latest_reconciliation_at: string;
}

export interface StageDirectoryProvisionSummary {
  created_paths: string[];
  created_directory_count: number;
}

export interface ScanWorkspaceResult {
  summary: ScanSummary | null;
  errors: CommandErrorInfo[];
}

export interface S3ReconciliationResult {
  summary: S3ReconciliationSummary | null;
  errors: CommandErrorInfo[];
}

export interface StageDirectoryProvisionResult {
  summary: StageDirectoryProvisionSummary | null;
  errors: CommandErrorInfo[];
}

export interface RuntimeSummaryResult {
  summary: RuntimeSummary | null;
  errors: CommandErrorInfo[];
}

export interface DashboardOverviewResult {
  overview: DashboardOverview | null;
  errors: CommandErrorInfo[];
}

export interface StageListResult {
  stages: StageRecord[];
  errors: CommandErrorInfo[];
}

export interface EntityListResult {
  entities: EntityTableRow[];
  total: number;
  page: number;
  page_size: number;
  available_stages: string[];
  available_statuses: string[];
  errors: CommandErrorInfo[];
}

export interface EntityFilesResult {
  files: EntityFileRecord[];
  errors: CommandErrorInfo[];
}

export interface RegisterS3SourceArtifactRequest {
  stage_id: string;
  entity_id: string;
  artifact_id: string;
  bucket: string;
  key: string;
  version_id?: string | null;
  etag?: string | null;
  checksum_sha256?: string | null;
  size?: number | null;
}

export interface RegisterS3SourceArtifactPayload {
  file: EntityFileRecord;
}

export interface RegisterS3SourceArtifactResult {
  payload: RegisterS3SourceArtifactPayload | null;
  errors: CommandErrorInfo[];
}

export interface EntityDetailPayload {
  entity: EntityRecord;
  files: EntityFileRecord[];
  stage_states: EntityStageStateRecord[];
  stage_runs: StageRunRecord[];
  timeline: EntityTimelineItem[];
  latest_json_preview: string;
  selected_file_json: string | null;
  allowed_actions: EntityStageAllowedActions[];
  file_allowed_actions: EntityFileAllowedActions[];
}

export interface EntityDetailResult {
  detail: EntityDetailPayload | null;
  errors: CommandErrorInfo[];
}

export interface FileCopyPayload {
  status: FileCopyStatus;
  entity_id: string;
  source_stage_id: string;
  target_stage_id: string | null;
  source_file_path: string | null;
  target_file_path: string | null;
  target_file: EntityFileRecord | null;
  message: string;
}

export interface FileCopyResult {
  payload: FileCopyPayload | null;
  errors: CommandErrorInfo[];
}

export interface AppEventsResult {
  events: AppEventRecord[];
  errors: CommandErrorInfo[];
}

export interface WorkspaceFileNode {
  entity_file_id: number;
  entity_id: string;
  stage_id: string;
  file_name: string;
  file_path: string;
  storage_provider: StorageProvider;
  bucket: string | null;
  key: string | null;
  artifact_id: string | null;
  relation_to_source: string | null;
  producer_run_id: string | null;
  file_exists: boolean;
  missing_since: string | null;
  is_managed_copy: boolean;
  copy_source_file_id: number | null;
  copy_source_entity_id: string | null;
  copy_source_stage_id: string | null;
  runtime_status: string | null;
  file_status: string;
  validation_status: EntityValidationStatus;
  validation_errors: ConfigValidationIssue[];
  current_stage: string | null;
  next_stage: string | null;
  checksum: string;
  file_size: number;
  file_mtime: string;
  updated_at: string;
  can_open_file: boolean;
  can_open_folder: boolean;
}

export interface InvalidDiscoveryRecord {
  stage_id: string | null;
  file_name: string;
  file_path: string;
  code: string;
  message: string;
  created_at: string;
}

export interface WorkspaceStageTreeCounters {
  registered_files: number;
  present_files: number;
  missing_files: number;
  invalid_files: number;
  managed_copies: number;
  pending: number;
  queued: number;
  in_progress: number;
  retry_wait: number;
  done: number;
  failed: number;
  blocked: number;
  skipped: number;
}

export interface WorkspaceStageTree {
  stage_id: string;
  input_folder: string;
  input_uri: string | null;
  storage_provider: StorageProvider;
  output_folder: string | null;
  workflow_url: string | null;
  next_stage: string | null;
  is_active: boolean;
  archived_at: string | null;
  folder_path: string;
  folder_exists: boolean;
  files: WorkspaceFileNode[];
  invalid_files: InvalidDiscoveryRecord[];
  counters: WorkspaceStageTreeCounters;
}

export interface WorkspaceEntityTrailNode {
  entity_file_id: number;
  stage_id: string;
  file_name: string;
  file_path: string;
  file_exists: boolean;
  runtime_status: string | null;
  is_managed_copy: boolean;
  can_open_file: boolean;
  can_open_folder: boolean;
}

export interface WorkspaceEntityTrailEdge {
  from_entity_file_id: number;
  to_entity_file_id: number;
  relation: string;
  created_child_path: string | null;
}

export interface WorkspaceEntityTrail {
  entity_id: string;
  file_count: number;
  stages: WorkspaceEntityTrailNode[];
  edges: WorkspaceEntityTrailEdge[];
}

export interface WorkspaceExplorerTotals {
  stages_total: number;
  active_stages_total: number;
  inactive_stages_total: number;
  entities_total: number;
  registered_files_total: number;
  present_files_total: number;
  missing_files_total: number;
  invalid_files_total: number;
  managed_copies_total: number;
}

export interface WorkspaceExplorerResult {
  generated_at: string;
  workdir_path: string;
  last_scan_at: string | null;
  stages: WorkspaceStageTree[];
  entity_trails: WorkspaceEntityTrail[];
  totals: WorkspaceExplorerTotals;
  errors: CommandErrorInfo[];
}

export interface StageRunRecord {
  id: number;
  run_id: string;
  entity_id: string;
  entity_file_id: number | null;
  stage_id: string;
  attempt_no: number;
  workflow_url: string;
  request_json: string;
  response_json: string | null;
  http_status: number | null;
  success: boolean;
  error_type: string | null;
  error_message: string | null;
  started_at: string;
  finished_at: string | null;
  duration_ms: number | null;
}

export interface RunDueTasksSummary {
  claimed: number;
  succeeded: number;
  retry_scheduled: number;
  failed: number;
  blocked: number;
  skipped: number;
  stuck_reconciled: number;
  errors: CommandErrorInfo[];
}

export interface RunDueTasksResult {
  summary: RunDueTasksSummary | null;
  errors: CommandErrorInfo[];
}

export interface RunEntityStageResult {
  summary: RunDueTasksSummary | null;
  errors: CommandErrorInfo[];
}

export interface StageRunsResult {
  runs: StageRunRecord[];
  errors: CommandErrorInfo[];
}

export interface ReconcileStuckTasksResult {
  reconciled: number;
  errors: CommandErrorInfo[];
}

export interface ManualEntityStageActionResult {
  detail: EntityDetailPayload | null;
  summary: RunDueTasksSummary | null;
  errors: CommandErrorInfo[];
}

export interface OpenEntityPathPayload {
  opened_path: string;
}

export interface OpenEntityPathResult {
  payload: OpenEntityPathPayload | null;
  errors: CommandErrorInfo[];
}

export interface SaveEntityFileJsonResult {
  detail: EntityDetailPayload | null;
  errors: CommandErrorInfo[];
}

export const notConfiguredState: AppInitializationState = {
  phase: "app_not_configured",
  message: "No workdir is selected.",
  selected_workdir_path: null,
  project_name: null,
  config_path: null,
  database_path: null,
  config_status: "not_loaded",
  database_status: "not_ready",
  stage_count: 0,
  stage_ids: [],
  last_config_load_at: null,
  validation: {
    is_valid: true,
    issues: [],
  },
  workdir_state: null,
  database_state: null,
  config: null,
  errors: [],
};

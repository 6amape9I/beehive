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
}

export interface StageDefinition {
  id: string;
  input_folder: string;
  output_folder: string;
  workflow_url: string;
  max_attempts: number;
  retry_delay_sec: number;
  next_stage: string | null;
}

export interface PipelineConfig {
  project: ProjectConfig;
  runtime: RuntimeConfig;
  stages: StageDefinition[];
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
  output_folder: string;
  workflow_url: string;
  max_attempts: number;
  retry_delay_sec: number;
  next_stage: string | null;
  is_active: boolean;
  archived_at: string | null;
  last_seen_in_config_at: string | null;
  created_at: string;
  updated_at: string;
  entity_count: number;
}

export interface EntityRecord {
  entity_id: string;
  file_path: string;
  file_name: string;
  stage_id: string;
  current_stage: string | null;
  next_stage: string | null;
  status: string;
  checksum: string;
  file_mtime: string;
  file_size: number;
  payload_json: string;
  meta_json: string;
  validation_status: EntityValidationStatus;
  validation_errors: ConfigValidationIssue[];
  discovered_at: string;
  updated_at: string;
}

export interface EntityStageStateRecord {
  id: number;
  entity_id: string;
  stage_id: string;
  file_path: string;
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
  updated_at: string;
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
  total_registered_entities: number;
  entities_by_status: StatusCount[];
  latest_discovery_at: string | null;
  discovery_error_count: number;
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
  registered_count: number;
  updated_count: number;
  unchanged_count: number;
  invalid_count: number;
  duplicate_count: number;
  elapsed_ms: number;
  latest_discovery_at: string;
}

export interface ScanWorkspaceResult {
  summary: ScanSummary | null;
  errors: CommandErrorInfo[];
}

export interface RuntimeSummaryResult {
  summary: RuntimeSummary | null;
  errors: CommandErrorInfo[];
}

export interface StageListResult {
  stages: StageRecord[];
  errors: CommandErrorInfo[];
}

export interface EntityListResult {
  entities: EntityRecord[];
  total: number;
  available_stages: string[];
  errors: CommandErrorInfo[];
}

export interface EntityDetailPayload {
  entity: EntityRecord;
  stage_states: EntityStageStateRecord[];
  json_preview: string;
}

export interface EntityDetailResult {
  detail: EntityDetailPayload | null;
  errors: CommandErrorInfo[];
}

export interface AppEventsResult {
  events: AppEventRecord[];
  errors: CommandErrorInfo[];
}

export interface WorkspaceFileRecord {
  entity_id: string;
  file_name: string;
  file_path: string;
  status: string;
  validation_status: EntityValidationStatus;
  updated_at: string;
}

export interface InvalidDiscoveryRecord {
  stage_id: string | null;
  file_name: string;
  file_path: string;
  code: string;
  message: string;
  created_at: string;
}

export interface WorkspaceStageGroup {
  stage: StageRecord;
  files: WorkspaceFileRecord[];
  invalid_files: InvalidDiscoveryRecord[];
}

export interface WorkspaceExplorerResult {
  groups: WorkspaceStageGroup[];
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

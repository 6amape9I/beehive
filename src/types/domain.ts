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
}

export type AppInitializationPhase =
  | "app_not_configured"
  | "workdir_selected"
  | "config_loaded"
  | "config_invalid"
  | "database_ready"
  | "bootstrap_failed"
  | "fully_initialized";

export interface BootstrapErrorInfo {
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
  config_loaded: boolean;
  config_status: string;
  database_status: string;
  stage_count: number;
  stage_ids: string[];
  last_config_load_at: string | null;
  validation: ConfigValidationResult;
  workdir_state: WorkdirState | null;
  database_state: DatabaseState | null;
  config: PipelineConfig | null;
  errors: BootstrapErrorInfo[];
}

export interface BootstrapResult {
  state: AppInitializationState;
}

export const notConfiguredState: AppInitializationState = {
  phase: "app_not_configured",
  message: "No workdir is selected.",
  selected_workdir_path: null,
  project_name: null,
  config_path: null,
  database_path: null,
  config_loaded: false,
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

use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;
use uuid::Uuid;

use crate::database;
use crate::domain::{ResourceClass, RunDueTasksSummary, WorkerStartRequest, WorkerSummary};
use crate::executor;
use crate::services::{runtime, workspaces};

const DEFAULT_IDLE_SLEEP_MS: u64 = 1000;
const DEFAULT_RECOVERY_INTERVAL_SEC: u64 = 30;
const DEFAULT_RETENTION_INTERVAL_SEC: u64 = 300;
const OPERATOR_REPAIR_WRITE_GATE_TIMEOUT_SEC: u64 = 5;
const WORKER_CONTEXT_LOG_INTERVAL_SEC: u64 = 60;
const WORKER_IDLE_LOG_INTERVAL_SEC: u64 = 60;
pub(crate) const BROAD_RUN_DISABLED_CODE: &str = "workers_enabled_broad_run_disabled";
pub(crate) const BROAD_RUN_DISABLED_MESSAGE: &str =
    "Workers are enabled for this workspace. Use selected run or worker pools instead.";

#[derive(Debug, Clone)]
struct WorkerEnvConfig {
    enabled: bool,
    workspace_scope: Option<Vec<String>>,
    default_concurrency: Option<u32>,
    local_llm_concurrency: Option<u32>,
    idle_sleep_ms: u64,
    recovery_interval_sec: u64,
    retention_interval_sec: u64,
}

pub(crate) fn worker_summary(workspace_id: &str) -> Result<WorkerSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    let mut summary = database::get_worker_summary(&context.database_path, &context.config)?;
    summary.workers_enabled = workspace_workers_enabled(workspace_id);
    summary.broad_runs_disabled = summary.workers_enabled;
    apply_env_effective_concurrency(&mut summary, &context.config.runtime.worker_pools)?;
    summary.runtime_status = runtime_status(&summary);
    Ok(summary)
}

pub(crate) fn recover_expired_leases(workspace_id: &str) -> Result<u64, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    database::recover_expired_worker_leases(&context.database_path)
}

pub(crate) fn start_workers(
    workspace_id: &str,
    input: &WorkerStartRequest,
) -> Result<WorkerSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    let env_config = WorkerEnvConfig::from_env()?;
    let default_limit = effective_concurrency(
        context.config.runtime.worker_pools.default.concurrency,
        env_config.default_concurrency,
    );
    let local_llm_limit = effective_concurrency(
        context.config.runtime.worker_pools.local_llm.concurrency,
        env_config.local_llm_concurrency,
    );
    database::set_all_worker_pools_started(
        &context.database_path,
        input.default_workers.min(default_limit),
        input.local_llm_workers.min(local_llm_limit),
    )?;
    let mut summary = worker_summary(workspace_id)?;
    annotate_requested_desired(
        &mut summary,
        &[
            (ResourceClass::Default, input.default_workers),
            (ResourceClass::LocalLlm, input.local_llm_workers),
        ],
    );
    Ok(summary)
}

pub(crate) fn stop_workers(workspace_id: &str) -> Result<WorkerSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    database::set_all_worker_pools_stopped(&context.database_path)?;
    worker_summary(workspace_id)
}

pub(crate) fn update_pool_desired_concurrency(
    workspace_id: &str,
    resource_class: ResourceClass,
    desired_concurrency: u32,
) -> Result<WorkerSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    let env_config = WorkerEnvConfig::from_env()?;
    let configured = match resource_class {
        ResourceClass::Default => context.config.runtime.worker_pools.default.concurrency,
        ResourceClass::LocalLlm => context.config.runtime.worker_pools.local_llm.concurrency,
    };
    let env_value = match resource_class {
        ResourceClass::Default => env_config.default_concurrency,
        ResourceClass::LocalLlm => env_config.local_llm_concurrency,
    };
    let limit = effective_concurrency(configured, env_value);
    database::update_worker_pool_desired_concurrency(
        &context.database_path,
        resource_class,
        desired_concurrency.min(limit),
    )?;
    let mut summary = worker_summary(workspace_id)?;
    annotate_requested_desired(&mut summary, &[(resource_class, desired_concurrency)]);
    Ok(summary)
}

pub(crate) fn pause_all(workspace_id: &str, reason: Option<&str>) -> Result<WorkerSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    database::set_all_worker_pools_paused(&context.database_path, true, reason)?;
    worker_summary(workspace_id)
}

pub(crate) fn resume_all(workspace_id: &str) -> Result<WorkerSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    database::set_all_worker_pools_paused(&context.database_path, false, None)?;
    worker_summary(workspace_id)
}

pub(crate) fn pause_pool(
    workspace_id: &str,
    resource_class: ResourceClass,
    reason: Option<&str>,
) -> Result<WorkerSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    database::set_worker_pool_paused(&context.database_path, resource_class, true, reason)?;
    worker_summary(workspace_id)
}

pub(crate) fn resume_pool(
    workspace_id: &str,
    resource_class: ResourceClass,
) -> Result<WorkerSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    database::set_worker_pool_paused(&context.database_path, resource_class, false, None)?;
    worker_summary(workspace_id)
}

pub(crate) fn release_lease(
    workspace_id: &str,
    lease_id: &str,
    reason: &str,
) -> Result<bool, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    database::release_worker_lease(&context.database_path, lease_id, reason)
}

pub(crate) fn reconcile_stuck(workspace_id: &str) -> Result<(u64, WorkerSummary), String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    let reconciled = database::reconcile_stuck_worker_states(
        &context.database_path,
        context.config.runtime.worker_lease_sec,
    )?;
    let summary = worker_summary(workspace_id)?;
    Ok((reconciled, summary))
}

pub(crate) fn repair_workers(workspace_id: &str) -> Result<(u64, WorkerSummary), String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    let repaired = database::repair_worker_leases_with_gate_timeout(
        &context.database_path,
        workspace_id,
        std::process::id(),
        context.config.runtime.worker_lease_sec,
        Duration::from_secs(OPERATOR_REPAIR_WRITE_GATE_TIMEOUT_SEC),
    )?;
    let summary = worker_summary(workspace_id)?;
    Ok((repaired, summary))
}

pub(crate) fn workspace_workers_enabled(workspace_id: &str) -> bool {
    let enabled = env_flag("BEEHIVE_WORKERS_ENABLED");
    let scope = parse_workspace_scope(std::env::var("BEEHIVE_WORKER_WORKSPACES").ok());
    workers_enabled_for_scope(enabled, scope.as_deref(), workspace_id)
}

pub(crate) fn is_broad_run_disabled_error(message: &str) -> bool {
    message == BROAD_RUN_DISABLED_MESSAGE
}

pub fn start_from_env() -> Result<(), String> {
    let config = WorkerEnvConfig::from_env()?;
    if !config.enabled {
        log_worker_manager("disabled", "BEEHIVE_WORKERS_ENABLED is not set.");
        return Ok(());
    }

    let Some(scope) = config.workspace_scope.clone() else {
        log_worker_manager(
            "disabled_no_workspace_scope",
            "BEEHIVE_WORKER_WORKSPACES is not set; workers were not started.",
        );
        return Ok(());
    };
    let workspace_ids = resolve_workspace_scope(&scope)?;
    if workspace_ids.is_empty() {
        log_worker_manager(
            "disabled_empty_scope",
            "No workspaces matched worker scope.",
        );
        return Ok(());
    }

    for workspace_id in workspace_ids {
        start_workspace_workers(workspace_id, config.clone())?;
    }
    Ok(())
}

fn start_workspace_workers(workspace_id: String, config: WorkerEnvConfig) -> Result<(), String> {
    let context = runtime::load_workspace_context(&workspace_id)?;
    let default_concurrency = effective_concurrency(
        context.config.runtime.worker_pools.default.concurrency,
        config.default_concurrency,
    );
    let local_llm_concurrency = effective_concurrency(
        context.config.runtime.worker_pools.local_llm.concurrency,
        config.local_llm_concurrency,
    );

    let mut maintenance_owner_assigned = false;
    for index in 0..default_concurrency {
        let maintenance_owner = !maintenance_owner_assigned;
        maintenance_owner_assigned = true;
        spawn_worker_loop(
            &workspace_id,
            ResourceClass::Default,
            index,
            config.clone(),
            maintenance_owner,
        )?;
    }
    for index in 0..local_llm_concurrency {
        let maintenance_owner = !maintenance_owner_assigned;
        maintenance_owner_assigned = true;
        spawn_worker_loop(
            &workspace_id,
            ResourceClass::LocalLlm,
            index,
            config.clone(),
            maintenance_owner,
        )?;
    }
    log_worker_manager(
        "started",
        &format!(
            "Started workers for workspace '{workspace_id}': default={default_concurrency}, local_llm={local_llm_concurrency}."
        ),
    );
    Ok(())
}

fn spawn_worker_loop(
    workspace_id: &str,
    resource_class: ResourceClass,
    index: u32,
    config: WorkerEnvConfig,
    maintenance_owner: bool,
) -> Result<(), String> {
    let workspace_id = workspace_id.to_string();
    let worker_id = worker_id(&workspace_id, resource_class, index);
    thread::Builder::new()
        .name(worker_id.clone())
        .spawn(move || {
            worker_loop(
                workspace_id,
                resource_class,
                worker_id,
                config,
                maintenance_owner,
            )
        })
        .map(|_| ())
        .map_err(|error| format!("Failed to spawn worker thread: {error}"))
}

fn worker_loop(
    workspace_id: String,
    resource_class: ResourceClass,
    worker_id: String,
    config: WorkerEnvConfig,
    maintenance_owner: bool,
) {
    let idle_sleep = Duration::from_millis(config.idle_sleep_ms.max(100));
    let recovery_interval = Duration::from_secs(config.recovery_interval_sec.max(1));
    let retention_interval = Duration::from_secs(config.retention_interval_sec.max(60));
    let mut last_recovery = Instant::now() - recovery_interval;
    let mut last_retention = Instant::now();
    let context_log_interval = Duration::from_secs(WORKER_CONTEXT_LOG_INTERVAL_SEC);
    let idle_log_interval = Duration::from_secs(WORKER_IDLE_LOG_INTERVAL_SEC);
    let mut last_context_log = Instant::now() - context_log_interval;
    let mut last_context_error_log = Instant::now() - context_log_interval;
    let mut last_idle_log = Instant::now() - idle_log_interval;

    log_worker_event(
        "worker_loop_started",
        Some(&worker_id),
        Some(json!({
            "workspace_id": workspace_id,
            "resource_class": resource_class.as_str(),
        })),
    );

    loop {
        if maintenance_owner && last_retention.elapsed() >= retention_interval {
            run_retention_prune(&workspace_id, &worker_id, &mut last_retention);
        }
        let log_context_loaded = last_context_log.elapsed() >= context_log_interval;
        let summary = match run_worker_loop_once(
            &workspace_id,
            resource_class,
            &worker_id,
            &mut last_recovery,
            recovery_interval,
            log_context_loaded,
        ) {
            Ok(summary) => summary,
            Err(message) => {
                if last_context_error_log.elapsed() >= context_log_interval {
                    log_worker_event(
                        "worker_context_error",
                        Some(&worker_id),
                        Some(json!({
                            "workspace_id": workspace_id,
                            "resource_class": resource_class.as_str(),
                            "message": message,
                        })),
                    );
                    last_context_error_log = Instant::now();
                }
                thread::sleep(idle_sleep);
                continue;
            }
        };
        if log_context_loaded {
            last_context_log = Instant::now();
        }
        if worker_was_idle(&summary) {
            if last_idle_log.elapsed() >= idle_log_interval {
                log_worker_event(
                    "worker_claim_idle",
                    Some(&worker_id),
                    Some(json!({
                        "workspace_id": workspace_id,
                        "resource_class": resource_class.as_str(),
                    })),
                );
                last_idle_log = Instant::now();
            }
            thread::sleep(idle_sleep);
        }
    }
}

fn run_retention_prune(workspace_id: &str, worker_id: &str, last_retention: &mut Instant) {
    match runtime::load_worker_runtime_context(workspace_id)
        .and_then(|context| database::prune_runtime_history(&context.database_path))
    {
        Ok(summary) => {
            if summary.app_events_deleted > 0 || summary.worker_leases_deleted > 0 {
                log_worker_event(
                    "runtime_retention_pruned",
                    Some(worker_id),
                    Some(json!({
                        "workspace_id": workspace_id,
                        "app_events_deleted": summary.app_events_deleted,
                        "worker_leases_deleted": summary.worker_leases_deleted,
                    })),
                );
            }
        }
        Err(message) => {
            log_worker_event(
                "runtime_retention_prune_failed",
                Some(worker_id),
                Some(json!({
                    "workspace_id": workspace_id,
                    "message": message,
                })),
            );
        }
    }
    *last_retention = Instant::now();
}

fn run_worker_loop_once(
    workspace_id: &str,
    resource_class: ResourceClass,
    worker_id: &str,
    last_recovery: &mut Instant,
    recovery_interval: Duration,
    log_context_loaded: bool,
) -> Result<RunDueTasksSummary, String> {
    let context = runtime::load_worker_runtime_context(workspace_id)?;
    if log_context_loaded {
        log_worker_event(
            "worker_context_loaded",
            Some(worker_id),
            Some(json!({
                "workspace_id": workspace_id,
                "resource_class": resource_class.as_str(),
                "database_path": context.database_path.display().to_string(),
            })),
        );
    }
    if last_recovery.elapsed() >= recovery_interval {
        if let Err(message) = database::recover_expired_worker_leases(&context.database_path) {
            log_worker_event(
                "worker_context_error",
                Some(worker_id),
                Some(json!({
                    "workspace_id": workspace_id,
                    "resource_class": resource_class.as_str(),
                    "message": message,
                    "phase": "recover_expired_worker_leases",
                })),
            );
        }
        *last_recovery = Instant::now();
    }
    match database::worker_pool_is_paused(&context.database_path, resource_class) {
        Ok(true) => return Ok(RunDueTasksSummary::default()),
        Ok(false) => {}
        Err(message) => return Err(message),
    }

    executor::run_worker_task(
        &context.workdir_path,
        &context.database_path,
        resource_class,
        worker_id,
        context.config.runtime.request_timeout_sec,
        context.config.runtime.worker_lease_sec,
        context.config.runtime.worker_heartbeat_sec,
        context.config.runtime.scheduling_policy,
        context.config.runtime.file_stability_delay_ms,
    )
}

fn worker_was_idle(summary: &RunDueTasksSummary) -> bool {
    summary.claimed == 0 && summary.errors.is_empty()
}

fn apply_env_effective_concurrency(
    summary: &mut WorkerSummary,
    configured: &crate::domain::WorkerPoolsConfig,
) -> Result<(), String> {
    let env_config = WorkerEnvConfig::from_env()?;
    for pool in &mut summary.pools {
        let env_value = match pool.resource_class {
            ResourceClass::Default => env_config.default_concurrency,
            ResourceClass::LocalLlm => env_config.local_llm_concurrency,
        };
        pool.env_concurrency_limit = env_value;
        let yaml_limit = match pool.resource_class {
            ResourceClass::Default => configured.default.concurrency,
            ResourceClass::LocalLlm => configured.local_llm.concurrency,
        };
        let upper_bound = effective_concurrency(yaml_limit, env_value);
        pool.effective_concurrency = if pool.is_started {
            pool.desired_concurrency.min(upper_bound)
        } else {
            0
        };
    }
    Ok(())
}

fn annotate_requested_desired(summary: &mut WorkerSummary, requested: &[(ResourceClass, u32)]) {
    for (resource_class, requested_value) in requested {
        if let Some(pool) = summary
            .pools
            .iter_mut()
            .find(|pool| pool.resource_class == *resource_class)
        {
            pool.requested_desired_concurrency = Some(*requested_value);
        }
    }
}

fn runtime_status(summary: &WorkerSummary) -> String {
    let any_started = summary.pools.iter().any(|pool| pool.is_started);
    let any_active = summary.active_leases_total > 0;
    let all_started_paused = summary
        .pools
        .iter()
        .filter(|pool| pool.is_started)
        .all(|pool| pool.is_paused);
    if !summary.workers_enabled {
        return "stopped".to_string();
    }
    if any_started && all_started_paused {
        return "paused".to_string();
    }
    if any_started {
        return "running".to_string();
    }
    if any_active {
        return "draining".to_string();
    }
    "stopped".to_string()
}

fn effective_concurrency(configured: u32, env_value: Option<u32>) -> u32 {
    match env_value {
        Some(value) => configured.min(value),
        None => configured,
    }
}

fn resolve_workspace_scope(scope: &[String]) -> Result<Vec<String>, String> {
    if scope.iter().any(|item| item == "all") {
        return Ok(workspaces::list_workspace_descriptors(false)?
            .into_iter()
            .map(|workspace| workspace.id)
            .collect());
    }
    Ok(scope.to_vec())
}

fn workers_enabled_for_scope(enabled: bool, scope: Option<&[String]>, workspace_id: &str) -> bool {
    enabled
        && scope
            .map(|items| {
                items
                    .iter()
                    .any(|item| item == "all" || item == workspace_id)
            })
            .unwrap_or(false)
}

fn worker_id(workspace_id: &str, resource_class: ResourceClass, index: u32) -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "beehive".to_string());
    let process_id = std::process::id();
    let short_uuid = Uuid::new_v4()
        .to_string()
        .chars()
        .take(8)
        .collect::<String>();
    format!(
        "{}-{}-{}-{}-{}-{}",
        host,
        process_id,
        workspace_id,
        resource_class.as_str(),
        index,
        short_uuid
    )
}

impl WorkerEnvConfig {
    fn from_env() -> Result<Self, String> {
        Ok(Self {
            enabled: env_flag("BEEHIVE_WORKERS_ENABLED"),
            workspace_scope: parse_workspace_scope(std::env::var("BEEHIVE_WORKER_WORKSPACES").ok()),
            default_concurrency: parse_optional_u32("BEEHIVE_WORKER_DEFAULT_CONCURRENCY")?,
            local_llm_concurrency: parse_optional_u32("BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY")?,
            idle_sleep_ms: parse_optional_u64("BEEHIVE_WORKER_IDLE_SLEEP_MS")?
                .unwrap_or(DEFAULT_IDLE_SLEEP_MS),
            recovery_interval_sec: parse_optional_u64("BEEHIVE_WORKER_RECOVERY_INTERVAL_SEC")?
                .unwrap_or(DEFAULT_RECOVERY_INTERVAL_SEC),
            retention_interval_sec: parse_optional_u64("BEEHIVE_RUNTIME_RETENTION_INTERVAL_SEC")?
                .unwrap_or(DEFAULT_RETENTION_INTERVAL_SEC),
        })
    }
}

fn parse_workspace_scope(value: Option<String>) -> Option<Vec<String>> {
    value
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
}

fn parse_optional_u32(key: &str) -> Result<Option<u32>, String> {
    std::env::var(key)
        .ok()
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|error| format!("{key} must be a valid u32: {error}"))
        })
        .transpose()
}

fn parse_optional_u64(key: &str) -> Result<Option<u64>, String> {
    std::env::var(key)
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|error| format!("{key} must be a valid u64: {error}"))
        })
        .transpose()
}

fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn log_worker_manager(code: &str, message: &str) {
    eprintln!(r#"{{"event":"worker_manager","code":"{code}","message":"{message}"}}"#);
}

fn log_worker_event(code: &str, worker_id: Option<&str>, payload: Option<serde_json::Value>) {
    let mut event = json!({
        "event": "worker_lifecycle",
        "code": code,
    });
    if let Some(worker_id) = worker_id {
        event["worker_id"] = json!(worker_id);
    }
    if let Some(payload) = payload {
        event["payload"] = payload;
    }
    eprintln!("{event}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::discovery::scan_workspace;
    use crate::services::{runtime, workspaces};
    use std::ffi::OsStr;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct MockWebhook {
        url: String,
        request_count: Arc<AtomicUsize>,
    }

    fn with_test_env<F>(registry_path: &Path, root: &Path, run: F)
    where
        F: FnOnce(),
    {
        let _guard = workspaces::env_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_registry = std::env::var_os("BEEHIVE_WORKSPACES_CONFIG");
        let previous_root = std::env::var_os("BEEHIVE_WORKSPACES_ROOT");
        std::env::set_var("BEEHIVE_WORKSPACES_CONFIG", registry_path);
        std::env::set_var("BEEHIVE_WORKSPACES_ROOT", root);
        run();
        restore_env_var("BEEHIVE_WORKSPACES_CONFIG", previous_registry.as_deref());
        restore_env_var("BEEHIVE_WORKSPACES_ROOT", previous_root.as_deref());
    }

    fn restore_env_var(name: &str, value: Option<&OsStr>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn mock_webhook() -> MockWebhook {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock webhook");
        let address = listener.local_addr().expect("mock address");
        let request_count = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&request_count);
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0_u8; 4096];
                let _ = stream.read(&mut buffer);
                count.fetch_add(1, Ordering::SeqCst);
                let body = r#"{"success":true}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        MockWebhook {
            url: format!("http://{address}/webhook"),
            request_count,
        }
    }

    fn write_registry(registry_path: &Path, workdir: &Path) {
        fs::write(
            registry_path,
            format!(
                r#"
workspaces:
  - id: smoke
    name: Smoke
    provider: s3
    bucket: steos-s3-data
    workspace_prefix: smoke
    region: ru-1
    endpoint: https://s3.example
    workdir_path: {}
    pipeline_path: {}
    database_path: {}
"#,
                workdir.display(),
                workdir.join("pipeline.yaml").display(),
                workdir.join("app.db").display()
            ),
        )
        .expect("registry");
    }

    fn write_pipeline(workdir: &Path, workflow_url: &str) {
        write_pipeline_with_concurrency(workdir, workflow_url, 1, 0);
    }

    fn write_pipeline_with_concurrency(
        workdir: &Path,
        workflow_url: &str,
        default_concurrency: u32,
        local_llm_concurrency: u32,
    ) {
        fs::create_dir_all(workdir).expect("workdir");
        fs::write(
            workdir.join("pipeline.yaml"),
            format!(
                r#"
project:
  name: smoke
  workdir: .
runtime:
  request_timeout_sec: 5
  file_stability_delay_ms: 0
  worker_pools:
    default:
      concurrency: {default_concurrency}
    local_llm:
      concurrency: {local_llm_concurrency}
stages:
  - id: stage_0
    input_folder: stages/stage_0
    workflow_url: {workflow_url}
"#
            ),
        )
        .expect("pipeline");
    }

    fn with_default_worker_env<F>(value: Option<&str>, run: F)
    where
        F: FnOnce(),
    {
        let previous = std::env::var_os("BEEHIVE_WORKER_DEFAULT_CONCURRENCY");
        match value {
            Some(value) => std::env::set_var("BEEHIVE_WORKER_DEFAULT_CONCURRENCY", value),
            None => std::env::remove_var("BEEHIVE_WORKER_DEFAULT_CONCURRENCY"),
        }
        run();
        restore_env_var("BEEHIVE_WORKER_DEFAULT_CONCURRENCY", previous.as_deref());
    }

    #[test]
    fn workers_disabled_by_default_and_require_scope() {
        assert!(!workers_enabled_for_scope(false, None, "workspace-a"));
        assert!(!workers_enabled_for_scope(true, None, "workspace-a"));
        assert!(!workers_enabled_for_scope(true, Some(&[]), "workspace-a"));
    }

    #[test]
    fn workers_scope_matches_explicit_workspace_or_all() {
        let explicit = vec!["workspace-a".to_string(), "workspace-b".to_string()];
        assert!(workers_enabled_for_scope(
            true,
            Some(&explicit),
            "workspace-a"
        ));
        assert!(!workers_enabled_for_scope(
            true,
            Some(&explicit),
            "workspace-c"
        ));

        let all = vec!["all".to_string()];
        assert!(workers_enabled_for_scope(true, Some(&all), "workspace-c"));
    }

    #[test]
    fn worker_loop_once_reaches_claim_and_calls_webhook_without_bootstrap_loop() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path().join("root");
        let workdir = root.join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        let webhook = mock_webhook();
        write_pipeline(&workdir, &webhook.url);
        write_registry(&registry_path, &workdir);

        with_test_env(&registry_path, &root, || {
            let context = runtime::load_workspace_context("smoke").expect("heavy bootstrap once");
            let source_path = workdir.join("stages/stage_0/entity-1.json");
            fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
            fs::write(
                &source_path,
                r#"{"id":"entity-1","payload":{"title":"hello"},"meta":{}}"#,
            )
            .expect("source file");
            scan_workspace(&workdir, &context.database_path).expect("scan");
            database::set_all_worker_pools_started(&context.database_path, 1, 0)
                .expect("start pool");

            let mut last_recovery = Instant::now();
            let summary = run_worker_loop_once(
                "smoke",
                ResourceClass::Default,
                "test-worker",
                &mut last_recovery,
                Duration::from_secs(DEFAULT_RECOVERY_INTERVAL_SEC),
                false,
            )
            .expect("worker iteration");

            assert_eq!(summary.claimed, 1);
            assert_eq!(summary.succeeded, 1);
            assert_eq!(webhook.request_count.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn worker_start_summary_explains_env_and_yaml_cap() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path().join("root");
        let workdir = root.join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        write_pipeline_with_concurrency(&workdir, "http://127.0.0.1:9999/webhook", 1, 0);
        write_registry(&registry_path, &workdir);

        with_test_env(&registry_path, &root, || {
            with_default_worker_env(Some("10"), || {
                runtime::load_workspace_context("smoke").expect("bootstrap");
                let summary = start_workers(
                    "smoke",
                    &WorkerStartRequest {
                        default_workers: 3,
                        local_llm_workers: 0,
                    },
                )
                .expect("start");
                let default_pool = summary
                    .pools
                    .iter()
                    .find(|pool| pool.resource_class == ResourceClass::Default)
                    .expect("default pool");

                assert_eq!(default_pool.requested_desired_concurrency, Some(3));
                assert_eq!(default_pool.desired_concurrency, 1);
                assert_eq!(default_pool.configured_concurrency, 1);
                assert_eq!(default_pool.env_concurrency_limit, Some(10));
                assert_eq!(default_pool.effective_concurrency, 1);
            });
        });
    }

    #[test]
    fn worker_start_summary_uses_requested_when_env_and_yaml_allow_it() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path().join("root");
        let workdir = root.join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        write_pipeline_with_concurrency(&workdir, "http://127.0.0.1:9999/webhook", 10, 0);
        write_registry(&registry_path, &workdir);

        with_test_env(&registry_path, &root, || {
            with_default_worker_env(Some("10"), || {
                runtime::load_workspace_context("smoke").expect("bootstrap");
                let summary = start_workers(
                    "smoke",
                    &WorkerStartRequest {
                        default_workers: 3,
                        local_llm_workers: 0,
                    },
                )
                .expect("start");
                let default_pool = summary
                    .pools
                    .iter()
                    .find(|pool| pool.resource_class == ResourceClass::Default)
                    .expect("default pool");

                assert_eq!(default_pool.requested_desired_concurrency, Some(3));
                assert_eq!(default_pool.desired_concurrency, 3);
                assert_eq!(default_pool.configured_concurrency, 10);
                assert_eq!(default_pool.env_concurrency_limit, Some(10));
                assert_eq!(default_pool.effective_concurrency, 3);
            });
        });
    }

    #[test]
    fn worker_start_summary_caps_to_yaml_when_env_absent() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let root = tempdir.path().join("root");
        let workdir = root.join("smoke");
        let registry_path = tempdir.path().join("workspaces.yaml");
        write_pipeline_with_concurrency(&workdir, "http://127.0.0.1:9999/webhook", 5, 0);
        write_registry(&registry_path, &workdir);

        with_test_env(&registry_path, &root, || {
            with_default_worker_env(None, || {
                runtime::load_workspace_context("smoke").expect("bootstrap");
                let summary = start_workers(
                    "smoke",
                    &WorkerStartRequest {
                        default_workers: 10,
                        local_llm_workers: 0,
                    },
                )
                .expect("start");
                let default_pool = summary
                    .pools
                    .iter()
                    .find(|pool| pool.resource_class == ResourceClass::Default)
                    .expect("default pool");

                assert_eq!(default_pool.requested_desired_concurrency, Some(10));
                assert_eq!(default_pool.desired_concurrency, 5);
                assert_eq!(default_pool.configured_concurrency, 5);
                assert_eq!(default_pool.env_concurrency_limit, None);
                assert_eq!(default_pool.effective_concurrency, 5);
            });
        });
    }
}

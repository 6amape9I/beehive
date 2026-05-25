use std::thread;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::database;
use crate::domain::{ResourceClass, RunDueTasksSummary, WorkerSummary};
use crate::executor;
use crate::services::{runtime, workspaces};

const DEFAULT_IDLE_SLEEP_MS: u64 = 1000;
const DEFAULT_RECOVERY_INTERVAL_SEC: u64 = 30;
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
}

pub(crate) fn worker_summary(workspace_id: &str) -> Result<WorkerSummary, String> {
    let context = runtime::load_workspace_context(workspace_id)?;
    let mut summary = database::get_worker_summary(&context.database_path, &context.config)?;
    summary.workers_enabled = workspace_workers_enabled(workspace_id);
    summary.broad_runs_disabled = summary.workers_enabled;
    Ok(summary)
}

pub(crate) fn recover_expired_leases(workspace_id: &str) -> Result<u64, String> {
    let context = runtime::load_workspace_context(workspace_id)?;
    database::recover_expired_worker_leases(&context.database_path)
}

pub(crate) fn pause_all(workspace_id: &str, reason: Option<&str>) -> Result<WorkerSummary, String> {
    let context = runtime::load_workspace_context(workspace_id)?;
    database::set_all_worker_pools_paused(&context.database_path, true, reason)?;
    worker_summary(workspace_id)
}

pub(crate) fn resume_all(workspace_id: &str) -> Result<WorkerSummary, String> {
    let context = runtime::load_workspace_context(workspace_id)?;
    database::set_all_worker_pools_paused(&context.database_path, false, None)?;
    worker_summary(workspace_id)
}

pub(crate) fn pause_pool(
    workspace_id: &str,
    resource_class: ResourceClass,
    reason: Option<&str>,
) -> Result<WorkerSummary, String> {
    let context = runtime::load_workspace_context(workspace_id)?;
    database::set_worker_pool_paused(&context.database_path, resource_class, true, reason)?;
    worker_summary(workspace_id)
}

pub(crate) fn resume_pool(
    workspace_id: &str,
    resource_class: ResourceClass,
) -> Result<WorkerSummary, String> {
    let context = runtime::load_workspace_context(workspace_id)?;
    database::set_worker_pool_paused(&context.database_path, resource_class, false, None)?;
    worker_summary(workspace_id)
}

pub(crate) fn release_lease(
    workspace_id: &str,
    lease_id: &str,
    reason: &str,
) -> Result<bool, String> {
    let context = runtime::load_workspace_context(workspace_id)?;
    database::release_worker_lease(&context.database_path, lease_id, reason)
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

    for index in 0..default_concurrency {
        spawn_worker_loop(&workspace_id, ResourceClass::Default, index, config.clone())?;
    }
    for index in 0..local_llm_concurrency {
        spawn_worker_loop(
            &workspace_id,
            ResourceClass::LocalLlm,
            index,
            config.clone(),
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
) -> Result<(), String> {
    let workspace_id = workspace_id.to_string();
    let worker_id = worker_id(&workspace_id, resource_class, index);
    thread::Builder::new()
        .name(worker_id.clone())
        .spawn(move || worker_loop(workspace_id, resource_class, worker_id, config))
        .map(|_| ())
        .map_err(|error| format!("Failed to spawn worker thread: {error}"))
}

fn worker_loop(
    workspace_id: String,
    resource_class: ResourceClass,
    worker_id: String,
    config: WorkerEnvConfig,
) {
    let idle_sleep = Duration::from_millis(config.idle_sleep_ms.max(100));
    let recovery_interval = Duration::from_secs(config.recovery_interval_sec.max(1));
    let mut last_recovery = Instant::now() - recovery_interval;

    loop {
        let context = match runtime::load_workspace_context(&workspace_id) {
            Ok(context) => context,
            Err(message) => {
                log_worker_error(&worker_id, &message);
                thread::sleep(idle_sleep);
                continue;
            }
        };
        if last_recovery.elapsed() >= recovery_interval {
            if let Err(message) = database::recover_expired_worker_leases(&context.database_path) {
                log_worker_error(&worker_id, &message);
            }
            last_recovery = Instant::now();
        }
        match database::worker_pool_is_paused(&context.database_path, resource_class) {
            Ok(true) => {
                thread::sleep(idle_sleep);
                continue;
            }
            Ok(false) => {}
            Err(message) => {
                log_worker_error(&worker_id, &message);
                thread::sleep(idle_sleep);
                continue;
            }
        }

        let summary = executor::run_worker_task(
            &context.workdir_path,
            &context.database_path,
            resource_class,
            &worker_id,
            context.config.runtime.request_timeout_sec,
            context.config.runtime.worker_lease_sec,
            context.config.runtime.worker_heartbeat_sec,
            context.config.runtime.file_stability_delay_ms,
        );
        match summary {
            Ok(summary) if worker_was_idle(&summary) => thread::sleep(idle_sleep),
            Ok(_) => {}
            Err(message) => {
                log_worker_error(&worker_id, &message);
                thread::sleep(idle_sleep);
            }
        }
    }
}

fn worker_was_idle(summary: &RunDueTasksSummary) -> bool {
    summary.claimed == 0 && summary.errors.is_empty()
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

fn log_worker_error(worker_id: &str, message: &str) {
    eprintln!(r#"{{"event":"worker_error","worker_id":"{worker_id}","message":"{message}"}}"#);
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

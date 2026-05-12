use std::collections::HashMap;

use chrono::Utc;
use rusqlite::Connection;
use serde_json::json;

use crate::domain::{
    AppEventLevel, EntityFileAllowedActions, EntityFileRecord, EntityStageAllowedActions,
    EntityStageStateRecord, StorageProvider,
};

pub(crate) fn build_stage_allowed_actions(
    stage_states: &[EntityStageStateRecord],
) -> Vec<EntityStageAllowedActions> {
    stage_states
        .iter()
        .map(|state| {
            let mut reasons = Vec::new();
            let status = state.status.as_str();
            let active = !matches!(status, "queued" | "in_progress" | "done");
            if !active {
                reasons.push(format!("Status '{}' is not manually actionable.", status));
            }
            let can_retry_now = matches!(status, "pending" | "retry_wait");
            let can_reset_to_pending =
                matches!(status, "failed" | "blocked" | "skipped" | "retry_wait");
            let can_skip = matches!(status, "pending" | "retry_wait");
            let can_run_this_stage = can_retry_now;

            if can_retry_now && status == "retry_wait" {
                reasons.push(
                    "Manual retry may bypass next_retry_at for operator debugging.".to_string(),
                );
            }
            if can_reset_to_pending {
                reasons.push(
                    "Reset keeps stage_runs history and clears retry/error fields.".to_string(),
                );
            }
            if can_skip {
                reasons.push(
                    "Skip does not create a copy, advance the entity, or call n8n.".to_string(),
                );
            }

            EntityStageAllowedActions {
                stage_id: state.stage_id.clone(),
                can_retry_now,
                can_reset_to_pending,
                can_skip,
                can_run_this_stage,
                reasons,
            }
        })
        .collect()
}

pub(crate) fn build_file_allowed_actions(
    files: &[EntityFileRecord],
    stage_states: &[EntityStageStateRecord],
) -> Vec<EntityFileAllowedActions> {
    let states_by_entity_stage = stage_states
        .iter()
        .map(|state| {
            (
                (state.entity_id.clone(), state.stage_id.clone()),
                state.status.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();

    files
        .iter()
        .map(|file| {
            let status = states_by_entity_stage
                .get(&(file.entity_id.clone(), file.stage_id.clone()))
                .copied();
            build_file_policy(file, status)
        })
        .collect()
}

pub(crate) fn evaluate_entity_file_allowed_actions(
    connection: &Connection,
    file: &EntityFileRecord,
) -> Result<EntityFileAllowedActions, String> {
    let status = super::find_stage_state_identity(connection, &file.entity_id, &file.stage_id)?
        .map(|state| state.status);
    Ok(build_file_policy(file, status.as_deref()))
}

pub(crate) fn record_entity_file_json_edit_rejected(
    connection: &Connection,
    file: &EntityFileRecord,
    policy: &EntityFileAllowedActions,
    operator_comment: Option<&str>,
) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    super::insert_app_event(
        connection,
        AppEventLevel::Warning,
        "entity_file_json_edit_rejected",
        &format!(
            "Business JSON edit was rejected for entity file '{}'.",
            file.file_path
        ),
        Some(json!({
            "entity_id": &file.entity_id,
            "stage_id": &file.stage_id,
            "entity_file_id": file.id,
            "file_path": &file.file_path,
            "operator_comment": operator_comment,
            "reasons": &policy.reasons,
        })),
        &now,
    )
}

fn build_file_policy(
    file: &EntityFileRecord,
    runtime_status: Option<&str>,
) -> EntityFileAllowedActions {
    let mut reasons = Vec::new();
    let mut can_edit_business_json = true;

    if !file.file_exists {
        can_edit_business_json = false;
        reasons.push("File is marked missing; business JSON cannot be edited.".to_string());
    }
    if file.storage_provider == StorageProvider::S3 {
        can_edit_business_json = false;
        reasons.push(
            "S3 artifact pointers are read-only in B1; Beehive does not edit S3 business JSON."
                .to_string(),
        );
    }

    match runtime_status {
        Some("pending" | "retry_wait" | "failed" | "blocked" | "skipped") => {
            if can_edit_business_json {
                reasons.push(format!(
                    "Business JSON can be edited while runtime status is '{}'.",
                    runtime_status.unwrap_or_default()
                ));
            }
        }
        Some("queued") => {
            can_edit_business_json = false;
            reasons.push(
                "Runtime state 'queued' is active; business JSON editing is disabled.".to_string(),
            );
        }
        Some("in_progress") => {
            can_edit_business_json = false;
            reasons.push(
                "Runtime state 'in_progress' is active; business JSON editing is disabled."
                    .to_string(),
            );
        }
        Some("done") => {
            can_edit_business_json = false;
            reasons.push(
                "Runtime state 'done' is complete; completed artifacts should not be edited silently."
                    .to_string(),
            );
        }
        Some(status) => {
            can_edit_business_json = false;
            reasons.push(format!(
                "Runtime state '{}' is not approved for business JSON editing.",
                status
            ));
        }
        None => {
            can_edit_business_json = false;
            reasons.push(
                "No runtime stage state exists for this file. Run Scan workspace before editing."
                    .to_string(),
            );
        }
    }

    EntityFileAllowedActions {
        entity_file_id: file.id,
        can_edit_business_json,
        can_open_file: file.file_exists && file.storage_provider == StorageProvider::Local,
        can_open_folder: !file.file_path.trim().is_empty()
            && file.storage_provider == StorageProvider::Local,
        reasons,
    }
}

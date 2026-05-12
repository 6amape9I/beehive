use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::database::{
    ensure_entity_stub, evaluate_entity_file_allowed_actions, find_entity_file_by_id,
    find_latest_entity_file_for_stage, find_stage_by_id, get_entity_detail_with_selection,
    insert_app_event, load_active_stages_from_connection, open_connection,
    recompute_entity_summaries, record_entity_file_json_edit_rejected, system_time_to_rfc3339,
    upsert_entity_file, upsert_entity_stage_state, PersistEntityFileInput,
    PersistEntityStageStateInput,
};
use crate::discovery::ensure_stage_directories_for_stage_ids;
use crate::domain::{
    AppEventLevel, EntityDetailPayload, EntityValidationStatus, FileCopyPayload, FileCopyStatus,
    StageStatus,
};
use crate::file_safety::read_stable_file;
use crate::save_path::{resolve_save_path_route, route_for_stage_input_folder, SavePathRoute};
use crate::state_machine::parse_status;
use crate::workdir::path_string;

pub fn create_next_stage_copy(
    workdir_path: &Path,
    database_path: &Path,
    entity_id: &str,
    source_stage_id: &str,
) -> Result<FileCopyPayload, String> {
    let connection = open_connection(database_path)?;
    let source_stage = find_stage_by_id(&connection, source_stage_id)?
        .ok_or_else(|| format!("Source stage '{source_stage_id}' was not found."))?;
    let source_file = find_latest_entity_file_for_stage(&connection, entity_id, source_stage_id)?
        .ok_or_else(|| {
        format!(
            "No file instance was found for entity '{}' on source stage '{}'.",
            entity_id, source_stage_id
        )
    })?;

    if !source_file.file_exists {
        return Err(format!(
            "Source file '{}' is currently marked missing and cannot be copied.",
            source_file.file_path
        ));
    }

    let target_stage_id = source_file
        .next_stage
        .clone()
        .or_else(|| source_stage.next_stage.clone());

    let now = Utc::now().to_rfc3339();
    let Some(target_stage_id) = target_stage_id else {
        insert_app_event(
            &connection,
            AppEventLevel::Warning,
            "managed_copy_failed",
            &format!(
                "No next stage is configured for entity '{}' from source stage '{}'.",
                entity_id, source_stage_id
            ),
            Some(json!({
                "entity_id": entity_id,
                "source_stage_id": source_stage_id,
            })),
            &now,
        )?;
        return Ok(FileCopyPayload {
            status: FileCopyStatus::Blocked,
            entity_id: entity_id.to_string(),
            source_stage_id: source_stage_id.to_string(),
            target_stage_id: None,
            source_file_path: Some(source_file.file_path),
            target_file_path: None,
            target_file: None,
            message: "No next stage is configured for this source stage.".to_string(),
        });
    };

    let Some(target_stage) = find_stage_by_id(&connection, &target_stage_id)? else {
        insert_app_event(
            &connection,
            AppEventLevel::Warning,
            "managed_copy_failed",
            &format!(
                "Target stage '{}' does not exist for entity '{}'.",
                target_stage_id, entity_id
            ),
            Some(json!({
                "entity_id": entity_id,
                "source_stage_id": source_stage_id,
                "target_stage_id": target_stage_id,
            })),
            &now,
        )?;
        return Ok(FileCopyPayload {
            status: FileCopyStatus::Blocked,
            entity_id: entity_id.to_string(),
            source_stage_id: source_stage_id.to_string(),
            target_stage_id: Some(target_stage_id),
            source_file_path: Some(source_file.file_path),
            target_file_path: None,
            target_file: None,
            message: "The resolved next stage does not exist.".to_string(),
        });
    };

    if !target_stage.is_active {
        insert_app_event(
            &connection,
            AppEventLevel::Warning,
            "managed_copy_failed",
            &format!(
                "Target stage '{}' is inactive for entity '{}'.",
                target_stage.id, entity_id
            ),
            Some(json!({
                "entity_id": entity_id,
                "source_stage_id": source_stage_id,
                "target_stage_id": target_stage.id,
            })),
            &now,
        )?;
        return Ok(FileCopyPayload {
            status: FileCopyStatus::Blocked,
            entity_id: entity_id.to_string(),
            source_stage_id: source_stage_id.to_string(),
            target_stage_id: Some(target_stage.id),
            source_file_path: Some(source_file.file_path),
            target_file_path: None,
            target_file: None,
            message: "The resolved next stage is inactive.".to_string(),
        });
    }

    ensure_stage_directories_for_stage_ids(
        workdir_path,
        database_path,
        &[target_stage.id.clone()],
    )?;

    let source_path = PathBuf::from(&source_file.file_path);
    let source_bytes = fs::read(&source_path).map_err(|error| {
        format!(
            "Failed to read source file '{}': {error}",
            source_path.display()
        )
    })?;
    let source_json = serde_json::from_slice::<Value>(&source_bytes).map_err(|error| {
        format!(
            "Failed to parse source file '{}' as JSON before copy: {error}",
            source_path.display()
        )
    })?;
    let Some(root) = source_json.as_object() else {
        return Err(format!(
            "Source file '{}' must contain a JSON object before managed copy.",
            source_path.display()
        ));
    };

    let updated_json = build_target_json(
        root,
        source_stage_id,
        &target_stage.id,
        target_stage.next_stage.as_deref(),
        &now,
    )?;
    let target_path = workdir_path
        .join(&target_stage.input_folder)
        .join(&source_file.file_name);
    let target_path_string = path_string(&target_path);
    let target_bytes = serde_json::to_vec_pretty(&updated_json)
        .map_err(|error| format!("Failed to serialize target JSON for managed copy: {error}"))?;
    let target_checksum = format!("{:x}", Sha256::digest(&target_bytes));

    if target_path.exists() {
        let existing_bytes = fs::read(&target_path).map_err(|error| {
            format!(
                "Failed to read existing target file '{}' during collision check: {error}",
                target_path.display()
            )
        })?;
        let existing_json = serde_json::from_slice::<Value>(&existing_bytes).map_err(|error| {
            format!(
                "Failed to parse existing target file '{}' during collision check: {error}",
                target_path.display()
            )
        })?;
        let existing_entity_id = existing_json
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let existing_checksum = format!("{:x}", Sha256::digest(&existing_bytes));

        if existing_entity_id != entity_id {
            insert_app_event(
                &connection,
                AppEventLevel::Error,
                "unsafe_file_operation_rejected",
                &format!(
                    "Managed copy target '{}' already belongs to entity '{}'.",
                    target_path.display(),
                    existing_entity_id
                ),
                Some(json!({
                    "entity_id": entity_id,
                    "source_stage_id": source_stage_id,
                    "target_stage_id": target_stage.id,
                    "target_file_path": target_path_string,
                })),
                &now,
            )?;
            return Ok(FileCopyPayload {
                status: FileCopyStatus::Failed,
                entity_id: entity_id.to_string(),
                source_stage_id: source_stage_id.to_string(),
                target_stage_id: Some(target_stage.id),
                source_file_path: Some(source_file.file_path),
                target_file_path: Some(path_string(&target_path)),
                target_file: None,
                message: "Target path already exists for another entity.".to_string(),
            });
        }

        let is_compatible_existing_target = existing_checksum == target_checksum
            || existing_json.as_object().is_some_and(|root| {
                root.get("id").and_then(Value::as_str) == Some(entity_id)
                    && root.get("current_stage").and_then(Value::as_str)
                        == Some(target_stage.id.as_str())
                    && root.get("status").and_then(Value::as_str) == Some("pending")
                    && root.get("next_stage").and_then(Value::as_str)
                        == target_stage.next_stage.as_deref()
                    && root
                        .get("meta")
                        .and_then(Value::as_object)
                        .and_then(|meta| meta.get("beehive"))
                        .and_then(Value::as_object)
                        .and_then(|beehive| beehive.get("copy_source_stage"))
                        .and_then(Value::as_str)
                        == Some(source_stage_id)
                    && root
                        .get("meta")
                        .and_then(Value::as_object)
                        .and_then(|meta| meta.get("beehive"))
                        .and_then(Value::as_object)
                        .and_then(|beehive| beehive.get("copy_target_stage"))
                        .and_then(Value::as_str)
                        == Some(target_stage.id.as_str())
            });

        if !is_compatible_existing_target {
            insert_app_event(
                &connection,
                AppEventLevel::Error,
                "managed_copy_failed",
                &format!(
                    "Managed copy target '{}' already exists with different content.",
                    target_path.display()
                ),
                Some(json!({
                    "entity_id": entity_id,
                    "source_stage_id": source_stage_id,
                    "target_stage_id": target_stage.id,
                    "target_file_path": target_path_string,
                })),
                &now,
            )?;
            return Ok(FileCopyPayload {
                status: FileCopyStatus::Failed,
                entity_id: entity_id.to_string(),
                source_stage_id: source_stage_id.to_string(),
                target_stage_id: Some(target_stage.id),
                source_file_path: Some(source_file.file_path),
                target_file_path: Some(path_string(&target_path)),
                target_file: None,
                message: "Target path already exists with different content.".to_string(),
            });
        }

        let target_file = register_target_file(
            database_path,
            &target_stage.id,
            &target_stage.next_stage,
            entity_id,
            &target_path,
            &source_file.file_name,
            &existing_checksum,
            &existing_bytes,
            Some(source_file.id),
            &now,
        )?;
        insert_app_event(
            &connection,
            AppEventLevel::Info,
            "managed_copy_skipped",
            &format!(
                "Managed copy target '{}' already exists with compatible content.",
                target_path.display()
            ),
            Some(json!({
                "entity_id": entity_id,
                "source_stage_id": source_stage_id,
                "target_stage_id": target_stage.id,
                "target_file_path": path_string(&target_path),
            })),
            &now,
        )?;

        return Ok(FileCopyPayload {
            status: FileCopyStatus::AlreadyExists,
            entity_id: entity_id.to_string(),
            source_stage_id: source_stage_id.to_string(),
            target_stage_id: Some(target_stage.id),
            source_file_path: Some(source_file.file_path),
            target_file_path: Some(path_string(&target_path)),
            target_file: Some(target_file),
            message: "Target file already exists with compatible content.".to_string(),
        });
    }

    write_atomic_json(&target_path, &target_bytes)?;
    let target_file = register_target_file(
        database_path,
        &target_stage.id,
        &target_stage.next_stage,
        entity_id,
        &target_path,
        &source_file.file_name,
        &target_checksum,
        &target_bytes,
        Some(source_file.id),
        &now,
    )?;

    let connection = open_connection(database_path)?;
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "managed_copy_created",
        &format!(
            "Managed copy created for entity '{}' into stage '{}'.",
            entity_id, target_stage.id
        ),
        Some(json!({
            "entity_id": entity_id,
            "source_stage_id": source_stage_id,
            "target_stage_id": target_stage.id,
            "source_file_path": source_file.file_path,
            "target_file_path": path_string(&target_path),
        })),
        &now,
    )?;

    Ok(FileCopyPayload {
        status: FileCopyStatus::Created,
        entity_id: entity_id.to_string(),
        source_stage_id: source_stage_id.to_string(),
        target_stage_id: Some(target_stage.id),
        source_file_path: Some(source_file.file_path),
        target_file_path: Some(path_string(&target_path)),
        target_file: Some(target_file),
        message: "Managed copy created successfully.".to_string(),
    })
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResponseCopyPayload {
    pub status: FileCopyStatus,
    pub source_entity_id: String,
    pub source_stage_id: String,
    pub target_stage_id: Option<String>,
    pub source_file_path: Option<String>,
    pub target_file_paths: Vec<String>,
    pub target_files: Vec<crate::domain::EntityFileRecord>,
    pub output_count: usize,
    pub message: String,
}

struct PlannedResponseTarget {
    target_stage_id: String,
    target_stage_next_stage: Option<String>,
    child_entity_id: String,
    file_name: String,
    target_path: PathBuf,
    bytes: Vec<u8>,
    checksum: String,
    existing_bytes: Option<Vec<u8>>,
    existing_checksum: Option<String>,
}

pub fn create_next_stage_copies_from_response(
    workdir_path: &Path,
    database_path: &Path,
    source_entity_id: &str,
    source_stage_id: &str,
    response_payloads: &[Value],
    response_meta: Option<Value>,
    stage_run_id: &str,
) -> Result<ResponseCopyPayload, String> {
    let connection = open_connection(database_path)?;
    let source_stage = find_stage_by_id(&connection, source_stage_id)?
        .ok_or_else(|| format!("Source stage '{source_stage_id}' was not found."))?;
    let source_file =
        find_latest_entity_file_for_stage(&connection, source_entity_id, source_stage_id)?
            .ok_or_else(|| {
                format!(
                    "No file instance was found for entity '{}' on source stage '{}'.",
                    source_entity_id, source_stage_id
                )
            })?;
    let active_stages = load_active_stages_from_connection(&connection)?;
    let fallback_target_stage_id = source_file
        .next_stage
        .clone()
        .or_else(|| source_stage.next_stage.clone());

    if response_payloads.is_empty() {
        return Ok(ResponseCopyPayload {
            status: FileCopyStatus::Failed,
            source_entity_id: source_entity_id.to_string(),
            source_stage_id: source_stage_id.to_string(),
            target_stage_id: fallback_target_stage_id,
            source_file_path: Some(source_file.file_path.clone()),
            target_file_paths: Vec::new(),
            target_files: Vec::new(),
            output_count: 0,
            message: "n8n response did not contain any output payload objects.".to_string(),
        });
    }

    let source_path = PathBuf::from(&source_file.file_path);
    let source_bytes = fs::read(&source_path).map_err(|error| {
        format!(
            "Failed to read source file '{}': {error}",
            source_path.display()
        )
    })?;
    let source_json = serde_json::from_slice::<Value>(&source_bytes).map_err(|error| {
        format!(
            "Failed to parse source file '{}' as JSON before response copy: {error}",
            source_path.display()
        )
    })?;
    let source_root = source_json
        .as_object()
        .ok_or_else(|| "Source JSON root must be an object before response copy.".to_string())?;
    drop(connection);

    let now = Utc::now().to_rfc3339();
    let mut planned_paths = HashSet::new();
    let mut plans = Vec::new();
    let mut target_stage_ids = Vec::<String>::new();
    let output_count = response_payloads.len();
    for (index, payload) in response_payloads.iter().enumerate() {
        let Some(payload_object) = payload.as_object() else {
            return Ok(ResponseCopyPayload {
                status: FileCopyStatus::Failed,
                source_entity_id: source_entity_id.to_string(),
                source_stage_id: source_stage_id.to_string(),
                target_stage_id: None,
                source_file_path: Some(source_file.file_path.clone()),
                target_file_paths: Vec::new(),
                target_files: Vec::new(),
                output_count,
                message: format!("n8n output item at index {index} is not a JSON object."),
            });
        };

        let route = match payload_object.get("save_path") {
            Some(Value::String(save_path)) => {
                match resolve_save_path_route(save_path, workdir_path, &active_stages) {
                    Ok(route) => route,
                    Err(error) => {
                        return Ok(ResponseCopyPayload {
                            status: FileCopyStatus::Blocked,
                            source_entity_id: source_entity_id.to_string(),
                            source_stage_id: source_stage_id.to_string(),
                            target_stage_id: None,
                            source_file_path: Some(source_file.file_path.clone()),
                            target_file_paths: Vec::new(),
                            target_files: Vec::new(),
                            output_count,
                            message: error.message,
                        });
                    }
                }
            }
            Some(_) => {
                return Ok(ResponseCopyPayload {
                    status: FileCopyStatus::Blocked,
                    source_entity_id: source_entity_id.to_string(),
                    source_stage_id: source_stage_id.to_string(),
                    target_stage_id: None,
                    source_file_path: Some(source_file.file_path.clone()),
                    target_file_paths: Vec::new(),
                    target_files: Vec::new(),
                    output_count,
                    message: format!(
                        "n8n output item at index {index} has a non-string save_path."
                    ),
                });
            }
            None => {
                let Some(target_stage_id) = fallback_target_stage_id.as_deref() else {
                    return Ok(ResponseCopyPayload {
                        status: FileCopyStatus::Blocked,
                        source_entity_id: source_entity_id.to_string(),
                        source_stage_id: source_stage_id.to_string(),
                        target_stage_id: None,
                        source_file_path: Some(source_file.file_path.clone()),
                        target_file_paths: Vec::new(),
                        target_files: Vec::new(),
                        output_count,
                        message: format!(
                            "n8n output item at index {index} has no save_path and no next_stage is configured."
                        ),
                    });
                };
                let Some(target_stage) =
                    active_stages.iter().find(|stage| stage.id == target_stage_id)
                else {
                    return Ok(ResponseCopyPayload {
                        status: FileCopyStatus::Blocked,
                        source_entity_id: source_entity_id.to_string(),
                        source_stage_id: source_stage_id.to_string(),
                        target_stage_id: Some(target_stage_id.to_string()),
                        source_file_path: Some(source_file.file_path.clone()),
                        target_file_paths: Vec::new(),
                        target_files: Vec::new(),
                        output_count,
                        message: "The resolved next stage does not exist or is inactive."
                            .to_string(),
                    });
                };
                match route_for_stage_input_folder(workdir_path, target_stage) {
                    Ok(route) => route,
                    Err(error) => {
                        return Ok(ResponseCopyPayload {
                            status: FileCopyStatus::Blocked,
                            source_entity_id: source_entity_id.to_string(),
                            source_stage_id: source_stage_id.to_string(),
                            target_stage_id: Some(target_stage.id.clone()),
                            source_file_path: Some(source_file.file_path.clone()),
                            target_file_paths: Vec::new(),
                            target_files: Vec::new(),
                            output_count,
                            message: error.message,
                        });
                    }
                }
            }
        };

        if !target_stage_ids.iter().any(|stage_id| stage_id == &route.stage.id) {
            target_stage_ids.push(route.stage.id.clone());
        }
        let plan = match choose_response_target_plan(
            source_root,
            source_entity_id,
            source_stage_id,
            &route,
            payload,
            response_meta.clone(),
            source_file.id,
            stage_run_id,
            index,
            output_count,
            &now,
            &mut planned_paths,
        ) {
            Ok(plan) => plan,
            Err(message) => {
                return Ok(ResponseCopyPayload {
                    status: FileCopyStatus::Failed,
                    source_entity_id: source_entity_id.to_string(),
                    source_stage_id: source_stage_id.to_string(),
                    target_stage_id: Some(route.stage.id),
                    source_file_path: Some(source_file.file_path.clone()),
                    target_file_paths: Vec::new(),
                    target_files: Vec::new(),
                    output_count,
                    message,
                });
            }
        };
        plans.push(plan);
    }

    ensure_stage_directories_for_stage_ids(workdir_path, database_path, &target_stage_ids)?;
    for plan in &plans {
        let Some(parent) = plan.target_path.parent() else {
            return Err(format!(
                "Target path '{}' does not have a parent directory.",
                plan.target_path.display()
            ));
        };
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create target directory '{}': {error}",
                parent.display()
            )
        })?;
    }

    let mut target_files = Vec::new();
    let mut target_paths = Vec::new();
    let mut created_count = 0_u64;
    let mut already_exists_count = 0_u64;
    for plan in &plans {
        let (bytes, checksum) = if let (Some(existing_bytes), Some(existing_checksum)) =
            (&plan.existing_bytes, &plan.existing_checksum)
        {
            already_exists_count += 1;
            (existing_bytes.as_slice(), existing_checksum.as_str())
        } else {
            write_atomic_json(&plan.target_path, &plan.bytes)?;
            created_count += 1;
            (plan.bytes.as_slice(), plan.checksum.as_str())
        };
        let target_file = register_target_file(
            database_path,
            &plan.target_stage_id,
            &plan.target_stage_next_stage,
            &plan.child_entity_id,
            &plan.target_path,
            &plan.file_name,
            checksum,
            bytes,
            Some(source_file.id),
            &now,
        )?;
        target_paths.push(path_string(&plan.target_path));
        target_files.push(target_file);
    }

    let connection = open_connection(database_path)?;
    let response_target_stage_id = if target_stage_ids.len() == 1 {
        Some(target_stage_ids[0].clone())
    } else {
        None
    };
    insert_app_event(
        &connection,
        AppEventLevel::Info,
        "managed_copy_created",
        &format!(
            "Registered {} n8n output artifact(s) from entity '{}' into {} stage(s).",
            output_count,
            source_entity_id,
            target_stage_ids.len()
        ),
        Some(json!({
            "source_entity_id": source_entity_id,
            "source_stage_id": source_stage_id,
            "source_file_id": source_file.id,
            "source_file_path": &source_file.file_path,
            "target_stage_id": response_target_stage_id.clone(),
            "target_stage_ids": &target_stage_ids,
            "stage_run_id": stage_run_id,
            "output_count": output_count,
            "created_count": created_count,
            "already_exists_count": already_exists_count,
            "target_file_paths": &target_paths,
        })),
        &now,
    )?;

    Ok(ResponseCopyPayload {
        status: if created_count == 0 {
            FileCopyStatus::AlreadyExists
        } else {
            FileCopyStatus::Created
        },
        source_entity_id: source_entity_id.to_string(),
        source_stage_id: source_stage_id.to_string(),
        target_stage_id: response_target_stage_id,
        source_file_path: Some(source_file.file_path),
        target_file_paths: target_paths,
        target_files,
        output_count,
        message: format!("Registered {output_count} n8n output artifact(s)."),
    })
}

#[allow(dead_code)]
pub fn create_next_stage_copy_from_response(
    workdir_path: &Path,
    database_path: &Path,
    entity_id: &str,
    source_stage_id: &str,
    response_payload: Value,
    response_meta: Option<Value>,
    stage_run_id: &str,
) -> Result<FileCopyPayload, String> {
    let copy = create_next_stage_copies_from_response(
        workdir_path,
        database_path,
        entity_id,
        source_stage_id,
        &[response_payload],
        response_meta,
        stage_run_id,
    )?;
    Ok(FileCopyPayload {
        status: copy.status,
        entity_id: copy.source_entity_id,
        source_stage_id: copy.source_stage_id,
        target_stage_id: copy.target_stage_id,
        source_file_path: copy.source_file_path,
        target_file_path: copy.target_file_paths.first().cloned(),
        target_file: copy.target_files.first().cloned(),
        message: copy.message,
    })
}

pub fn save_entity_file_business_json(
    workdir_path: &Path,
    database_path: &Path,
    entity_file_id: i64,
    payload_json: &str,
    meta_json: &str,
    operator_comment: Option<&str>,
    file_stability_delay_ms: u64,
) -> Result<EntityDetailPayload, String> {
    let connection = open_connection(database_path)?;
    let file = find_entity_file_by_id(&connection, entity_file_id)?
        .ok_or_else(|| format!("Entity file id '{entity_file_id}' was not found."))?;
    let edit_policy = evaluate_entity_file_allowed_actions(&connection, &file)?;
    if !edit_policy.can_edit_business_json {
        record_entity_file_json_edit_rejected(&connection, &file, &edit_policy, operator_comment)?;
        return Err(edit_policy.reasons.join(" "));
    }
    drop(connection);

    if !file.file_exists {
        return Err(format!(
            "Entity file '{}' is marked missing and cannot be edited.",
            file.file_path
        ));
    }

    let file_path = canonical_registered_file_path(workdir_path, &file.file_path, true)?;
    let stable_read = read_stable_file(&file_path, file_stability_delay_ms)
        .map_err(|issue| format!("{}: {}", issue.code, issue.message))?;
    let current_checksum = format!("{:x}", Sha256::digest(&stable_read.bytes));
    if current_checksum != file.checksum
        || stable_read.file_size != file.file_size
        || stable_read.file_mtime != file.file_mtime
    {
        return Err(format!(
            "File '{}' changed since the last scan. Refresh or scan the workspace before saving.",
            file.file_path
        ));
    }

    let disk_json = serde_json::from_slice::<Value>(&stable_read.bytes).map_err(|error| {
        format!(
            "Failed to parse registered file '{}' before save: {error}",
            file.file_path
        )
    })?;
    let mut root = disk_json
        .as_object()
        .cloned()
        .ok_or_else(|| "Edited file root must be a JSON object.".to_string())?;
    let disk_entity_id = root.get("id").and_then(Value::as_str).unwrap_or_default();
    if disk_entity_id != file.entity_id {
        return Err(format!(
            "Registered file '{}' changed entity id from '{}' to '{}'. Run scan before editing.",
            file.file_path, file.entity_id, disk_entity_id
        ));
    }

    let payload = serde_json::from_str::<Value>(payload_json)
        .map_err(|error| format!("Payload JSON is invalid: {error}"))?;
    if payload.is_null() {
        return Err("Payload must exist and must not be null.".to_string());
    }
    let meta = serde_json::from_str::<Value>(meta_json)
        .map_err(|error| format!("Meta JSON is invalid: {error}"))?;
    if !meta.is_object() {
        return Err("Meta must be a JSON object.".to_string());
    }

    root.insert("payload".to_string(), payload.clone());
    root.insert("meta".to_string(), meta.clone());

    let updated_bytes = serde_json::to_vec_pretty(&Value::Object(root))
        .map_err(|error| format!("Failed to serialize edited JSON: {error}"))?;
    write_atomic_json(&file_path, &updated_bytes)?;

    let metadata = fs::metadata(&file_path).map_err(|error| {
        format!(
            "Failed to read metadata after saving '{}': {error}",
            file_path.display()
        )
    })?;
    let modified = metadata.modified().map_err(|error| {
        format!(
            "Failed to read modified time after saving '{}': {error}",
            file_path.display()
        )
    })?;
    let now = Utc::now().to_rfc3339();
    let updated_checksum = format!("{:x}", Sha256::digest(&updated_bytes));
    let status = parse_status(&file.status).ok_or_else(|| {
        format!(
            "Unknown file status '{}' for '{}'.",
            file.status, file.file_path
        )
    })?;

    let mut connection = open_connection(database_path)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start JSON-save transaction: {error}"))?;
    upsert_entity_file(
        &transaction,
        &PersistEntityFileInput {
            entity_id: file.entity_id.clone(),
            stage_id: file.stage_id.clone(),
            file_path: file.file_path.clone(),
            file_name: file.file_name.clone(),
            checksum: updated_checksum,
            file_mtime: system_time_to_rfc3339(modified),
            file_size: metadata.len(),
            payload_json: serde_json::to_string(&payload)
                .map_err(|error| format!("Failed to store payload JSON: {error}"))?,
            meta_json: serde_json::to_string(&meta)
                .map_err(|error| format!("Failed to store meta JSON: {error}"))?,
            current_stage: file.current_stage.clone(),
            next_stage: file.next_stage.clone(),
            status,
            validation_status: file.validation_status.clone(),
            validation_errors: file.validation_errors.clone(),
            is_managed_copy: file.is_managed_copy,
            copy_source_file_id: file.copy_source_file_id,
            first_seen_at: file.first_seen_at.clone(),
            last_seen_at: now.clone(),
            updated_at: now.clone(),
        },
    )?;
    recompute_entity_summaries(&transaction)?;
    insert_app_event(
        &transaction,
        AppEventLevel::Info,
        "entity_file_json_saved",
        &format!(
            "Business JSON was saved for entity file '{}'.",
            file.file_path
        ),
        Some(json!({
            "entity_id": &file.entity_id,
            "stage_id": &file.stage_id,
            "entity_file_id": entity_file_id,
            "file_path": &file.file_path,
            "operator_comment": operator_comment,
        })),
        &now,
    )?;
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit JSON-save transaction: {error}"))?;

    get_entity_detail_with_selection(database_path, &file.entity_id, Some(entity_file_id))?
        .ok_or_else(|| format!("Entity '{}' was not found after JSON save.", file.entity_id))
}

fn build_target_json(
    root: &Map<String, Value>,
    source_stage_id: &str,
    target_stage_id: &str,
    next_stage: Option<&str>,
    now: &str,
) -> Result<Value, String> {
    let mut next_root = root.clone();
    next_root.insert(
        "current_stage".to_string(),
        Value::String(target_stage_id.to_string()),
    );
    next_root.insert("status".to_string(), Value::String("pending".to_string()));
    next_root.insert(
        "next_stage".to_string(),
        next_stage
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );

    let mut meta = next_root
        .remove("meta")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    meta.insert("updated_at".to_string(), Value::String(now.to_string()));

    let mut beehive = meta
        .remove("beehive")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    beehive.insert(
        "copy_source_stage".to_string(),
        Value::String(source_stage_id.to_string()),
    );
    beehive.insert(
        "copy_target_stage".to_string(),
        Value::String(target_stage_id.to_string()),
    );
    beehive.insert(
        "copy_created_at".to_string(),
        Value::String(now.to_string()),
    );
    meta.insert("beehive".to_string(), Value::Object(beehive));
    next_root.insert("meta".to_string(), Value::Object(meta));

    Ok(Value::Object(next_root))
}

#[allow(clippy::too_many_arguments)]
fn choose_response_target_plan(
    source_root: &Map<String, Value>,
    source_entity_id: &str,
    source_stage_id: &str,
    route: &SavePathRoute,
    response_payload: &Value,
    response_meta: Option<Value>,
    source_file_id: i64,
    stage_run_id: &str,
    output_index: usize,
    output_count: usize,
    now: &str,
    planned_paths: &mut HashSet<String>,
) -> Result<PlannedResponseTarget, String> {
    let target_stage_id = route.stage.id.as_str();
    let next_stage = route.stage.next_stage.as_deref();
    let generated_id = generated_child_entity_id(
        source_entity_id,
        target_stage_id,
        output_index,
        response_payload,
    )?;
    let explicit_id = response_payload
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| is_safe_child_entity_id(value))
        .map(ToOwned::to_owned);
    let mut candidate_ids = Vec::new();
    if let Some(explicit_id) = explicit_id {
        candidate_ids.push(explicit_id);
    }
    if !candidate_ids
        .iter()
        .any(|candidate| candidate == &generated_id)
    {
        candidate_ids.push(generated_id);
    }

    let mut last_rejection = None;
    for child_entity_id in candidate_ids {
        let file_name = format!("{child_entity_id}.json");
        let target_path = route.target_dir.join(&file_name);
        let target_path_string = path_string(&target_path);
        if planned_paths.contains(&target_path_string) {
            last_rejection = Some(format!(
                "Output target path '{}' is already planned by another response item.",
                target_path.display()
            ));
            continue;
        }

        let target_json = build_target_json_from_response(
            source_root,
            &child_entity_id,
            source_entity_id,
            source_stage_id,
            target_stage_id,
            next_stage,
            response_payload.clone(),
            response_meta.clone(),
            source_file_id,
            stage_run_id,
            output_index,
            output_count,
            now,
        )?;
        let bytes = serde_json::to_vec_pretty(&target_json).map_err(|error| {
            format!("Failed to serialize target JSON for n8n response copy: {error}")
        })?;
        let checksum = format!("{:x}", Sha256::digest(&bytes));

        let (existing_bytes, existing_checksum) = if target_path.exists() {
            let existing_bytes = fs::read(&target_path).map_err(|error| {
                format!(
                    "Failed to read existing target file '{}' during response copy collision check: {error}",
                    target_path.display()
                )
            })?;
            let existing_json = serde_json::from_slice::<Value>(&existing_bytes).map_err(|error| {
                format!(
                    "Failed to parse existing target file '{}' during response copy collision check: {error}",
                    target_path.display()
                )
            })?;
            let existing_checksum = format!("{:x}", Sha256::digest(&existing_bytes));
            if !is_compatible_response_target(
                &existing_json,
                &checksum,
                &existing_checksum,
                &child_entity_id,
                source_entity_id,
                source_file_id,
                source_stage_id,
                target_stage_id,
                next_stage,
                response_payload,
                output_index,
            ) {
                last_rejection = Some(format!(
                    "Target path '{}' already exists with different content.",
                    target_path.display()
                ));
                continue;
            }
            (Some(existing_bytes), Some(existing_checksum))
        } else {
            (None, None)
        };

        planned_paths.insert(target_path_string);
        return Ok(PlannedResponseTarget {
            target_stage_id: route.stage.id.clone(),
            target_stage_next_stage: route.stage.next_stage.clone(),
            child_entity_id,
            file_name,
            target_path,
            bytes,
            checksum,
            existing_bytes,
            existing_checksum,
        });
    }

    Err(last_rejection.unwrap_or_else(|| {
        format!("No safe target file name could be planned for output item {output_index}.")
    }))
}

fn is_safe_child_entity_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn generated_child_entity_id(
    source_entity_id: &str,
    target_stage_id: &str,
    output_index: usize,
    response_payload: &Value,
) -> Result<String, String> {
    let canonical = canonical_json_bytes(response_payload)?;
    let hash = format!("{:x}", Sha256::digest(&canonical));
    Ok(format!(
        "{}__{}__{}_{}",
        sanitize_id_part(source_entity_id),
        sanitize_id_part(target_stage_id),
        output_index,
        &hash[..8]
    ))
}

fn sanitize_id_part(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('-').is_empty() {
        "item".to_string()
    } else {
        sanitized
    }
}

fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, String> {
    fn normalize(value: &Value) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
            Value::Object(object) => {
                let mut entries = object.iter().collect::<Vec<_>>();
                entries.sort_by(|left, right| left.0.cmp(right.0));
                let mut normalized = Map::new();
                for (key, value) in entries {
                    normalized.insert(key.clone(), normalize(value));
                }
                Value::Object(normalized)
            }
            other => other.clone(),
        }
    }
    serde_json::to_vec(&normalize(value))
        .map_err(|error| format!("Failed to serialize canonical payload JSON: {error}"))
}

#[allow(clippy::too_many_arguments)]
fn is_compatible_response_target(
    existing_json: &Value,
    planned_checksum: &str,
    existing_checksum: &str,
    child_entity_id: &str,
    source_entity_id: &str,
    source_file_id: i64,
    source_stage_id: &str,
    target_stage_id: &str,
    next_stage: Option<&str>,
    response_payload: &Value,
    output_index: usize,
) -> bool {
    if existing_checksum == planned_checksum {
        return true;
    }
    let Some(root) = existing_json.as_object() else {
        return false;
    };
    if root.get("id").and_then(Value::as_str) != Some(child_entity_id)
        || root.get("current_stage").and_then(Value::as_str) != Some(target_stage_id)
        || root.get("status").and_then(Value::as_str) != Some("pending")
        || root.get("payload") != Some(response_payload)
    {
        return false;
    }
    if optional_string(root.get("next_stage")) != next_stage {
        return false;
    }
    let Some(beehive) = root
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("beehive"))
        .and_then(Value::as_object)
    else {
        return false;
    };
    beehive.get("source_entity_id").and_then(Value::as_str) == Some(source_entity_id)
        && beehive.get("source_entity_file_id").and_then(Value::as_i64) == Some(source_file_id)
        && beehive.get("source_stage_id").and_then(Value::as_str) == Some(source_stage_id)
        && beehive.get("target_stage_id").and_then(Value::as_str) == Some(target_stage_id)
        && beehive.get("output_index").and_then(Value::as_u64) == Some(output_index as u64)
}

fn optional_string(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::String(value)) => Some(value.as_str()),
        Some(Value::Null) | None => None,
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_target_json_from_response(
    source_root: &Map<String, Value>,
    child_entity_id: &str,
    source_entity_id: &str,
    source_stage_id: &str,
    target_stage_id: &str,
    next_stage: Option<&str>,
    response_payload: Value,
    response_meta: Option<Value>,
    source_file_id: i64,
    stage_run_id: &str,
    output_index: usize,
    output_count: usize,
    now: &str,
) -> Result<Value, String> {
    if !response_payload.is_object() {
        return Err("n8n response payload must be a JSON object for next-stage copy.".to_string());
    }

    let mut root = Map::new();
    root.insert("id".to_string(), Value::String(child_entity_id.to_string()));
    root.insert(
        "current_stage".to_string(),
        Value::String(target_stage_id.to_string()),
    );
    root.insert(
        "next_stage".to_string(),
        next_stage
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );
    root.insert("status".to_string(), Value::String("pending".to_string()));
    root.insert("payload".to_string(), response_payload);

    let mut meta = source_root
        .get("meta")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(response_meta) = response_meta.and_then(|value| value.as_object().cloned()) {
        for (key, value) in response_meta {
            if key != "beehive" {
                meta.insert(key, value);
            }
        }
    }
    meta.insert("source".to_string(), Value::String("n8n".to_string()));
    meta.entry("created_at".to_string())
        .or_insert_with(|| Value::String(now.to_string()));
    meta.insert("updated_at".to_string(), Value::String(now.to_string()));

    let mut beehive = Map::new();
    beehive.insert(
        "created_by".to_string(),
        Value::String("n8n_response".to_string()),
    );
    beehive.insert(
        "source_entity_id".to_string(),
        Value::String(source_entity_id.to_string()),
    );
    beehive.insert(
        "source_stage_id".to_string(),
        Value::String(source_stage_id.to_string()),
    );
    beehive.insert(
        "target_stage_id".to_string(),
        Value::String(target_stage_id.to_string()),
    );
    beehive.insert(
        "source_entity_file_id".to_string(),
        Value::Number(source_file_id.into()),
    );
    beehive.insert(
        "stage_run_id".to_string(),
        Value::String(stage_run_id.to_string()),
    );
    beehive.insert(
        "output_index".to_string(),
        Value::Number((output_index as u64).into()),
    );
    beehive.insert(
        "output_count".to_string(),
        Value::Number((output_count as u64).into()),
    );
    beehive.insert(
        "copy_source_stage".to_string(),
        Value::String(source_stage_id.to_string()),
    );
    beehive.insert(
        "copy_target_stage".to_string(),
        Value::String(target_stage_id.to_string()),
    );
    beehive.insert(
        "copy_created_at".to_string(),
        Value::String(now.to_string()),
    );
    meta.insert("beehive".to_string(), Value::Object(beehive));
    root.insert("meta".to_string(), Value::Object(meta));

    Ok(Value::Object(root))
}

fn write_atomic_json(target_path: &Path, bytes: &[u8]) -> Result<(), String> {
    let Some(parent) = target_path.parent() else {
        return Err(format!(
            "Target path '{}' does not have a parent directory.",
            target_path.display()
        ));
    };

    let tmp_path = parent.join(format!(
        ".beehive-tmp-{}-{}.json",
        Utc::now()
            .timestamp_nanos_opt()
            .unwrap_or_else(|| Utc::now().timestamp_micros() * 1000),
        std::process::id()
    ));

    let write_result = (|| -> Result<(), String> {
        let mut file = File::create(&tmp_path).map_err(|error| {
            format!(
                "Failed to create temporary file '{}' for managed copy: {error}",
                tmp_path.display()
            )
        })?;
        file.write_all(bytes).map_err(|error| {
            format!(
                "Failed to write temporary file '{}' for managed copy: {error}",
                tmp_path.display()
            )
        })?;
        file.sync_all().map_err(|error| {
            format!(
                "Failed to sync temporary file '{}' for managed copy: {error}",
                tmp_path.display()
            )
        })?;
        fs::rename(&tmp_path, target_path).map_err(|error| {
            format!(
                "Failed to move temporary file '{}' to '{}': {error}",
                tmp_path.display(),
                target_path.display()
            )
        })?;
        Ok(())
    })();

    if write_result.is_err() && tmp_path.exists() {
        let _ = fs::remove_file(&tmp_path);
    }

    write_result
}

pub(crate) fn canonical_registered_file_path(
    workdir_path: &Path,
    registered_file_path: &str,
    must_exist: bool,
) -> Result<PathBuf, String> {
    let workdir = workdir_path.canonicalize().map_err(|error| {
        format!(
            "Failed to canonicalize workdir '{}': {error}",
            workdir_path.display()
        )
    })?;
    let candidate = PathBuf::from(registered_file_path);
    if !candidate.is_absolute() {
        return Err(format!(
            "Registered file path '{}' is not absolute.",
            registered_file_path
        ));
    }
    if must_exist && !candidate.exists() {
        return Err(format!(
            "Registered file path '{}' does not exist.",
            registered_file_path
        ));
    }

    let canonical = if candidate.exists() {
        candidate.canonicalize().map_err(|error| {
            format!(
                "Failed to canonicalize registered file '{}': {error}",
                candidate.display()
            )
        })?
    } else {
        let parent = candidate.parent().ok_or_else(|| {
            format!(
                "Registered file path '{}' does not have a parent directory.",
                registered_file_path
            )
        })?;
        let canonical_parent = parent.canonicalize().map_err(|error| {
            format!(
                "Failed to canonicalize registered file parent '{}': {error}",
                parent.display()
            )
        })?;
        canonical_parent.join(candidate.file_name().ok_or_else(|| {
            format!(
                "Registered file path '{}' does not have a file name.",
                registered_file_path
            )
        })?)
    };

    if !canonical.starts_with(&workdir) {
        return Err(format!(
            "Registered file path '{}' is outside the selected workdir.",
            registered_file_path
        ));
    }
    Ok(canonical)
}

fn register_target_file(
    database_path: &Path,
    target_stage_id: &str,
    target_stage_next_stage: &Option<String>,
    entity_id: &str,
    target_path: &Path,
    file_name: &str,
    checksum: &str,
    bytes: &[u8],
    copy_source_file_id: Option<i64>,
    now: &str,
) -> Result<crate::domain::EntityFileRecord, String> {
    let mut connection = open_connection(database_path)?;
    let transaction = connection.transaction().map_err(|error| {
        format!("Failed to start target file registration transaction: {error}")
    })?;
    ensure_entity_stub(&transaction, entity_id, now)?;

    let metadata = fs::metadata(target_path).map_err(|error| {
        format!(
            "Failed to read target file metadata '{}': {error}",
            target_path.display()
        )
    })?;
    let file_mtime = metadata
        .modified()
        .map(system_time_to_rfc3339)
        .map_err(|error| {
            format!(
                "Failed to read target file modified time '{}': {error}",
                target_path.display()
            )
        })?;
    let file_size = metadata.len();

    let parsed = serde_json::from_slice::<Value>(bytes)
        .map_err(|error| format!("Failed to parse target JSON after write: {error}"))?;
    let root = parsed
        .as_object()
        .ok_or_else(|| "Managed copy target JSON root must be an object.".to_string())?;
    let payload_value = root
        .get("payload")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let meta_value = root
        .get("meta")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    let payload_json = serde_json::to_string(&payload_value)
        .map_err(|error| format!("Failed to serialize managed copy payload JSON: {error}"))?;
    let meta_json = serde_json::to_string(&meta_value)
        .map_err(|error| format!("Failed to serialize managed copy meta JSON: {error}"))?;

    let (_outcome, file_id) = upsert_entity_file(
        &transaction,
        &PersistEntityFileInput {
            entity_id: entity_id.to_string(),
            stage_id: target_stage_id.to_string(),
            file_path: path_string(target_path),
            file_name: file_name.to_string(),
            checksum: checksum.to_string(),
            file_mtime,
            file_size,
            payload_json,
            meta_json,
            current_stage: Some(target_stage_id.to_string()),
            next_stage: target_stage_next_stage.clone(),
            status: StageStatus::Pending,
            validation_status: EntityValidationStatus::Valid,
            validation_errors: Vec::new(),
            is_managed_copy: true,
            copy_source_file_id,
            first_seen_at: now.to_string(),
            last_seen_at: now.to_string(),
            updated_at: now.to_string(),
        },
    )?;

    let stage = find_stage_by_id(&transaction, target_stage_id)?.ok_or_else(|| {
        format!(
            "Target stage '{}' disappeared during registration.",
            target_stage_id
        )
    })?;
    upsert_entity_stage_state(
        &transaction,
        &PersistEntityStageStateInput {
            entity_id: entity_id.to_string(),
            stage_id: target_stage_id.to_string(),
            file_path: path_string(target_path),
            file_instance_id: Some(file_id),
            file_exists: true,
            status: StageStatus::Pending,
            max_attempts: stage.max_attempts,
            discovered_at: now.to_string(),
            last_seen_at: now.to_string(),
            updated_at: now.to_string(),
        },
    )?;
    recompute_entity_summaries(&transaction)?;
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit target file registration: {error}"))?;

    let connection = open_connection(database_path)?;
    find_latest_entity_file_for_stage(&connection, entity_id, target_stage_id)?
        .ok_or_else(|| {
            format!(
                "Target file registration finished but no entity file was found for entity '{}' on stage '{}'.",
                entity_id, target_stage_id
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{
        bootstrap_database, get_entity_detail, list_app_events, list_entity_files,
    };
    use crate::discovery::scan_workspace;
    use crate::domain::{
        EntityFileRecord, PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition,
    };
    use rusqlite::params;

    fn test_config(stages: Vec<StageDefinition>) -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            runtime: RuntimeConfig::default(),
            stages,
        }
    }

    fn stage(
        id: &str,
        input_folder: &str,
        output_folder: &str,
        next_stage: Option<&str>,
    ) -> StageDefinition {
        StageDefinition {
            id: id.to_string(),
            input_folder: input_folder.to_string(),
            output_folder: output_folder.to_string(),
            workflow_url: format!("http://localhost:5678/webhook/{id}"),
            max_attempts: 3,
            retry_delay_sec: 10,
            next_stage: next_stage.map(ToOwned::to_owned),
        }
    }

    fn write_json(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, contents).expect("write json");
    }

    fn setup_business_json_edit_case(
        status: Option<&str>,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        PathBuf,
        PathBuf,
        EntityFileRecord,
    ) {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/incoming-out",
                None,
            )]),
        )
        .expect("bootstrap");
        let source_path = workdir.join("stages/incoming/entity-1.json");
        write_json(
            &source_path,
            r#"{"id":"entity-1","current_stage":"incoming","status":"pending","payload":{"value":1},"meta":{"source":"manual"}}"#,
        );
        scan_workspace(&workdir, &database_path).expect("scan");
        if let Some(status) = status {
            let connection = open_connection(&database_path).expect("connection");
            connection
                .execute(
                    "UPDATE entity_stage_states SET status = ?1 WHERE entity_id = 'entity-1'",
                    params![status],
                )
                .expect("set status");
            connection
                .execute(
                    "UPDATE entities SET current_status = ?1 WHERE entity_id = 'entity-1'",
                    params![status],
                )
                .expect("set entity status");
        }
        let file = list_entity_files(&database_path, Some("entity-1")).expect("files")[0].clone();
        (tempdir, workdir, database_path, source_path, file)
    }

    #[test]
    fn copy_to_active_next_stage_creates_target_file_and_keeps_source_unchanged() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage(
                    "incoming",
                    "stages/incoming",
                    "stages/incoming-out",
                    Some("normalized"),
                ),
                stage(
                    "normalized",
                    "stages/normalized",
                    "stages/normalized-out",
                    Some("enriched"),
                ),
                stage("enriched", "stages/enriched", "stages/enriched-out", None),
            ]),
        )
        .expect("bootstrap");

        write_json(
            &source_path,
            r#"{
  "id": "entity-1",
  "current_stage": "incoming",
  "next_stage": "normalized",
  "status": "pending",
  "payload": {"value": 1},
  "meta": {"source": "manual"}
}"#,
        );
        scan_workspace(&workdir, &database_path).expect("scan");

        let payload = create_next_stage_copy(&workdir, &database_path, "entity-1", "incoming")
            .expect("copy payload");
        let detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");
        let target_path = workdir
            .join("stages")
            .join("normalized")
            .join("entity-1.json");
        let target_json =
            serde_json::from_slice::<Value>(&fs::read(&target_path).expect("read target"))
                .expect("parse target");
        let source_json =
            serde_json::from_slice::<Value>(&fs::read(&source_path).expect("read source"))
                .expect("parse source");

        assert_eq!(payload.status, FileCopyStatus::Created);
        assert!(target_path.exists());
        assert_eq!(detail.files.len(), 2);
        assert_eq!(
            target_json.get("current_stage").and_then(Value::as_str),
            Some("normalized")
        );
        assert_eq!(
            target_json.get("status").and_then(Value::as_str),
            Some("pending")
        );
        assert_eq!(
            target_json.get("next_stage").and_then(Value::as_str),
            Some("enriched")
        );
        assert_eq!(
            target_json
                .get("meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("beehive"))
                .and_then(Value::as_object)
                .and_then(|beehive| beehive.get("copy_source_stage"))
                .and_then(Value::as_str),
            Some("incoming")
        );
        assert_eq!(
            source_json.get("current_stage").and_then(Value::as_str),
            Some("incoming")
        );
        assert_eq!(
            detail
                .stage_states
                .iter()
                .find(|state| state.stage_id == "normalized")
                .map(|state| state.status.as_str()),
            Some("pending")
        );
        assert_eq!(
            detail
                .stage_states
                .iter()
                .find(|state| state.stage_id == "incoming")
                .map(|state| state.status.as_str()),
            Some("pending")
        );
    }

    #[test]
    fn repeated_compatible_copy_returns_already_exists() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let source_path = workdir
            .join("stages")
            .join("incoming")
            .join("entity-1.json");
        let target_path = workdir
            .join("stages")
            .join("normalized")
            .join("entity-1.json");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage(
                    "incoming",
                    "stages/incoming",
                    "stages/incoming-out",
                    Some("normalized"),
                ),
                stage(
                    "normalized",
                    "stages/normalized",
                    "stages/normalized-out",
                    None,
                ),
            ]),
        )
        .expect("bootstrap");

        write_json(
            &source_path,
            r#"{"id":"entity-1","current_stage":"incoming","next_stage":"normalized","status":"pending","payload":{"value":1},"meta":{"source":"manual"}}"#,
        );
        scan_workspace(&workdir, &database_path).expect("scan");
        create_next_stage_copy(&workdir, &database_path, "entity-1", "incoming")
            .expect("first copy");

        let source_before = fs::read(&source_path).expect("read source before");
        let mut target_json = serde_json::from_slice::<Value>(
            &fs::read(&target_path).expect("read target after first copy"),
        )
        .expect("parse target after first copy");
        let target_root = target_json
            .as_object_mut()
            .expect("target root should be object");
        let meta = target_root
            .entry("meta")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("target meta should be object");
        meta.insert(
            "updated_at".to_string(),
            Value::String("2030-01-02T03:04:05Z".to_string()),
        );
        meta.insert(
            "operator_note".to_string(),
            Value::String("existing-target-kept".to_string()),
        );
        let beehive = meta
            .entry("beehive")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("beehive meta should be object");
        beehive.insert(
            "copy_created_at".to_string(),
            Value::String("2030-01-02T03:04:05Z".to_string()),
        );
        let target_bytes_before =
            serde_json::to_vec_pretty(&target_json).expect("serialize mutated target");
        fs::write(&target_path, &target_bytes_before).expect("write mutated target");
        let target_checksum_before = format!("{:x}", Sha256::digest(&target_bytes_before));
        let target_mtime_before = fs::metadata(&target_path)
            .expect("target metadata before")
            .modified()
            .expect("target modified before");

        let second = create_next_stage_copy(&workdir, &database_path, "entity-1", "incoming")
            .expect("second copy");
        let target_bytes_after = fs::read(&target_path).expect("read target after second copy");
        let target_mtime_after = fs::metadata(&target_path)
            .expect("target metadata after")
            .modified()
            .expect("target modified after");
        let files = list_entity_files(&database_path, Some("entity-1")).expect("files");
        let target_file = files
            .iter()
            .find(|file| file.stage_id == "normalized")
            .expect("normalized target file exists");
        let detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");
        let expected_payload_json =
            serde_json::to_string(&target_json.get("payload").cloned().expect("payload exists"))
                .expect("serialize expected payload");
        let expected_meta_json =
            serde_json::to_string(&target_json.get("meta").cloned().expect("meta exists"))
                .expect("serialize expected meta");
        let expected_preview = json!({
            "id": "entity-1",
            "current_stage": "normalized",
            "next_stage": Value::Null,
            "status": "pending",
            "payload": target_json.get("payload").cloned().expect("payload exists"),
            "meta": target_json.get("meta").cloned().expect("meta exists"),
        });

        assert_eq!(second.status, FileCopyStatus::AlreadyExists);
        assert_eq!(target_bytes_after, target_bytes_before);
        assert_eq!(target_mtime_after, target_mtime_before);
        assert_eq!(
            fs::read(&source_path).expect("read source after"),
            source_before
        );
        assert_eq!(target_file.checksum, target_checksum_before);
        assert_eq!(target_file.payload_json, expected_payload_json);
        assert_eq!(target_file.meta_json, expected_meta_json);
        assert_eq!(
            serde_json::from_str::<Value>(&detail.latest_json_preview).expect("parse preview"),
            expected_preview
        );
    }

    #[test]
    fn target_collision_with_different_content_fails_safely() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![
                stage(
                    "incoming",
                    "stages/incoming",
                    "stages/incoming-out",
                    Some("normalized"),
                ),
                stage(
                    "normalized",
                    "stages/normalized",
                    "stages/normalized-out",
                    None,
                ),
            ]),
        )
        .expect("bootstrap");

        write_json(
            &workdir
                .join("stages")
                .join("incoming")
                .join("entity-1.json"),
            r#"{"id":"entity-1","current_stage":"incoming","next_stage":"normalized","status":"pending","payload":{"value":1},"meta":{"source":"manual"}}"#,
        );
        write_json(
            &workdir
                .join("stages")
                .join("normalized")
                .join("entity-1.json"),
            r#"{"id":"entity-1","current_stage":"normalized","next_stage":null,"status":"pending","payload":{"value":999},"meta":{"source":"manual"}}"#,
        );
        scan_workspace(&workdir, &database_path).expect("scan");

        let payload = create_next_stage_copy(&workdir, &database_path, "entity-1", "incoming")
            .expect("copy payload");
        let files = list_entity_files(&database_path, Some("entity-1")).expect("files");

        assert_eq!(payload.status, FileCopyStatus::Failed);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn no_target_stage_returns_blocked_without_fake_stage() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/incoming-out",
                None,
            )]),
        )
        .expect("bootstrap");

        write_json(
            &workdir
                .join("stages")
                .join("incoming")
                .join("entity-1.json"),
            r#"{"id":"entity-1","current_stage":"incoming","status":"pending","payload":{"value":1},"meta":{"source":"manual"}}"#,
        );
        scan_workspace(&workdir, &database_path).expect("scan");

        let payload = create_next_stage_copy(&workdir, &database_path, "entity-1", "incoming")
            .expect("copy payload");
        let detail = get_entity_detail(&database_path, "entity-1")
            .expect("detail result")
            .expect("detail exists");

        assert_eq!(payload.status, FileCopyStatus::Blocked);
        assert_eq!(detail.files.len(), 1);
    }

    #[test]
    fn save_business_json_updates_file_snapshot_for_editable_state() {
        let (_tempdir, workdir, database_path, source_path, file) =
            setup_business_json_edit_case(Some("pending"));

        let detail = save_entity_file_business_json(
            &workdir,
            &database_path,
            file.id,
            r#"{"value":2}"#,
            r#"{"source":"operator"}"#,
            Some("edit payload"),
            0,
        )
        .expect("save");
        let saved_bytes = fs::read(&source_path).expect("saved file");
        let saved_json = serde_json::from_slice::<Value>(&saved_bytes).expect("saved json");
        let saved_file = detail
            .files
            .iter()
            .find(|item| item.id == file.id)
            .expect("file");
        let saved_state = detail
            .stage_states
            .iter()
            .find(|state| state.stage_id == "incoming")
            .expect("state");

        assert_eq!(saved_json["payload"]["value"], 2);
        assert_eq!(saved_json["meta"]["source"], "operator");
        assert_ne!(saved_file.checksum, file.checksum);
        assert_eq!(saved_state.status, "pending");
        assert_eq!(detail.entity.current_status, "pending");
    }

    #[test]
    fn save_business_json_rejects_stale_disk_snapshot() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        bootstrap_database(
            &database_path,
            &test_config(vec![stage(
                "incoming",
                "stages/incoming",
                "stages/incoming-out",
                None,
            )]),
        )
        .expect("bootstrap");
        let source_path = workdir.join("stages/incoming/entity-1.json");
        write_json(
            &source_path,
            r#"{"id":"entity-1","payload":{"value":1},"meta":{}}"#,
        );
        scan_workspace(&workdir, &database_path).expect("scan");
        let file = list_entity_files(&database_path, Some("entity-1")).expect("files")[0].clone();
        write_json(
            &source_path,
            r#"{"id":"entity-1","payload":{"value":99},"meta":{}}"#,
        );

        let error = save_entity_file_business_json(
            &workdir,
            &database_path,
            file.id,
            r#"{"value":2}"#,
            r#"{}"#,
            None,
            0,
        )
        .expect_err("stale save should reject");

        assert!(error.contains("changed since the last scan"));
    }

    #[test]
    fn save_business_json_allows_only_editable_runtime_statuses() {
        for status in ["pending", "retry_wait", "failed", "blocked", "skipped"] {
            let (_tempdir, workdir, database_path, source_path, file) =
                setup_business_json_edit_case(Some(status));

            let detail = save_entity_file_business_json(
                &workdir,
                &database_path,
                file.id,
                r#"{"value":42}"#,
                r#"{"source":"operator"}"#,
                Some("allowed edit"),
                0,
            )
            .unwrap_or_else(|error| panic!("status {status} should allow save: {error}"));
            let saved_json =
                serde_json::from_slice::<Value>(&fs::read(&source_path).expect("saved bytes"))
                    .expect("saved json");
            let saved_state = detail
                .stage_states
                .iter()
                .find(|state| state.stage_id == "incoming")
                .expect("state");

            assert_eq!(saved_json["payload"]["value"], 42);
            assert_eq!(saved_json["meta"]["source"], "operator");
            assert_eq!(saved_state.status, status);
        }
    }

    #[test]
    fn save_business_json_rejects_active_complete_and_missing_runtime_state_without_mutation() {
        for status in ["queued", "in_progress", "done"] {
            let (_tempdir, workdir, database_path, source_path, file) =
                setup_business_json_edit_case(Some(status));
            let original_bytes = fs::read(&source_path).expect("original bytes");

            let error = save_entity_file_business_json(
                &workdir,
                &database_path,
                file.id,
                r#"{"value":99}"#,
                r#"{"source":"operator"}"#,
                Some("rejected edit"),
                0,
            )
            .expect_err("forbidden status should reject save");
            let after_bytes = fs::read(&source_path).expect("after bytes");
            let after_file =
                list_entity_files(&database_path, Some("entity-1")).expect("files")[0].clone();
            let events = list_app_events(&database_path, 20).expect("events");

            assert!(error.contains(status));
            assert_eq!(after_bytes, original_bytes);
            assert_eq!(after_file.checksum, file.checksum);
            assert_eq!(after_file.file_mtime, file.file_mtime);
            assert!(events
                .iter()
                .any(|event| event.code == "entity_file_json_edit_rejected"));
        }

        let (_tempdir, workdir, database_path, source_path, file) =
            setup_business_json_edit_case(None);
        let original_bytes = fs::read(&source_path).expect("original bytes");
        let connection = open_connection(&database_path).expect("connection");
        connection
            .execute(
                "DELETE FROM entity_stage_states WHERE entity_id = 'entity-1' AND stage_id = 'incoming'",
                [],
            )
            .expect("delete state");
        drop(connection);

        let error = save_entity_file_business_json(
            &workdir,
            &database_path,
            file.id,
            r#"{"value":99}"#,
            r#"{"source":"operator"}"#,
            Some("missing state edit"),
            0,
        )
        .expect_err("missing state should reject save");
        let after_bytes = fs::read(&source_path).expect("after bytes");
        let after_file =
            list_entity_files(&database_path, Some("entity-1")).expect("files")[0].clone();
        let events = list_app_events(&database_path, 20).expect("events");

        assert!(error.contains("No runtime stage state exists"));
        assert_eq!(after_bytes, original_bytes);
        assert_eq!(after_file.checksum, file.checksum);
        assert_eq!(after_file.file_mtime, file.file_mtime);
        assert!(events
            .iter()
            .any(|event| event.code == "entity_file_json_edit_rejected"));
    }
}

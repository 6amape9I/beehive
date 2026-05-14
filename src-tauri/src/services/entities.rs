use std::collections::{HashMap, HashSet};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::database::{self, RegisterS3ArtifactPointerInput};
use crate::domain::{
    EntityDetailPayload, EntityDetailResult, EntityListQuery, EntityListResult,
    EntityMutationPayload, EntityMutationResult, ImportJsonBatchPayload, ImportJsonBatchRequest,
    ImportJsonFileInput, ImportJsonFileResult, S3StorageConfig, StageStatus, StorageProvider,
    UpdateEntityRequest,
};
use crate::s3_client::{AwsS3MetadataClient, S3MetadataClient, S3ObjectMetadata};
use crate::services::workspaces;

trait JsonObjectUploader {
    fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, String>;
    fn put_json_object(
        &self,
        bucket: &str,
        key: &str,
        bytes: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> Result<S3ObjectMetadata, String>;
}

struct AwsJsonObjectUploader {
    client: AwsS3MetadataClient,
}

impl JsonObjectUploader for AwsJsonObjectUploader {
    fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, String> {
        Ok(self.client.head_object(bucket, key)?.is_some())
    }

    fn put_json_object(
        &self,
        bucket: &str,
        key: &str,
        bytes: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> Result<S3ObjectMetadata, String> {
        self.client.put_json_object(bucket, key, bytes, metadata)
    }
}

pub(crate) fn list_entities_for_workspace(
    workspace_id: &str,
    query: EntityListQuery,
) -> Result<EntityListResult, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    let page = database::list_entity_table_page(&workspace.database_path, &query)?;
    let stages = database::list_stages(&workspace.database_path)?;
    Ok(EntityListResult {
        total: page.total,
        page: page.page,
        page_size: page.page_size,
        available_stages: stages.into_iter().map(|stage| stage.id).collect(),
        available_statuses: page.available_statuses,
        entities: page.entities,
        errors: Vec::new(),
    })
}

pub(crate) fn get_entity_for_workspace(
    workspace_id: &str,
    entity_id: &str,
) -> Result<Option<EntityDetailPayload>, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    database::get_entity_detail(&workspace.database_path, entity_id)
}

pub(crate) fn update_entity_for_workspace(
    workspace_id: &str,
    entity_id: &str,
    input: &UpdateEntityRequest,
) -> Result<Option<EntityMutationPayload>, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    Ok(
        database::update_entity_metadata(&workspace.database_path, entity_id, input)?
            .map(|entity| EntityMutationPayload { entity }),
    )
}

pub(crate) fn archive_entity_for_workspace(
    workspace_id: &str,
    entity_id: &str,
) -> Result<Option<EntityMutationPayload>, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    Ok(
        database::archive_entity(&workspace.database_path, entity_id)?
            .map(|entity| EntityMutationPayload { entity }),
    )
}

pub(crate) fn restore_entity_for_workspace(
    workspace_id: &str,
    entity_id: &str,
) -> Result<Option<EntityMutationPayload>, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    Ok(
        database::restore_entity(&workspace.database_path, entity_id)?
            .map(|entity| EntityMutationPayload { entity }),
    )
}

pub(crate) fn entity_detail_result(
    detail: Option<EntityDetailPayload>,
    not_found_message: String,
) -> EntityDetailResult {
    match detail {
        Some(detail) => EntityDetailResult {
            detail: Some(detail),
            errors: Vec::new(),
        },
        None => EntityDetailResult {
            detail: None,
            errors: vec![crate::domain::CommandErrorInfo {
                code: "entity_not_found".to_string(),
                message: not_found_message,
                path: None,
            }],
        },
    }
}

pub(crate) fn entity_mutation_result(
    payload: Option<EntityMutationPayload>,
    not_found_message: String,
) -> EntityMutationResult {
    match payload {
        Some(payload) => EntityMutationResult {
            payload: Some(payload),
            errors: Vec::new(),
        },
        None => EntityMutationResult {
            payload: None,
            errors: vec![crate::domain::CommandErrorInfo {
                code: "entity_not_found".to_string(),
                message: not_found_message,
                path: None,
            }],
        },
    }
}

pub(crate) fn import_json_batch_for_workspace(
    workspace_id: &str,
    input: &ImportJsonBatchRequest,
) -> Result<ImportJsonBatchPayload, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    let storage = workspace_s3_storage(&workspace)?;
    let uploader = AwsJsonObjectUploader {
        client: AwsS3MetadataClient::from_storage_config(&storage)?,
    };
    import_json_batch_with_uploader(workspace_id, input, &uploader)
}

fn import_json_batch_with_uploader(
    workspace_id: &str,
    input: &ImportJsonBatchRequest,
    uploader: &dyn JsonObjectUploader,
) -> Result<ImportJsonBatchPayload, String> {
    let workspace = workspaces::get_workspace(workspace_id)?;
    let storage = workspace_s3_storage(&workspace)?;
    let stage_id = normalize_stage_id(&input.stage_id)?;
    let stage = database::list_stages(&workspace.database_path)?
        .into_iter()
        .find(|stage| stage.id == stage_id)
        .ok_or_else(|| format!("Target stage '{stage_id}' was not found."))?;
    if !stage.is_active {
        return Err(format!("Target stage '{stage_id}' is inactive."));
    }

    let overwrite_existing = input.options.overwrite_existing.unwrap_or(false);
    let mut seen_keys = HashSet::new();
    let mut results = Vec::new();

    for file in &input.files {
        match import_one_file(
            &workspace.database_path,
            &storage,
            &stage_id,
            &mut seen_keys,
            overwrite_existing,
            file,
            uploader,
        ) {
            Ok(result) => results.push(result),
            Err(error) => {
                let (status, message) = if let Some(message) = error.strip_prefix("invalid: ") {
                    ("invalid", message.to_string())
                } else {
                    ("failed", error)
                };
                results.push(ImportJsonFileResult {
                    file_name: file.file_name.clone(),
                    status: status.to_string(),
                    entity_id: None,
                    artifact_id: None,
                    bucket: None,
                    key: None,
                    object_key: None,
                    error: Some(message),
                })
            }
        }
    }

    let imported_count = results
        .iter()
        .filter(|result| result.status == "imported")
        .count() as u64;
    let invalid_count = results
        .iter()
        .filter(|result| result.status == "invalid")
        .count() as u64;
    let failed_count = results
        .iter()
        .filter(|result| result.status == "failed")
        .count() as u64;
    let skipped_count = results
        .iter()
        .filter(|result| result.status == "skipped")
        .count() as u64;

    Ok(ImportJsonBatchPayload {
        stage_id,
        uploaded_count: imported_count,
        registered_count: imported_count,
        imported_count,
        invalid_count,
        failed_count,
        skipped_count,
        files: results,
    })
}

fn import_one_file(
    database_path: &std::path::Path,
    storage: &S3StorageConfig,
    stage_id: &str,
    seen_keys: &mut HashSet<String>,
    overwrite_existing: bool,
    file: &ImportJsonFileInput,
    uploader: &dyn JsonObjectUploader,
) -> Result<ImportJsonFileResult, String> {
    if !file.content.is_object() {
        return Err("invalid: JSON file content must be an object.".to_string());
    }
    let file_name = sanitize_json_file_name(&file.file_name)?;
    let bytes = serde_json::to_vec(&file.content)
        .map_err(|error| format!("Failed to serialize JSON content: {error}"))?;
    let checksum = sha256_hex(&bytes);
    let short_hash = checksum.chars().take(12).collect::<String>();
    let entity_id = derive_entity_id(&file.content, &file_name, &short_hash);
    let artifact_id = derive_artifact_id(&file.content, &entity_id, &short_hash);
    let object_key = choose_object_key(
        storage,
        stage_id,
        &file_name,
        &short_hash,
        seen_keys,
        overwrite_existing,
        uploader,
    )?;
    let mut metadata = HashMap::new();
    metadata.insert("beehive-entity-id".to_string(), entity_id.clone());
    metadata.insert("beehive-artifact-id".to_string(), artifact_id.clone());
    metadata.insert("beehive-stage-id".to_string(), stage_id.to_string());

    let uploaded = uploader.put_json_object(&storage.bucket, &object_key, bytes, metadata)?;
    let files = database::register_s3_artifact_pointers(
        database_path,
        &[RegisterS3ArtifactPointerInput {
            entity_id: entity_id.clone(),
            artifact_id: artifact_id.clone(),
            relation_to_source: Some("source".to_string()),
            stage_id: stage_id.to_string(),
            bucket: storage.bucket.clone(),
            key: object_key.clone(),
            version_id: uploaded.version_id,
            etag: uploaded.etag,
            checksum_sha256: Some(checksum),
            size: uploaded.size,
            last_modified: uploaded.last_modified,
            source_file_id: None,
            producer_run_id: None,
            status: StageStatus::Pending,
        }],
    )?;
    let registered = files
        .first()
        .ok_or_else(|| "S3 registration returned no entity file.".to_string())?;

    Ok(ImportJsonFileResult {
        file_name,
        status: "imported".to_string(),
        entity_id: Some(registered.entity_id.clone()),
        artifact_id: registered.artifact_id.clone(),
        bucket: registered.bucket.clone(),
        key: registered.key.clone(),
        object_key: registered.key.clone(),
        error: None,
    })
}

fn workspace_s3_storage(
    workspace: &workspaces::RegisteredWorkspace,
) -> Result<S3StorageConfig, String> {
    if workspace.provider != StorageProvider::S3 {
        return Err(format!(
            "Workspace '{}' is not an S3 workspace.",
            workspace.id
        ));
    }
    Ok(S3StorageConfig {
        bucket: workspace
            .bucket
            .clone()
            .ok_or_else(|| format!("Workspace '{}' has no bucket.", workspace.id))?,
        workspace_prefix: workspace
            .workspace_prefix
            .clone()
            .ok_or_else(|| format!("Workspace '{}' has no workspace_prefix.", workspace.id))?,
        region: workspace.region.clone(),
        endpoint: workspace.endpoint.clone(),
    })
}

fn choose_object_key(
    storage: &S3StorageConfig,
    stage_id: &str,
    file_name: &str,
    short_hash: &str,
    seen_keys: &mut HashSet<String>,
    overwrite_existing: bool,
    uploader: &dyn JsonObjectUploader,
) -> Result<String, String> {
    let prefix = storage.workspace_prefix.trim_matches('/');
    let base_key = format!("{prefix}/stages/{stage_id}/{file_name}");
    if overwrite_existing {
        seen_keys.insert(base_key.clone());
        return Ok(base_key);
    }
    if seen_keys.insert(base_key.clone()) && !uploader.object_exists(&storage.bucket, &base_key)? {
        return Ok(base_key);
    }

    let hashed_name = append_hash_suffix(file_name, short_hash);
    let hashed_key = format!("{prefix}/stages/{stage_id}/{hashed_name}");
    if seen_keys.insert(hashed_key.clone())
        && !uploader.object_exists(&storage.bucket, &hashed_key)?
    {
        return Ok(hashed_key);
    }

    for index in 2..1000 {
        let candidate_name = append_hash_and_index(file_name, short_hash, index);
        let candidate_key = format!("{prefix}/stages/{stage_id}/{candidate_name}");
        if seen_keys.insert(candidate_key.clone())
            && !uploader.object_exists(&storage.bucket, &candidate_key)?
        {
            return Ok(candidate_key);
        }
    }

    Err(format!(
        "Could not choose a non-conflicting S3 object key for '{file_name}'."
    ))
}

fn sanitize_json_file_name(value: &str) -> Result<String, String> {
    let leaf = value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(value)
        .trim()
        .to_string();
    if leaf.is_empty() {
        return Err("invalid: file_name is required.".to_string());
    }
    if leaf.contains("..") {
        return Err("invalid: file_name must not contain '..'.".to_string());
    }
    let sanitized = leaf
        .chars()
        .map(|ch| {
            if ch.is_control() || ch == '/' || ch == '\\' {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    if !sanitized.to_ascii_lowercase().ends_with(".json") {
        return Err("invalid: file_name must end with .json.".to_string());
    }
    Ok(sanitized)
}

fn derive_entity_id(content: &Value, file_name: &str, short_hash: &str) -> String {
    content
        .get("entity_id")
        .and_then(Value::as_str)
        .or_else(|| content.get("id").and_then(Value::as_str))
        .map(sanitize_logical_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let stem = file_stem(file_name);
            let stem = sanitize_logical_id(&stem);
            format!("{stem}_{short_hash}")
        })
}

fn derive_artifact_id(content: &Value, entity_id: &str, short_hash: &str) -> String {
    content
        .get("artifact_id")
        .and_then(Value::as_str)
        .map(sanitize_logical_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            if entity_id.is_empty() {
                format!("source__{short_hash}")
            } else {
                format!("{entity_id}__source")
            }
        })
}

fn sanitize_logical_id(value: &str) -> String {
    let mut output = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_control() || ch == '/' || ch == '\\' {
                '_'
            } else if ch.is_whitespace() {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    while output.contains("..") {
        output = output.replace("..", ".");
    }
    output.trim_matches(['_', '.', '-']).to_string()
}

fn normalize_stage_id(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("stage_id is required.".to_string());
    }
    Ok(value.to_string())
}

fn append_hash_suffix(file_name: &str, short_hash: &str) -> String {
    let stem = file_stem(file_name);
    format!("{stem}__{short_hash}.json")
}

fn append_hash_and_index(file_name: &str, short_hash: &str, index: u64) -> String {
    let stem = file_stem(file_name);
    format!("{stem}__{short_hash}_{index}.json")
}

fn file_stem(file_name: &str) -> String {
    file_name
        .strip_suffix(".json")
        .or_else(|| file_name.strip_suffix(".JSON"))
        .unwrap_or(file_name)
        .to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_json_file_name_without_losing_cyrillic() {
        assert_eq!(
            sanitize_json_file_name("Папка/Сущность 1.json").expect("name"),
            "Сущность 1.json"
        );
        assert!(sanitize_json_file_name("bad..name.json").is_err());
        assert!(sanitize_json_file_name("entity.txt").is_err());
    }

    #[test]
    fn derives_ids_from_content_or_filename() {
        let content = serde_json::json!({"id": "entity 1", "artifact_id": "artifact/1"});
        assert_eq!(
            derive_entity_id(&content, "fallback.json", "abcdef"),
            "entity_1"
        );
        assert_eq!(
            derive_artifact_id(&content, "entity_1", "abcdef"),
            "artifact_1"
        );
        let fallback = serde_json::json!({"name": "No id"});
        assert_eq!(
            derive_entity_id(&fallback, "Файл сущности.json", "abcdef123456"),
            "Файл_сущности_abcdef123456"
        );
        assert_eq!(
            derive_artifact_id(&fallback, "entity_1", "abcdef"),
            "entity_1__source"
        );
    }
}

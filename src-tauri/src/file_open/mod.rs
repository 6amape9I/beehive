use std::path::{Path, PathBuf};

use crate::database::{find_entity_file_by_id, open_connection};
use crate::file_ops::canonical_registered_file_path;
use crate::workdir::path_string;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenEntityPathKind {
    File,
    Folder,
}

pub fn resolve_entity_open_path(
    workdir_path: &Path,
    database_path: &Path,
    entity_file_id: i64,
    kind: OpenEntityPathKind,
) -> Result<PathBuf, String> {
    let connection = open_connection(database_path)?;
    let file = find_entity_file_by_id(&connection, entity_file_id)?
        .ok_or_else(|| format!("Entity file id '{entity_file_id}' was not found."))?;
    let file_path = canonical_registered_file_path(workdir_path, &file.file_path, false)?;

    match kind {
        OpenEntityPathKind::File => {
            if !file_path.exists() {
                return Err(format!(
                    "Registered file '{}' does not exist and cannot be opened.",
                    file.file_path
                ));
            }
            Ok(file_path)
        }
        OpenEntityPathKind::Folder => {
            let Some(parent) = file_path.parent() else {
                return Err(format!(
                    "Registered file '{}' does not have a parent folder.",
                    file.file_path
                ));
            };
            if !parent.exists() {
                return Err(format!(
                    "Registered file folder '{}' does not exist.",
                    parent.display()
                ));
            }
            parent.canonicalize().map_err(|error| {
                format!(
                    "Failed to canonicalize registered file folder '{}': {error}",
                    parent.display()
                )
            })
        }
    }
}

pub fn open_entity_path(
    workdir_path: &Path,
    database_path: &Path,
    entity_file_id: i64,
    kind: OpenEntityPathKind,
) -> Result<String, String> {
    let target = resolve_entity_open_path(workdir_path, database_path, entity_file_id, kind)?;
    opener::open(&target)
        .map_err(|error| format!("Failed to open '{}': {error}", target.display()))?;
    Ok(path_string(&target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{bootstrap_database, list_entity_files};
    use crate::discovery::scan_workspace;
    use crate::domain::{PipelineConfig, ProjectConfig, RuntimeConfig, StageDefinition};
    use std::fs;

    fn config() -> PipelineConfig {
        PipelineConfig {
            project: ProjectConfig {
                name: "beehive".to_string(),
                workdir: ".".to_string(),
            },
            storage: None,
            runtime: RuntimeConfig::default(),
            stages: vec![StageDefinition {
                id: "incoming".to_string(),
                input_folder: "stages/incoming".to_string(),
                input_uri: None,
                output_folder: "stages/out".to_string(),
                workflow_url: "http://localhost/webhook".to_string(),
                max_attempts: 3,
                retry_delay_sec: 10,
                next_stage: None,
                save_path_aliases: Vec::new(),
            }],
        }
    }

    #[test]
    fn resolves_registered_file_and_folder_paths() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        let file_path = workdir.join("stages/incoming/entity-1.json");
        fs::create_dir_all(file_path.parent().expect("parent")).expect("parent");
        fs::write(&file_path, r#"{"id":"entity-1","payload":{"ok":true}}"#).expect("file");

        bootstrap_database(&database_path, &config()).expect("bootstrap");
        scan_workspace(&workdir, &database_path).expect("scan");
        let file = list_entity_files(&database_path, Some("entity-1")).expect("files")[0].clone();

        let resolved_file =
            resolve_entity_open_path(&workdir, &database_path, file.id, OpenEntityPathKind::File)
                .expect("resolve file");
        let resolved_folder = resolve_entity_open_path(
            &workdir,
            &database_path,
            file.id,
            OpenEntityPathKind::Folder,
        )
        .expect("resolve folder");

        assert_eq!(
            resolved_file,
            file_path.canonicalize().expect("canonical file")
        );
        assert_eq!(
            resolved_folder,
            file_path
                .parent()
                .expect("parent")
                .canonicalize()
                .expect("canonical parent")
        );
    }

    #[test]
    fn rejects_unknown_file_id() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let workdir = tempdir.path().join("workdir");
        let database_path = workdir.join("app.db");
        fs::create_dir_all(&workdir).expect("workdir");
        bootstrap_database(&database_path, &config()).expect("bootstrap");

        let error =
            resolve_entity_open_path(&workdir, &database_path, 999, OpenEntityPathKind::File)
                .expect_err("unknown file should fail");

        assert!(error.contains("was not found"));
    }
}

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub(crate) struct StableFileRead {
    pub bytes: Vec<u8>,
    pub file_size: u64,
    pub file_mtime: String,
}

#[derive(Debug, Clone)]
pub(crate) struct FileStabilityIssue {
    pub code: &'static str,
    pub message: String,
}

pub(crate) fn read_stable_file(
    path: &Path,
    stability_delay_ms: u64,
) -> Result<StableFileRead, FileStabilityIssue> {
    let before = fs::metadata(path).map_err(|error| FileStabilityIssue {
        code: "file_metadata_unavailable",
        message: format!(
            "Failed to read file metadata for '{}': {error}",
            path.display()
        ),
    })?;
    let before_modified = before.modified().map_err(|error| FileStabilityIssue {
        code: "file_metadata_unavailable",
        message: format!(
            "Failed to read file modified time for '{}': {error}",
            path.display()
        ),
    })?;

    if stability_delay_ms > 0 && is_too_fresh(before_modified, stability_delay_ms) {
        return Err(FileStabilityIssue {
            code: "unstable_file_skipped",
            message: format!(
                "File '{}' was modified too recently and was skipped until it is stable.",
                path.display()
            ),
        });
    }

    let bytes = fs::read(path).map_err(|error| FileStabilityIssue {
        code: "file_read_failed",
        message: format!("Failed to read JSON file '{}': {error}", path.display()),
    })?;

    let after = fs::metadata(path).map_err(|error| FileStabilityIssue {
        code: "file_metadata_unavailable",
        message: format!(
            "Failed to re-read file metadata for '{}': {error}",
            path.display()
        ),
    })?;
    let after_modified = after.modified().map_err(|error| FileStabilityIssue {
        code: "file_metadata_unavailable",
        message: format!(
            "Failed to re-read file modified time for '{}': {error}",
            path.display()
        ),
    })?;

    if before.len() != after.len() || before_modified != after_modified {
        return Err(FileStabilityIssue {
            code: "unstable_file_skipped",
            message: format!(
                "File '{}' changed while it was being read and was skipped.",
                path.display()
            ),
        });
    }

    Ok(StableFileRead {
        bytes,
        file_size: after.len(),
        file_mtime: system_time_to_rfc3339(after_modified),
    })
}

fn is_too_fresh(modified: SystemTime, stability_delay_ms: u64) -> bool {
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return true;
    };
    elapsed < Duration::from_millis(stability_delay_ms)
}

fn system_time_to_rfc3339(value: SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339()
}

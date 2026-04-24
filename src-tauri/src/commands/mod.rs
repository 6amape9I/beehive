use crate::bootstrap;
use crate::domain::BootstrapResult;

#[tauri::command]
pub fn initialize_workdir(path: String) -> BootstrapResult {
    bootstrap::initialize_workdir(&path)
}

#[tauri::command]
pub fn open_workdir(path: String) -> BootstrapResult {
    bootstrap::open_workdir(&path)
}

#[tauri::command]
pub fn reload_workdir(path: String) -> BootstrapResult {
    bootstrap::reload_workdir(&path)
}

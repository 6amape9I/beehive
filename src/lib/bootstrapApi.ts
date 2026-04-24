import { invoke } from "@tauri-apps/api/core";

import type { BootstrapResult } from "../types/domain";

export async function initializeWorkdir(path: string): Promise<BootstrapResult> {
  return invoke<BootstrapResult>("initialize_workdir", { path });
}

export async function openWorkdir(path: string): Promise<BootstrapResult> {
  return invoke<BootstrapResult>("open_workdir", { path });
}

export async function reloadWorkdir(path: string): Promise<BootstrapResult> {
  return invoke<BootstrapResult>("reload_workdir", { path });
}

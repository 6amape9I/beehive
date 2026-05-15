import { createHttpClient } from "./httpClient";
import { tauriClient } from "./tauriClient";
import type { BeehiveApiClient } from "./types";

const viteEnv = (import.meta as ImportMeta & {
  env?: Record<string, string | undefined>;
}).env;
const configuredApiBaseUrl = viteEnv?.VITE_BEEHIVE_API_BASE_URL;
const apiBaseUrl = resolveBrowserApiBaseUrl(configuredApiBaseUrl);

export const apiClient: BeehiveApiClient = apiBaseUrl
  ? createHttpClient(apiBaseUrl)
  : tauriClient;

export const isHttpApiMode = Boolean(apiBaseUrl);

function resolveBrowserApiBaseUrl(configuredUrl?: string): string | null {
  if (typeof window === "undefined") {
    return configuredUrl?.trim() || null;
  }

  const currentOrigin = window.location.origin;
  const currentHostname = window.location.hostname;
  const configured = configuredUrl?.trim();

  if (!configured) {
    return isTauriRuntime() ? null : currentOrigin;
  }

  if (isLoopbackApiBase(configured) && !isLoopbackHost(currentHostname)) {
    return currentOrigin;
  }

  return configured;
}

function isTauriRuntime(): boolean {
  const maybeWindow = window as Window & {
    __TAURI_INTERNALS__?: unknown;
    __TAURI__?: unknown;
  };
  return Boolean(maybeWindow.__TAURI_INTERNALS__ || maybeWindow.__TAURI__);
}

function isLoopbackApiBase(value: string): boolean {
  try {
    return isLoopbackHost(new URL(value).hostname);
  } catch {
    return false;
  }
}

function isLoopbackHost(hostname: string): boolean {
  return (
    hostname === "127.0.0.1" ||
    hostname === "localhost" ||
    hostname === "::1" ||
    hostname === "[::1]"
  );
}

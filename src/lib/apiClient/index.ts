import { createHttpClient } from "./httpClient";
import { tauriClient } from "./tauriClient";
import type { BeehiveApiClient } from "./types";

const viteEnv = (import.meta as ImportMeta & {
  env?: Record<string, string | undefined>;
}).env;
const apiBaseUrl = viteEnv?.VITE_BEEHIVE_API_BASE_URL;

export const apiClient: BeehiveApiClient = apiBaseUrl
  ? createHttpClient(apiBaseUrl)
  : tauriClient;

export const isHttpApiMode = Boolean(apiBaseUrl);

import { invoke } from "@tauri-apps/api/core"

import type { ApiKeyStatus, AppBootstrap, Settings } from "@/platform/app-contract"

export async function readAppBootstrap(signal: AbortSignal): Promise<AppBootstrap> {
  const bootstrap = await invoke<AppBootstrap>("get_app_bootstrap")
  signal.throwIfAborted()
  return bootstrap
}

export async function writeSettings(settings: Settings): Promise<AppBootstrap> {
  return invoke<AppBootstrap>("save_settings", { settings })
}

export async function writeApiKey(providerId: string, key: string): Promise<ApiKeyStatus> {
  return invoke<ApiKeyStatus>("save_api_key", { providerId, key })
}

export async function removeApiKey(providerId: string): Promise<ApiKeyStatus> {
  return invoke<ApiKeyStatus>("delete_api_key", { providerId })
}

export async function reportUiError(kind: string): Promise<void> {
  return invoke<void>("report_ui_error", { kind })
}

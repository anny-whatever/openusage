import { invoke } from "@tauri-apps/api/core"

export type ScaffoldStatus = {
  platform: "windows"
  architecture: string
  trustedBackend: boolean
}

export async function readScaffoldStatus(signal: AbortSignal): Promise<ScaffoldStatus> {
  const status = await invoke<ScaffoldStatus>("get_scaffold_status")
  signal.throwIfAborted()
  return status
}


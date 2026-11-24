// The small part of Tauri's global API (withGlobalTauri) that Central uses. Present only inside the app.
interface TauriGlobal {
  core: { invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> };
  app?: { getVersion(): Promise<string> };
}

interface Window {
  __TAURI__?: TauriGlobal;
}

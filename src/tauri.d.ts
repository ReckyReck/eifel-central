// The small part of Tauri's global API (withGlobalTauri) that Central uses. Present only inside the app.
interface TauriEvent<T> {
  payload: T;
}

interface TauriGlobal {
  core: { invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> };
  app?: { getVersion(): Promise<string> };
  event?: { listen<T>(event: string, handler: (e: TauriEvent<T>) => void): Promise<() => void> };
}

interface Window {
  __TAURI__?: TauriGlobal;
}

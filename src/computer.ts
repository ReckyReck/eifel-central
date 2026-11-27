// Small questions to the app about this computer (Rust side: src-tauri/src/system.rs).

/** The system for the bug form, asked once from the app. */
export async function systemInfo(): Promise<string | null> {
  try {
    const s = await window.__TAURI__?.core.invoke<string>("system_info");
    return s && s.trim() ? s.trim() : null;
  } catch {
    return null;
  }
}

/** Which DAWs are open right now; null outside the app or if it cannot be found out. */
export async function runningDaws(): Promise<string[] | null> {
  const invoke = window.__TAURI__?.core.invoke;
  if (!invoke) return null;
  try {
    return await invoke<string[]>("running_daws");
  } catch {
    return null;
  }
}

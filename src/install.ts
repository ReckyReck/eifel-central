// Installing a plug-in: builds the request for the app's Rust side ("install"), follows its progress events and
// turns them into one "job" the window can show (LED chain + text). Only one install runs at a time.

import type { Format, Plugin } from "./catalog.js";
import type { Settings } from "./state.js";
import { PLATFORM } from "./state.js";

export type Phase = "download" | "verify" | "unpack" | "password" | "copy" | "done" | "error";

export interface Job {
  id: string;
  phase: Phase;
  /** 0..1 for the LED chain */
  fraction: number;
  /** download: bytes so far / total */
  done: number;
  total: number;
  message?: string;
}

interface ProgressEvent {
  id: string;
  phase: Exclude<Phase, "error">;
  done: number;
  total: number;
}

export interface InstallResult {
  id: string;
  version: string;
  formats: Format[];
  location: "system" | "user";
  paths: string[];
}

export type InstallOutcome =
  | { kind: "ok"; result: InstallResult }
  | { kind: "cancelled" }
  | { kind: "busy" }
  | { kind: "failed"; message: string };

/** Formats that will be installed: the ones switched on in Settings that this download contains. */
export function formatsToInstall(plugin: Plugin, settings: Settings): Format[] {
  const asset = plugin.latest.assets.find((a) => a.platform === PLATFORM);
  const offered = plugin.formats[PLATFORM] ?? [];
  if (!asset) return [];
  return settings.formats.filter((f) => asset.contains.includes(f) && offered.includes(f) && plugin.bundleNames[f]);
}

export function fractionOf(phase: Phase, done: number, total: number): number {
  switch (phase) {
    case "download":
      return 0.04 + 0.76 * (total > 0 ? Math.min(1, done / total) : 0);
    case "verify":
      return 0.83;
    case "unpack":
      return 0.88;
    case "password":
      return 0.91;
    case "copy":
      return done >= total && total > 0 ? 0.97 : 0.94;
    case "done":
      return 1;
    case "error":
      return 1;
  }
}

/** Starts the install. `onProgress` is called for every progress event of this plug-in. */
export async function install(plugin: Plugin, settings: Settings, onProgress: (j: Job) => void): Promise<InstallOutcome> {
  const tauri = window.__TAURI__;
  if (!tauri) return { kind: "failed", message: "Installing works only in the app." };
  const asset = plugin.latest.assets.find((a) => a.platform === PLATFORM);
  const formats = formatsToInstall(plugin, settings);
  if (!asset || !formats.length) return { kind: "failed", message: "Nothing to install with the formats chosen in Settings." };

  let unlisten: (() => void) | undefined;
  try {
    unlisten = await tauri.event?.listen<ProgressEvent>("install-progress", (e) => {
      const p = e.payload;
      if (p.id !== plugin.id) return;
      onProgress({ id: p.id, phase: p.phase, done: p.done, total: p.total, fraction: fractionOf(p.phase, p.done, p.total) });
    });
  } catch {
    /* no progress events: the install still works, the chain just jumps */
  }
  try {
    const result = await tauri.core.invoke<InstallResult>("install", {
      request: {
        pluginId: plugin.id,
        pluginName: plugin.name,
        version: plugin.latest.version,
        url: asset.url,
        sha256: asset.sha256,
        size: asset.size,
        bundleNames: plugin.bundleNames,
        bundleId: plugin.bundleId ?? null,
        formats,
        location: settings.location,
      },
    });
    return { kind: "ok", result };
  } catch (e) {
    const err = e as { kind?: string; message?: string } | string;
    if (typeof err === "object" && err !== null) {
      if (err.kind === "cancelled") return { kind: "cancelled" };
      if (err.kind === "busy") return { kind: "busy" };
      if (err.kind === "failed" && err.message) return { kind: "failed", message: err.message };
    }
    return { kind: "failed", message: typeof err === "string" ? err : "Unknown error." };
  } finally {
    unlisten?.();
  }
}

export async function cancelInstall(): Promise<void> {
  try {
    await window.__TAURI__?.core.invoke("cancel_install");
  } catch {
    /* ignore */
  }
}

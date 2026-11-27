// Installing, updating and removing plug-ins: builds the requests for the app's Rust side ("install_many",
// "remove_plugins"), follows the progress events and turns them into one "job" per plug-in that the window shows
// (LED chain + text). Only one job (install, update all, remove) runs at a time.

import type { Format, Plugin } from "./catalog.js";
import type { InstalledEntry, PluginView, Settings } from "./state.js";
import { PLATFORM } from "./state.js";

export type Phase = "queued" | "download" | "verify" | "unpack" | "password" | "copy" | "remove" | "done" | "error";

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
  phase: Exclude<Phase, "error" | "queued" | "remove">;
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
  | { kind: "ok"; installed: InstallResult[]; failed: Array<{ id: string; message: string }> }
  | { kind: "cancelled" }
  | { kind: "busy" }
  | { kind: "failed"; message: string };

export type RemoveOutcome =
  | { kind: "ok"; removed: string[]; paths: string[] }
  | { kind: "cancelled" }
  | { kind: "busy" }
  | { kind: "failed"; message: string };

/** Formats this download can install at all (offered for this platform, in the ZIP, bundle named). */
function installable(plugin: Plugin): Format[] {
  const asset = plugin.latest.assets.find((a) => a.platform === PLATFORM);
  const offered = plugin.formats[PLATFORM] ?? [];
  if (!asset) return [];
  return offered.filter((f) => asset.contains.includes(f) && !!plugin.bundleNames[f]);
}

/** Formats that will be installed: the ones switched on in Settings that this download contains. */
export function formatsToInstall(plugin: Plugin, settings: Settings): Format[] {
  const can = installable(plugin);
  return settings.formats.filter((f) => can.includes(f));
}

/**
 * Formats and place for one plug-in. `keepPlace` (UPDATE, REINSTALL, "Update all"): where the installed copy is,
 * with the formats it has - so an update never leaves an old copy behind. Otherwise (INSTALL, REPLACE): Settings.
 */
export function targetOf(plugin: Plugin, settings: Settings, installed: InstalledEntry | undefined, keepPlace: boolean): { formats: Format[]; location: Settings["location"] } {
  if (keepPlace && installed) {
    const can = installable(plugin);
    const kept = installed.formats.filter((f) => can.includes(f));
    return { formats: kept.length ? kept : formatsToInstall(plugin, settings), location: installed.location ?? settings.location };
  }
  return { formats: formatsToInstall(plugin, settings), location: settings.location };
}

export function fractionOf(phase: Phase, done: number, total: number): number {
  switch (phase) {
    case "queued":
      return 0;
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
    case "remove":
      return 0.5;
    case "done":
      return 1;
    case "error":
      return 1;
  }
}

function errorOf(e: unknown): { kind: "cancelled" } | { kind: "busy" } | { kind: "failed"; message: string } {
  const err = e as { kind?: string; message?: string } | string;
  if (typeof err === "object" && err !== null) {
    if (err.kind === "cancelled") return { kind: "cancelled" };
    if (err.kind === "busy") return { kind: "busy" };
    if (err.kind === "failed" && err.message) return { kind: "failed", message: err.message };
  }
  return { kind: "failed", message: typeof err === "string" ? err : "Unknown error." };
}

/**
 * Installs one or more plug-ins in one job: downloads one after the other, then ONE password / UAC prompt for all.
 * `onProgress` is called for every progress event of these plug-ins.
 */
export async function installMany(
  items: Array<{ view: PluginView; keepPlace: boolean }>,
  settings: Settings,
  onProgress: (j: Job) => void,
): Promise<InstallOutcome> {
  const tauri = window.__TAURI__;
  if (!tauri) return { kind: "failed", message: "Installing works only in the app." };
  const requests = [];
  for (const { view, keepPlace } of items) {
    const p = view.plugin;
    const asset = p.latest.assets.find((a) => a.platform === PLATFORM);
    const { formats, location } = targetOf(p, settings, view.installed, keepPlace);
    if (!asset || !formats.length) continue;
    requests.push({
      pluginId: p.id,
      pluginName: p.name,
      version: p.latest.version,
      url: asset.url,
      sha256: asset.sha256,
      size: asset.size,
      bundleNames: p.bundleNames,
      bundleId: p.bundleId ?? null,
      formats,
      location,
    });
  }
  if (!requests.length) return { kind: "failed", message: "Nothing to install with the formats chosen in Settings." };
  const ids = new Set(requests.map((r) => r.pluginId));

  let unlisten: (() => void) | undefined;
  try {
    unlisten = await tauri.event?.listen<ProgressEvent>("install-progress", (e) => {
      const p = e.payload;
      if (!ids.has(p.id)) return;
      onProgress({ id: p.id, phase: p.phase, done: p.done, total: p.total, fraction: fractionOf(p.phase, p.done, p.total) });
    });
  } catch {
    /* no progress events: the install still works, the chain just jumps */
  }
  try {
    const r = await tauri.core.invoke<{ installed: InstallResult[]; failed: Array<{ id: string; message: string }> }>("install_many", { requests });
    return { kind: "ok", installed: r.installed, failed: r.failed };
  } catch (e) {
    return errorOf(e);
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

/** Removes every copy of these plug-ins (both plug-in folders), ONE password / UAC prompt for all. */
export async function removePlugins(views: PluginView[]): Promise<RemoveOutcome> {
  const tauri = window.__TAURI__;
  if (!tauri) return { kind: "failed", message: "Removing works only in the app." };
  try {
    const r = await tauri.core.invoke<{ removed: string[]; paths: string[] }>("remove_plugins", {
      plugins: views.map((v) => ({ id: v.plugin.id, bundleNames: v.plugin.bundleNames, bundleId: v.plugin.bundleId ?? null, name: v.plugin.name })),
    });
    return { kind: "ok", removed: r.removed, paths: r.paths };
  } catch (e) {
    return errorOf(e);
  }
}

// Settings, "already seen" plug-ins, and what is installed.

import type { Format, Platform, Plugin } from "./catalog.js";
import { compareVersions } from "./catalog.js";

export const PLATFORM: Platform = /Windows/i.test(navigator.userAgent) ? "windows" : "macos";

export interface Settings {
  formats: Format[];
  location: "system" | "user";
  checkAtStart: boolean;
}

const SETTINGS_KEY = "central.settings.v1";
const SEEN_KEY = "central.seenPlugins.v1";

export const FORMATS_FOR_PLATFORM: Record<Platform, Format[]> = { macos: ["au", "vst3"], windows: ["vst3"] };

function defaults(): Settings {
  return { formats: [...FORMATS_FOR_PLATFORM[PLATFORM]], location: "system", checkAtStart: true };
}

export function loadSettings(): Settings {
  const d = defaults();
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "null");
    if (typeof raw !== "object" || raw === null) return d;
    const r = raw as Partial<Settings>;
    const allowed = FORMATS_FOR_PLATFORM[PLATFORM];
    const formats = Array.isArray(r.formats) ? r.formats.filter((f) => allowed.includes(f)) : d.formats;
    return {
      formats: formats.length ? formats : d.formats,
      location: PLATFORM === "macos" && r.location === "user" ? "user" : "system",
      checkAtStart: typeof r.checkAtStart === "boolean" ? r.checkAtStart : d.checkAtStart,
    };
  } catch {
    return d;
  }
}

export function saveSettings(s: Settings): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
  } catch {
    /* ignore */
  }
}

/**
 * Plug-ins Central has seen at an earlier start. A plug-in that is in the catalog but not in this list is shown as
 * "New". Call rememberSeen() after showing the catalog, so it is "New" for exactly one start.
 */
export function loadSeen(): Set<string> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(SEEN_KEY) ?? "[]");
    return new Set(Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

export function rememberSeen(ids: Iterable<string>, previous: Set<string>): void {
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify([...new Set([...previous, ...ids])]));
  } catch {
    /* ignore */
  }
}

export interface InstalledEntry {
  version: string;
  formats: Format[];
  /** Installed by Central (listed in installed.json) or found on disk from another installer. */
  byCentral: boolean;
  /** Where the copy that counts is: system folder or the user's own. */
  location: "system" | "user" | null;
  /** Every copy found on disk. */
  paths: string[];
}

/**
 * What is installed, by plug-in id. Comes from the app's Rust side ("get_installed": reads installed.json and the
 * bundles' Info.plist in /Library/Audio/Plug-Ins and ~/Library/Audio/Plug-Ins; on Windows the VST3 moduleinfo.json in
 * C:\Program Files\Common Files\VST3 and %LOCALAPPDATA%\Programs\Common\VST3). Outside the app: nothing.
 */
export async function loadInstalled(catalog: Plugin[]): Promise<Map<string, InstalledEntry>> {
  const invoke = window.__TAURI__?.core.invoke;
  if (!invoke) return new Map();
  try {
    const list = await invoke<Array<{ id: string } & InstalledEntry>>("get_installed", {
      plugins: catalog.map((p) => ({ id: p.id, bundleNames: p.bundleNames, bundleId: p.bundleId ?? null })),
    });
    return new Map(
      list.map((e) => [
        e.id,
        { version: e.version, formats: e.formats, byCentral: e.byCentral, location: e.location ?? null, paths: e.paths ?? [] },
      ]),
    );
  } catch {
    return new Map();
  }
}

export type Status = "update" | "current" | "not-installed" | "unavailable";

export interface PluginView {
  plugin: Plugin;
  status: Status;
  isNew: boolean;
  installed?: InstalledEntry;
}

export function viewOf(plugin: Plugin, installed: Map<string, InstalledEntry>, seen: Set<string>): PluginView {
  const inst = installed.get(plugin.id);
  const hasAsset = plugin.latest.assets.some((a) => a.platform === PLATFORM);
  let status: Status;
  if (inst) status = compareVersions(inst.version, plugin.latest.version) < 0 ? "update" : "current";
  else status = hasAsset ? "not-installed" : "unavailable";
  return { plugin, status, isNew: !inst && !seen.has(plugin.id), installed: inst };
}

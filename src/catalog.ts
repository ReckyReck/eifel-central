// Loading and checking catalog.json. The app reads ONLY this fixed address (no GitHub API, no tokens).

export const CATALOG_URL = "https://reckyreck.github.io/eifel-catalog/catalog.json";
export const SUPPORTED_SCHEMA = 1;

export type Platform = "macos" | "windows";
export type Format = "au" | "vst3" | "clap";

export interface Asset {
  platform: Platform;
  arch: string;
  url: string;
  sha256: string;
  size: number;
  contains: Format[];
}

export interface Release {
  version: string;
  releasedAt: string;
  changelog: string[];
  minOS: Partial<Record<Platform, string>>;
  assets: Asset[];
}

export interface Plugin {
  id: string;
  name: string;
  tagline: string;
  category: string;
  image: string | null;
  channel: string;
  formats: Partial<Record<Platform, Format[]>>;
  bundleNames: Partial<Record<Format, string>>;
  bundleId?: string;
  latest: Release;
}

export interface Catalog {
  schemaVersion: number;
  generatedAt: string;
  plugins: Plugin[];
}

export type LoadResult =
  | { kind: "ok"; catalog: Catalog; fetchedAt: number }
  | { kind: "cached"; catalog: Catalog; fetchedAt: number; error: string }
  | { kind: "error"; error: string }
  | { kind: "schema"; schemaVersion: number; cached?: { catalog: Catalog; fetchedAt: number } };

const CACHE_KEY = "central.catalogCache.v1";
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const SHA256 = /^[0-9a-f]{64}$/;

export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d < 0 ? -1 : 1;
  }
  return 0;
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === "string";
const strArray = (v: unknown): string[] => (Array.isArray(v) ? v.filter(isStr) : []);

function parseAsset(v: unknown): Asset | null {
  if (!isObj(v)) return null;
  const { platform, arch, url, sha256, size } = v;
  if (platform !== "macos" && platform !== "windows") return null;
  if (!isStr(url) || !url.startsWith("https://")) return null; // HTTPS only
  if (!isStr(sha256) || !SHA256.test(sha256)) return null; // never offer a download we cannot verify
  if (typeof size !== "number" || size <= 0) return null;
  const contains = strArray(v.contains).filter((f): f is Format => f === "au" || f === "vst3" || f === "clap");
  return { platform, arch: isStr(arch) ? arch : "", url, sha256, size, contains };
}

function parsePlugin(v: unknown): Plugin | null {
  if (!isObj(v) || !isObj(v.latest)) return null;
  const { id, name } = v;
  const latest = v.latest;
  if (!isStr(id) || !isStr(name) || !isStr(latest.version) || !SEMVER.test(latest.version)) return null;
  const formats: Partial<Record<Platform, Format[]>> = {};
  if (isObj(v.formats)) {
    for (const p of ["macos", "windows"] as const) {
      const list = strArray(v.formats[p]).filter((f): f is Format => f === "au" || f === "vst3" || f === "clap");
      if (list.length) formats[p] = list;
    }
  }
  const bundleNames: Partial<Record<Format, string>> = {};
  if (isObj(v.bundleNames)) {
    for (const f of ["au", "vst3", "clap"] as const) {
      const n = v.bundleNames[f];
      if (isStr(n)) bundleNames[f] = n;
    }
  }
  const minOS: Partial<Record<Platform, string>> = {};
  if (isObj(latest.minOS)) {
    for (const p of ["macos", "windows"] as const) {
      const m = latest.minOS[p];
      if (isStr(m)) minOS[p] = m;
    }
  }
  const image = isStr(v.image) && v.image.startsWith("https://") ? v.image : null;
  return {
    id,
    name,
    tagline: isStr(v.tagline) ? v.tagline : "",
    category: isStr(v.category) ? v.category : "",
    image,
    channel: isStr(v.channel) ? v.channel : "stable",
    formats,
    bundleNames,
    bundleId: isStr(v.bundleId) ? v.bundleId : undefined,
    latest: {
      version: latest.version,
      releasedAt: isStr(latest.releasedAt) ? latest.releasedAt : "",
      changelog: strArray(latest.changelog),
      minOS,
      assets: (Array.isArray(latest.assets) ? latest.assets : []).map(parseAsset).filter((a): a is Asset => a !== null),
    },
  };
}

/** Turns the downloaded JSON into a Catalog. Unknown fields are ignored; broken plug-in entries are skipped. */
export function parseCatalog(raw: unknown): Catalog | { schemaVersion: number } {
  if (!isObj(raw) || typeof raw.schemaVersion !== "number") throw new Error("not a catalog");
  if (raw.schemaVersion > SUPPORTED_SCHEMA) return { schemaVersion: raw.schemaVersion };
  const plugins = (Array.isArray(raw.plugins) ? raw.plugins : [])
    .map(parsePlugin)
    .filter((p): p is Plugin => p !== null)
    .filter((p) => p.channel === "stable");
  return { schemaVersion: raw.schemaVersion, generatedAt: isStr(raw.generatedAt) ? raw.generatedAt : "", plugins };
}

const isCatalog = (c: Catalog | { schemaVersion: number }): c is Catalog => "plugins" in c;

function readCache(): { catalog: Catalog; fetchedAt: number } | undefined {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return undefined;
    const saved: unknown = JSON.parse(raw);
    if (!isObj(saved) || typeof saved.fetchedAt !== "number") return undefined;
    const c = parseCatalog(saved.catalog);
    return isCatalog(c) ? { catalog: c, fetchedAt: saved.fetchedAt } : undefined;
  } catch {
    return undefined;
  }
}

function writeCache(raw: unknown, fetchedAt: number): void {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify({ catalog: raw, fetchedAt }));
  } catch {
    /* storage full or unavailable: the app still works, just without offline copy */
  }
}

export async function loadCatalog(timeoutMs = 15000): Promise<LoadResult> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(CATALOG_URL, { cache: "no-store", signal: controller.signal });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const raw: unknown = await res.json();
    const parsed = parseCatalog(raw);
    if (!isCatalog(parsed)) return { kind: "schema", schemaVersion: parsed.schemaVersion, cached: readCache() };
    const now = Date.now();
    writeCache(raw, now);
    return { kind: "ok", catalog: parsed, fetchedAt: now };
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    const cached = readCache();
    return cached ? { kind: "cached", ...cached, error } : { kind: "error", error };
  } finally {
    clearTimeout(timer);
  }
}

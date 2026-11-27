// Eifel Mastering Central - start-up and state.

import { loadCatalog } from "./catalog.js";
import type { Plugin } from "./catalog.js";
import { t } from "./i18n/index.js";
import { cancelInstall, formatsToInstall, install } from "./install.js";
import type { PluginView } from "./state.js";
import { PLATFORM, loadInstalled, loadSeen, loadSettings, rememberSeen, saveSettings, viewOf } from "./state.js";
import type { Actions, ViewState } from "./view.js";
import { render, toast } from "./view.js";

const root = document.getElementById("app");
if (!root) throw new Error("#app missing");

const state: ViewState = {
  load: null,
  views: [],
  filter: "all",
  search: "",
  settings: loadSettings(),
  settingsOpen: false,
  appVersion: "0.3.0",
  now: Date.now(),
  job: null,
  confirm: null,
};

let plugins: Plugin[] = [];

const seenAtStart = loadSeen();

const actions: Actions = {
  setFilter(f) {
    state.filter = f;
    update();
  },
  setSearch(s) {
    state.search = s;
    update();
  },
  openSettings(open) {
    state.settingsOpen = open;
    update();
  },
  changeSettings(s) {
    state.settings = s;
    saveSettings(s);
    update();
  },
  retry() {
    void refresh();
  },
  notYet() {
    toast(t("toast.notYet"));
  },
  install(v) {
    if (state.job && !["done", "error"].includes(state.job.phase)) return toast(t("toast.busy"));
    if (!formatsToInstall(v.plugin, state.settings).length) return toast(t("toast.noFormats"));
    // Safeguard: a copy that Central did not install is only replaced after asking.
    if (v.installed && !v.installed.byCentral) {
      state.confirm = v;
      return update();
    }
    void runInstall(v);
  },
  cancelInstall() {
    void cancelInstall();
  },
  answerReplace(ok) {
    const v = state.confirm;
    state.confirm = null;
    update();
    if (ok && v) void runInstall(v);
  },
};

async function runInstall(v: PluginView): Promise<void> {
  const p = v.plugin;
  state.job = { id: p.id, phase: "download", fraction: 0.04, done: 0, total: p.latest.assets.find((a) => a.platform === PLATFORM)?.size ?? 0 };
  update();
  const outcome = await install(p, state.settings, (j) => {
    state.job = j;
    update();
  });
  if (outcome.kind === "ok") {
    state.job = { id: p.id, phase: "done", fraction: 1, done: 1, total: 1 };
    const formats = outcome.result.formats.map((f) => f.toUpperCase()).join(", ");
    toast(t("toast.installed", { name: p.name, version: outcome.result.version, formats }), "green", 8000);
  } else if (outcome.kind === "cancelled") {
    state.job = null;
    toast(t("toast.cancelled"));
  } else if (outcome.kind === "busy") {
    state.job = null;
    toast(t("toast.busy"));
  } else {
    state.job = { id: p.id, phase: "error", fraction: 1, done: 0, total: 0, message: outcome.message };
    toast(t("toast.failed", { name: p.name, message: outcome.message }), "red", 15000);
  }
  await reloadInstalled();
}

async function reloadInstalled(): Promise<void> {
  const installed = await loadInstalled(plugins);
  state.views = plugins.map((p) => viewOf(p, installed, seenAtStart));
  update();
}

function update(): void {
  state.now = Date.now();
  render(root!, state, actions);
}

async function refresh(): Promise<void> {
  state.load = null;
  update();
  const load = await loadCatalog();
  state.load = load;
  const catalog = load.kind === "ok" || load.kind === "cached" ? load.catalog : load.kind === "schema" ? load.cached?.catalog : undefined;
  plugins = catalog?.plugins ?? [];
  const installed = await loadInstalled(plugins);
  state.views = plugins.map((p) => viewOf(p, installed, seenAtStart));
  if (load.kind === "ok") rememberSeen(plugins.map((p) => p.id), seenAtStart); // "New" only for this start
  update();
}

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && state.settingsOpen) actions.openSettings(false);
  else if (e.key === "Escape" && state.confirm) actions.answerReplace(false);
});
setInterval(update, 60000); // keeps "checked 3 min ago" current

void (async () => {
  try {
    const v = await window.__TAURI__?.app?.getVersion();
    if (v) state.appVersion = v;
  } catch {
    /* outside the app (preview): keep the default */
  }
  await refresh();
})();

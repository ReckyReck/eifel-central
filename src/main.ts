// Eifel Mastering Central - start-up and state.

import { loadCatalog } from "./catalog.js";
import type { Plugin } from "./catalog.js";
import { runningDaws, systemInfo } from "./computer.js";
import { feedbackUrl, openFeedback } from "./feedback.js";
import { t } from "./i18n/index.js";
import type { Job } from "./install.js";
import { cancelInstall, installMany, removePlugins, targetOf } from "./install.js";
import type { PluginView } from "./state.js";
import { loadInstalled, loadSeen, loadSettings, rememberSeen, saveSettings, viewOf } from "./state.js";
import type { Actions, ViewState } from "./view.js";
import { anyRunning, render, toast } from "./view.js";

const root = document.getElementById("app");
if (!root) throw new Error("#app missing");

const state: ViewState = {
  load: null,
  views: [],
  filter: "all",
  search: "",
  settings: loadSettings(),
  settingsOpen: false,
  appVersion: "0.4.0",
  now: Date.now(),
  jobs: {},
  dialog: null,
  menu: null,
  daws: null,
};

let plugins: Plugin[] = [];
/** "macOS 26.1 (Apple Silicon)" for bug reports, asked once */
let systemText: string | null = null;

const seenAtStart = loadSeen();

/** false (and a message) if a job is still running */
function free(): boolean {
  if (anyRunning(state.jobs)) {
    toast(t("toast.busy"));
    return false;
  }
  return true;
}

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
    state.menu = null;
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
  install(v) {
    if (!free()) return;
    if (!targetOf(v.plugin, state.settings, v.installed, false).formats.length) return toast(t("toast.noFormats"));
    // Safeguard: a copy that Central did not install is only replaced after asking.
    if (v.installed && !v.installed.byCentral) return openDialog({ kind: "replace", view: v });
    void runInstall([{ view: v, keepPlace: false }]);
  },
  update(v) {
    if (free()) openDialog({ kind: "update", views: [v], mode: "update" });
  },
  updateAll() {
    const views = state.views.filter((v) => v.status === "update");
    if (views.length && free()) openDialog({ kind: "update", views, mode: "update" });
  },
  reinstall(v) {
    if (free()) openDialog({ kind: "update", views: [v], mode: "reinstall" });
  },
  whatsNew(v) {
    openDialog({ kind: "whatsNew", view: v });
  },
  remove(v) {
    if (free() && v.installed) openDialog({ kind: "remove", views: [v], all: false });
  },
  uninstallAll() {
    const views = state.views.filter((v) => v.installed);
    if (!views.length || !free()) return;
    state.settingsOpen = false;
    openDialog({ kind: "remove", views, all: true });
  },
  answer(ok) {
    const d = state.dialog;
    state.dialog = null;
    update();
    if (!ok || !d) return;
    if (d.kind === "replace") void runInstall([{ view: d.view, keepPlace: false }]);
    else if (d.kind === "update") void runInstall(d.views.map((view) => ({ view, keepPlace: true })));
    else if (d.kind === "whatsNew") actions.update(d.view);
    else if (d.kind === "remove") void runRemove(d.views);
  },
  cancelInstall() {
    void cancelInstall();
  },
  toggleMenu(id) {
    if (state.menu === id) return;
    state.menu = id;
    update();
  },
  feedback(kind, v) {
    const target = v
      ? { name: v.plugin.name, version: v.installed?.version ?? v.plugin.latest.version, label: `plugin: ${v.plugin.id}` }
      : { name: "Eifel Mastering Central", version: state.appVersion, label: "app: central" };
    // a version Central could not read ("?") is better left for the tester to fill in
    if (!/^\d+\.\d+\.\d+$/.test(target.version)) target.version = v?.plugin.latest.version ?? state.appVersion;
    void openFeedback(feedbackUrl(kind, target, systemText)).then((r) =>
      r.ok ? toast(t("toast.feedbackOpened"), "green", 6000) : toast(t("toast.feedbackFailed", { message: r.message }), "red", 8000),
    );
  },
};

function openDialog(d: NonNullable<ViewState["dialog"]>): void {
  state.dialog = d;
  state.menu = null;
  update();
  void checkDaws(); // the dialog's DAW warning should be current
}

/** Installs / updates the given plug-ins in one job (one password prompt for all). */
async function runInstall(items: Array<{ view: PluginView; keepPlace: boolean }>): Promise<void> {
  if (!free()) return;
  state.jobs = {};
  for (const { view } of items) state.jobs[view.plugin.id] = { id: view.plugin.id, phase: "queued", fraction: 0, done: 0, total: 0 };
  update();
  const outcome = await installMany(items, state.settings, (j: Job) => {
    state.jobs[j.id] = j;
    update();
  });
  const nameOf = (id: string) => items.find((i) => i.view.plugin.id === id)?.view.plugin.name ?? id;
  if (outcome.kind === "ok") {
    for (const r of outcome.installed) state.jobs[r.id] = { id: r.id, phase: "done", fraction: 1, done: 1, total: 1 };
    for (const f of outcome.failed) state.jobs[f.id] = { id: f.id, phase: "error", fraction: 1, done: 0, total: 0, message: f.message };
    // plug-ins that were left out (no format to install) do not keep a "Waiting" chain
    for (const id of Object.keys(state.jobs)) if (state.jobs[id]!.phase === "queued") delete state.jobs[id];
    const ok = outcome.installed;
    const bad = outcome.failed;
    if (items.length === 1 && ok[0]) {
      const formats = ok[0].formats.map((f) => f.toUpperCase()).join(", ");
      toast(t("toast.installed", { name: nameOf(ok[0].id), version: ok[0].version, formats }), "green", 8000);
    } else if (items.length === 1 && bad[0]) {
      toast(t("toast.failed", { name: nameOf(bad[0].id), message: bad[0].message }), "red", 15000);
    } else if (!bad.length) {
      toast(t("toast.updatedMany", { n: ok.length }), "green", 8000);
    } else {
      toast(t("toast.someFailed", { ok: ok.length, n: bad.length, names: bad.map((f) => nameOf(f.id)).join(", ") }), "red", 15000);
    }
  } else if (outcome.kind === "cancelled") {
    state.jobs = {};
    toast(t("toast.cancelled"));
  } else if (outcome.kind === "busy") {
    state.jobs = {};
    toast(t("toast.busy"));
  } else {
    state.jobs = {};
    const first = items[0]!.view.plugin;
    for (const { view } of items) state.jobs[view.plugin.id] = { id: view.plugin.id, phase: "error", fraction: 1, done: 0, total: 0, message: outcome.message };
    toast(t("toast.failed", { name: items.length === 1 ? first.name : "Central", message: outcome.message }), "red", 15000);
  }
  await reloadInstalled();
}

/** Removes the given plug-ins (every copy found), one password prompt for all. */
async function runRemove(views: PluginView[]): Promise<void> {
  if (!free()) return;
  state.jobs = {};
  for (const v of views) state.jobs[v.plugin.id] = { id: v.plugin.id, phase: "remove", fraction: 0.5, done: 0, total: 0 };
  update();
  const outcome = await removePlugins(views);
  state.jobs = {};
  if (outcome.kind === "ok") {
    const one = views.length === 1 ? views[0] : undefined;
    toast(one ? t("toast.removed", { name: one.plugin.name }) : t("toast.removedMany", { n: outcome.removed.length }), "green", 8000);
  } else if (outcome.kind === "cancelled") toast(t("toast.cancelled"));
  else if (outcome.kind === "busy") toast(t("toast.busy"));
  else toast(t("toast.removeFailed", { message: outcome.message }), "red", 15000);
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

/** Which DAWs are open (footer LED, warnings in the dialogs). Redraws only when that changed. */
async function checkDaws(): Promise<void> {
  const daws = await runningDaws();
  if (JSON.stringify(daws) === JSON.stringify(state.daws)) return;
  state.daws = daws;
  update();
}

document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (state.menu) actions.toggleMenu(null);
  else if (state.settingsOpen) actions.openSettings(false);
  else if (state.dialog) actions.answer(false);
});
// a click anywhere outside the open menu closes it
document.addEventListener("click", (e) => {
  if (!state.menu) return;
  const wrap = (e.target as Element | null)?.closest?.(".menu-wrap");
  if (!wrap || wrap.getAttribute("data-menu") !== state.menu) actions.toggleMenu(null);
});
setInterval(update, 60000); // keeps "checked 3 min ago" current
if (window.__TAURI__) setInterval(() => void checkDaws(), 5000);

void (async () => {
  try {
    const v = await window.__TAURI__?.app?.getVersion();
    if (v) state.appVersion = v;
  } catch {
    /* outside the app (preview): keep the default */
  }
  void systemInfo().then((s) => (systemText = s));
  void checkDaws();
  await refresh();
})();


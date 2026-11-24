// Eifel Mastering Central - start-up and state.

import { loadCatalog } from "./catalog.js";
import { t } from "./i18n/index.js";
import { loadInstalled, loadSeen, loadSettings, rememberSeen, saveSettings, viewOf } from "./state.js";
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
  appVersion: "0.1.0",
  now: Date.now(),
};

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
};

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
  const plugins = catalog?.plugins ?? [];
  const installed = await loadInstalled(plugins);
  state.views = plugins.map((p) => viewOf(p, installed, seenAtStart));
  if (load.kind === "ok") rememberSeen(plugins.map((p) => p.id), seenAtStart); // "New" only for this start
  update();
}

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && state.settingsOpen) actions.openSettings(false);
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

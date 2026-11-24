// Builds the main window: header, filter bar, rack units, footer, settings panel.

import type { LoadResult } from "./catalog.js";
import { CATALOG_URL } from "./catalog.js";
import { display, h, led, screws, sectionLine } from "./dom.js";
import { t } from "./i18n/index.js";
import type { PluginView, Settings } from "./state.js";
import { FORMATS_FOR_PLATFORM, PLATFORM } from "./state.js";

export type Filter = "all" | "installed" | "updates" | "new";

export interface ViewState {
  load: LoadResult | null;
  views: PluginView[];
  filter: Filter;
  search: string;
  settings: Settings;
  settingsOpen: boolean;
  appVersion: string;
  now: number;
}

export interface Actions {
  setFilter(f: Filter): void;
  setSearch(s: string): void;
  openSettings(open: boolean): void;
  changeSettings(s: Settings): void;
  retry(): void;
  notYet(): void;
}

// ---- helpers ------------------------------------------------------------------------------------------------------
function counts(views: PluginView[]): Record<Filter, number> {
  return {
    all: views.length,
    installed: views.filter((v) => v.installed).length,
    updates: views.filter((v) => v.status === "update").length,
    new: views.filter((v) => v.isNew).length,
  };
}

function matches(v: PluginView, filter: Filter, search: string): boolean {
  if (filter === "installed" && !v.installed) return false;
  if (filter === "updates" && v.status !== "update") return false;
  if (filter === "new" && !v.isNew) return false;
  const q = search.trim().toLowerCase();
  if (!q) return true;
  const p = v.plugin;
  return [p.name, p.tagline, p.category].some((s) => s.toLowerCase().includes(q));
}

function when(ts: number, now: number): string {
  const min = Math.floor((now - ts) / 60000);
  if (min < 1) return t("when.justNow");
  if (min < 60) return t("when.minutes", { n: min });
  if (min < 24 * 60) return t("when.hours", { n: Math.floor(min / 60) });
  return t("when.date", { date: new Date(ts).toLocaleDateString("en-GB") });
}

/** "Recky's Velvet" -> light "Recky's " + bold "VELVET", like the plug-in panels. */
function pluginTitle(name: string): HTMLElement {
  const m = /^(Recky['’]s)\s+(.+)$/.exec(name);
  return m && m[1] && m[2]
    ? h("h2", { class: "unit-name" }, h("span", { class: "light" }, `${m[1]} `), h("b", {}, m[2].toUpperCase()))
    : h("h2", { class: "unit-name" }, h("b", {}, name.toUpperCase()));
}

function button(label: string, kind: "primary" | "secondary" | "unlit" | "danger", onclick?: () => void, disabled = false) {
  return h("button", { class: `btn btn-${kind}`, type: "button", disabled, onclick: onclick ? () => onclick() : undefined }, label);
}

// ---- parts --------------------------------------------------------------------------------------------------------
function header(s: ViewState, a: Actions): HTMLElement {
  const n = counts(s.views).updates;
  return h(
    "header",
    { class: "panel brushed head" },
    h(
      "div",
      { class: "wordmark" },
      h("h1", {}, h("span", { class: "light" }, `${t("app.wordmark.light")} `), h("b", {}, t("app.wordmark.bold"))),
      h("div", { class: "subtitle" }, t("app.subtitle")),
    ),
    h(
      "div",
      { class: "head-right" },
      h("div", { class: "labelled" }, h("span", { class: "tiny-label" }, t("header.updates")), display(t("header.ready", { n }), n ? "amber" : "dim")),
      button(t("header.updateAll"), n ? "primary" : "secondary", a.notYet, n === 0),
      h("button", { class: "btn-round", type: "button", title: t("header.settings"), "aria-label": t("header.settings"), onclick: () => a.openSettings(true) }, gear()),
    ),
  );
}

function gear(): SVGSVGElement {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("fill", "currentColor");
  path.setAttribute(
    "d",
    "M19.4 13a7.5 7.5 0 0 0 0-2l2.1-1.6-2-3.5-2.5 1a7.6 7.6 0 0 0-1.7-1L15 3h-4l-.4 2.9a7.6 7.6 0 0 0-1.7 1l-2.5-1-2 3.5L6.6 11a7.5 7.5 0 0 0 0 2l-2.1 1.6 2 3.5 2.5-1a7.6 7.6 0 0 0 1.7 1L11 21h4l.4-2.9a7.6 7.6 0 0 0 1.7-1l2.5 1 2-3.5zM13 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7z",
  );
  svg.append(path);
  return svg;
}

function filterBar(s: ViewState, a: Actions): HTMLElement {
  const c = counts(s.views);
  const seg = (f: Filter, label: string) =>
    h("button", { class: `seg ${s.filter === f ? "on" : ""}`, type: "button", onclick: () => a.setFilter(f), "aria-pressed": s.filter === f ? "true" : "false" }, `${label} · ${c[f]}`);
  const search = h("input", {
    class: "search",
    type: "search",
    placeholder: t("filter.search"),
    value: s.search,
    spellcheck: "false",
    oninput: (e: Event) => a.setSearch((e.target as HTMLInputElement).value),
  });
  return h(
    "div",
    { class: "filterbar" },
    h("div", { class: "segmented" }, seg("all", t("filter.all")), seg("installed", t("filter.installed")), seg("updates", t("filter.updates")), seg("new", t("filter.new"))),
    sectionLine(t("filter.section")),
    h("label", { class: "search-wrap" }, search),
  );
}

function faceplate(v: PluginView): HTMLElement {
  const p = v.plugin;
  if (p.image) return h("div", { class: "faceplate" }, h("img", { src: p.image, alt: p.name, loading: "lazy", draggable: "false" }));
  return h("div", { class: "faceplate faceplate-blank" }, h("span", {}, p.name));
}

function formatLeds(v: PluginView): HTMLElement {
  const offered = v.plugin.formats[PLATFORM] ?? [];
  const all = FORMATS_FOR_PLATFORM[PLATFORM].filter((f) => offered.includes(f));
  return h(
    "div",
    { class: "format-leds" },
    ...all.map((f) => h("span", { class: "fmt" }, led(v.installed?.formats.includes(f) ? "green" : "off"), f.toUpperCase())),
  );
}

function versions(v: PluginView): HTMLElement {
  const latest = v.plugin.latest.version;
  if (v.status === "update" && v.installed)
    return h(
      "div",
      { class: "versions" },
      h("span", { class: "tiny-label" }, t("unit.installedNew")),
      h("div", { class: "version-row" }, display(v.installed.version, "dim"), h("span", { class: "arrow" }, "→"), display(latest)),
    );
  if (v.installed)
    return h("div", { class: "versions" }, h("span", { class: "tiny-label" }, t("unit.installed")), h("div", { class: "version-row" }, display(v.installed.version)));
  return h("div", { class: "versions" }, h("span", { class: "tiny-label" }, t("unit.available")), h("div", { class: "version-row" }, display(latest)));
}

function status(v: PluginView): HTMLElement {
  const [color, text]: ["green" | "amber" | "red" | "off", string] =
    v.status === "update"
      ? ["amber", t("status.updateReady")]
      : v.status === "current"
        ? ["green", t("status.upToDate")]
        : v.status === "unavailable"
          ? ["off", t("status.notForPlatform", { platform: t(`platform.${PLATFORM}`) })]
          : ["off", t("status.notInstalled")];
  return h("div", { class: "status" }, led(color), h("span", {}, text), v.isNew ? h("span", { class: "badge-new" }, t("status.new")) : null);
}

function actionButton(v: PluginView, a: Actions): HTMLElement {
  if (v.status === "update") return button(t("button.update"), "primary", a.notYet);
  if (v.status === "current") return button(t("button.options"), "secondary", a.notYet);
  if (v.status === "unavailable") return button(t("button.install"), "unlit", undefined, true);
  return button(t("button.install"), "unlit", a.notYet);
}

function rackUnit(v: PluginView, a: Actions): HTMLElement {
  return h(
    "article",
    { class: "panel brushed unit", "data-id": v.plugin.id },
    ...screws(),
    faceplate(v),
    h("div", { class: "unit-info" }, pluginTitle(v.plugin.name), h("div", { class: "tagline" }, v.plugin.tagline), formatLeds(v)),
    versions(v),
    status(v),
    h("div", { class: "unit-action" }, actionButton(v, a)),
  );
}

function banner(s: ViewState, a: Actions): HTMLElement | null {
  const l = s.load;
  if (!l) return null;
  if (l.kind === "schema") return h("div", { class: "banner banner-amber" }, led("amber"), h("span", {}, t("banner.schema")));
  if (l.kind === "error" || l.kind === "cached")
    return h("div", { class: "banner banner-red" }, led("red"), h("span", {}, t("banner.offline")), button(t("banner.retry"), "secondary", a.retry));
  return null;
}

function list(s: ViewState, a: Actions): HTMLElement {
  const l = s.load;
  if (!l) return h("main", { class: "list" }, h("div", { class: "empty" }, t("list.loading")));
  if (l.kind === "error") return h("main", { class: "list" });
  const shown = s.views.filter((v) => matches(v, s.filter, s.search));
  if (!s.views.length) return h("main", { class: "list" }, h("div", { class: "empty" }, t("list.empty.catalog")));
  if (!shown.length) return h("main", { class: "list" }, h("div", { class: "empty" }, t("list.empty.filter")));
  return h("main", { class: "list" }, ...shown.map((v) => rackUnit(v, a)));
}

function footer(s: ViewState): HTMLElement {
  const l = s.load;
  const cat =
    !l
      ? [led("off"), t("footer.catalogLoading")]
      : l.kind === "ok"
        ? [led("green"), t("footer.catalogOk", { when: when(l.fetchedAt, s.now) })]
        : l.kind === "cached"
          ? [led("amber"), t("footer.catalogCached", { when: when(l.fetchedAt, s.now) })]
          : [led("red"), t("footer.catalogError")];
  return h(
    "footer",
    { class: "foot" },
    h("span", { class: "foot-item" }, ...cat),
    h("span", { class: "foot-item" }, led("off"), t("footer.dawInactive")),
    h("span", { class: "foot-version" }, t("footer.version", { version: s.appVersion })),
  );
}

// ---- settings -----------------------------------------------------------------------------------------------------
function settingsPanel(s: ViewState, a: Actions): HTMLElement | null {
  if (!s.settingsOpen) return null;
  const st = s.settings;
  const toggleFormat = (f: (typeof st.formats)[number]) => {
    const on = st.formats.includes(f);
    if (on && st.formats.length === 1) return; // keep at least one
    a.changeSettings({ ...st, formats: on ? st.formats.filter((x) => x !== f) : [...st.formats, f] });
  };
  const formatBtn = (f: (typeof st.formats)[number]) => {
    const on = st.formats.includes(f);
    return h(
      "button",
      { class: `btn ${on ? "btn-primary" : "btn-secondary"} btn-led`, type: "button", "aria-pressed": on ? "true" : "false", onclick: () => toggleFormat(f), title: on && st.formats.length === 1 ? t("settings.formatNeeded") : undefined },
      led(on ? "green" : "off"),
      f.toUpperCase(),
    );
  };
  const loc = (value: Settings["location"], label: string) =>
    h("button", { class: `seg ${st.location === value ? "on" : ""}`, type: "button", onclick: () => a.changeSettings({ ...st, location: value }) }, label);
  const reachable = s.load?.kind === "ok";

  const close = () => a.openSettings(false);
  return h(
    "div",
    { class: "modal-backdrop", onclick: (e: Event) => e.target === e.currentTarget && close() },
    h(
      "section",
      { class: "panel brushed modal settings", role: "dialog", "aria-modal": "true", "aria-label": t("settings.title") },
      ...screws(),
      h("h2", { class: "modal-title" }, t("settings.title")),
      sectionLine(PLATFORM === "macos" ? t("settings.formatsMac") : t("settings.formatsWin")),
      h("div", { class: "row" }, ...FORMATS_FOR_PLATFORM[PLATFORM].map(formatBtn)),
      PLATFORM === "macos" ? sectionLine(t("settings.location")) : null,
      PLATFORM === "macos" ? h("div", { class: "row" }, h("div", { class: "segmented" }, loc("system", t("settings.locationSystem")), loc("user", t("settings.locationUser")))) : null,
      sectionLine(t("settings.updates")),
      h(
        "div",
        { class: "row" },
        h(
          "button",
          { class: `btn ${st.checkAtStart ? "btn-primary" : "btn-secondary"}`, type: "button", "aria-pressed": st.checkAtStart ? "true" : "false", onclick: () => a.changeSettings({ ...st, checkAtStart: !st.checkAtStart }) },
          t("settings.checkAtStart"),
        ),
      ),
      sectionLine(t("settings.catalog")),
      h(
        "div",
        { class: "row" },
        display(CATALOG_URL.replace(/^https:\/\//, "").replace(/\/catalog\.json$/, ""), "mint"),
        led(reachable ? "green" : "red"),
        h("span", { class: `tiny-label ${reachable ? "ok" : "bad"}` }, reachable ? t("settings.reachable") : t("settings.unreachable")),
      ),
      sectionLine(t("settings.uninstall")),
      h(
        "div",
        { class: "row" },
        h("button", { class: "btn btn-secondary btn-led", type: "button", onclick: () => a.notYet() }, led("red"), t("settings.uninstallAll")),
        h("span", { class: "note" }, t("settings.uninstallNote")),
      ),
      h(
        "div",
        { class: "row modal-foot" },
        h("span", { class: "tiny-label" }, t("settings.central")),
        display(s.appVersion),
        h("span", { class: "spacer" }),
        button(t("settings.close"), "secondary", close),
      ),
    ),
  );
}

// ---- toast --------------------------------------------------------------------------------------------------------
let toastTimer: number | undefined;
export function toast(text: string): void {
  document.querySelector(".toast")?.remove();
  const el = h("div", { class: "toast", role: "status" }, led("amber"), h("span", {}, text));
  document.body.append(el);
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.remove(), 4500);
}

// ---- whole window -------------------------------------------------------------------------------------------------
export function render(root: HTMLElement, s: ViewState, a: Actions): void {
  // keep focus and caret in the search field across re-renders
  const active = document.activeElement;
  const searchFocused = active instanceof HTMLInputElement && active.classList.contains("search");
  const caret = searchFocused ? active.selectionStart : null;
  const scroll = root.querySelector(".list")?.scrollTop ?? 0;

  root.replaceChildren(header(s, a), filterBar(s, a), banner(s, a) ?? "", list(s, a), footer(s));
  const modal = settingsPanel(s, a);
  if (modal) root.append(modal);

  const listEl = root.querySelector(".list");
  if (listEl) listEl.scrollTop = scroll;
  if (searchFocused) {
    const input = root.querySelector<HTMLInputElement>(".search");
    input?.focus();
    if (input && caret !== null) input.setSelectionRange(caret, caret);
  }
}

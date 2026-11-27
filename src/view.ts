// Builds the main window: header, filter bar, rack units, footer, settings panel.

import type { LoadResult } from "./catalog.js";
import { CATALOG_URL } from "./catalog.js";
import { display, h, led, screws, sectionLine } from "./dom.js";
import { t } from "./i18n/index.js";
import type { FeedbackKind } from "./feedback.js";
import type { Job } from "./install.js";
import { formatsToInstall, targetOf } from "./install.js";
import type { PluginView, Settings } from "./state.js";
import { FORMATS_FOR_PLATFORM, PLATFORM } from "./state.js";

export type Filter = "all" | "installed" | "updates" | "new";

/** The dialog that is open (only one at a time). */
export type Dialog =
  | { kind: "replace"; view: PluginView }
  | { kind: "update"; views: PluginView[]; mode: "update" | "reinstall" }
  | { kind: "whatsNew"; view: PluginView }
  | { kind: "remove"; views: PluginView[]; all: boolean };

export interface ViewState {
  load: LoadResult | null;
  views: PluginView[];
  filter: Filter;
  search: string;
  settings: Settings;
  settingsOpen: boolean;
  appVersion: string;
  now: number;
  /** the running (or last finished) install / remove, by plug-in id */
  jobs: Record<string, Job>;
  dialog: Dialog | null;
  /** open drop-down menu: "feedback" or a plug-in id */
  menu: string | null;
  /** open DAWs (null = unknown, e.g. outside the app) */
  daws: string[] | null;
}

export interface Actions {
  setFilter(f: Filter): void;
  setSearch(s: string): void;
  openSettings(open: boolean): void;
  changeSettings(s: Settings): void;
  retry(): void;
  install(v: PluginView): void;
  update(v: PluginView): void;
  updateAll(): void;
  reinstall(v: PluginView): void;
  whatsNew(v: PluginView): void;
  remove(v: PluginView): void;
  uninstallAll(): void;
  /** OK (true) or Cancel (false) in the open dialog */
  answer(ok: boolean): void;
  cancelInstall(): void;
  toggleMenu(id: string | null): void;
  /** without a plug-in: about Central itself */
  feedback(kind: FeedbackKind, v?: PluginView): void;
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

function button(label: string, kind: "primary" | "secondary" | "unlit" | "danger", onclick?: () => void, disabled = false): HTMLButtonElement {
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
      menuWrap(
        s,
        a,
        "feedback",
        button(t("header.feedback"), "unlit", () => a.toggleMenu(s.menu === "feedback" ? null : "feedback")),
        [
          { label: t("feedback.aboutCentral"), heading: true },
          { label: t("feedback.bug"), run: () => a.feedback("bug") },
          { label: t("feedback.feature"), run: () => a.feedback("feature") },
          { label: t("feedback.idea"), run: () => a.feedback("idea") },
        ],
      ),
      h("div", { class: "labelled" }, h("span", { class: "tiny-label" }, t("header.updates")), display(t("header.ready", { n }), n ? "amber" : "dim")),
      button(t("header.updateAll"), n ? "primary" : "secondary", a.updateAll, n === 0 || anyRunning(s.jobs)),
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

const CHAIN = 12;
const RUNNING: Job["phase"][] = ["queued", "download", "verify", "unpack", "password", "copy", "remove"];
const isRunning = (j: Job | null | undefined): boolean => !!j && RUNNING.includes(j.phase);
export const anyRunning = (jobs: Record<string, Job>): boolean => Object.values(jobs).some(isRunning);

// ---- drop-down menus (OPTIONS ▾, SEND FEEDBACK ▾) -----------------------------------------------------------------
interface MenuItem {
  label: string;
  run?: () => void;
  disabled?: boolean;
  /** a small heading line, not clickable */
  heading?: boolean;
  /** a fine line above this item */
  separator?: boolean;
}

function menuWrap(s: ViewState, a: Actions, id: string, toggle: HTMLElement, items: MenuItem[]): HTMLElement {
  const open = s.menu === id;
  toggle.setAttribute("aria-haspopup", "menu");
  toggle.setAttribute("aria-expanded", open ? "true" : "false");
  const menu = open
    ? h(
        "div",
        { class: "menu", role: "menu" },
        ...items.flatMap((it) => [
          it.separator ? h("div", { class: "menu-sep", role: "separator" }) : null,
          it.heading
            ? h("div", { class: "menu-heading" }, it.label)
            : h(
                "button",
                {
                  class: "menu-item",
                  type: "button",
                  role: "menuitem",
                  disabled: !!it.disabled,
                  onclick: () => {
                    a.toggleMenu(null);
                    it.run?.();
                  },
                },
                it.label,
              ),
        ]),
      )
    : null;
  return h("div", { class: `menu-wrap ${open ? "open" : ""}`, "data-menu": id }, toggle, menu);
}

/** LED chain like the plug-ins' meters: lights up from the left as the install goes on. */
function ledChain(job: Job): HTMLElement {
  const lit = Math.round(job.fraction * CHAIN);
  const color = job.phase === "error" ? "red" : job.phase === "done" ? "green" : "amber";
  const leds = Array.from({ length: CHAIN }, (_, i) => {
    const on = i < lit;
    const blink = job.phase === "password" && i === lit - 1;
    return led(on ? color : "off", blink ? "blink" : "");
  });
  return h("div", { class: "led-chain", role: "progressbar", "aria-valuemin": 0, "aria-valuemax": 100, "aria-valuenow": Math.round(job.fraction * 100) }, ...leds);
}

function phaseText(job: Job): string {
  switch (job.phase) {
    case "download":
      return t("phase.download", { percent: job.total ? Math.floor((100 * job.done) / job.total) : 0 });
    case "verify":
      return t("phase.verify");
    case "unpack":
      return t("phase.unpack");
    case "password":
      return t(PLATFORM === "windows" ? "phase.passwordWin" : "phase.password");
    case "copy":
      return t("phase.copy");
    case "queued":
      return t("phase.queued");
    case "remove":
      return t("phase.remove");
    case "done":
      return t("phase.done");
    case "error":
      return t("status.failed");
  }
}

function jobStatus(job: Job): HTMLElement {
  return h(
    "div",
    { class: "status status-job", title: job.message },
    ledChain(job),
    h("span", { class: job.phase === "error" ? "bad" : "" }, phaseText(job)),
    job.phase === "done" ? h("span", { class: "note" }, t("phase.dawNote")) : null,
  );
}

function status(v: PluginView, job: Job | undefined): HTMLElement {
  if (job) return jobStatus(job);
  if (v.installed && !v.installed.byCentral)
    return h("div", { class: "status" }, led("amber"), h("span", {}, t("status.notByCentral")));
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

function optionsMenu(v: PluginView, s: ViewState, a: Actions, main: boolean): HTMLElement {
  const busy = anyRunning(s.jobs);
  const inst = v.installed;
  const items: MenuItem[] = [];
  if (v.plugin.latest.changelog.length) items.push({ label: t("options.whatsNew", { version: v.plugin.latest.version }), run: () => a.whatsNew(v) });
  if (inst && inst.byCentral) items.push({ label: t("options.reinstall"), run: () => a.reinstall(v), disabled: busy || v.status === "unavailable" });
  if (inst) items.push({ label: t("options.remove"), run: () => a.remove(v), disabled: busy });
  items.push({ label: t("options.bug"), run: () => a.feedback("bug", v), separator: items.length > 0 });
  items.push({ label: t("options.feature"), run: () => a.feedback("feature", v) });
  const toggle = button(t("button.options"), "secondary", () => a.toggleMenu(s.menu === v.plugin.id ? null : v.plugin.id));
  if (!main) toggle.classList.add("btn-small");
  return menuWrap(s, a, v.plugin.id, toggle, items);
}

function actions(v: PluginView, s: ViewState, a: Actions): HTMLElement[] {
  const job = s.jobs[v.plugin.id];
  if (isRunning(job)) {
    const canCancel = job!.phase === "queued" || job!.phase === "download" || job!.phase === "verify" || job!.phase === "unpack";
    if (job!.phase === "remove") return [button(t("button.removing"), "secondary", undefined, true)];
    return [canCancel ? button(t("button.cancel"), "secondary", a.cancelInstall) : button(t("button.installing"), "secondary", undefined, true)];
  }
  const busy = anyRunning(s.jobs);
  if (v.status === "current" && v.installed?.byCentral) return [optionsMenu(v, s, a, true)];
  let main: HTMLElement;
  if (v.installed && !v.installed.byCentral) main = button(t("button.replace"), "secondary", () => a.install(v), busy || v.status === "unavailable");
  else if (v.status === "update") main = button(t("button.update"), "primary", () => a.update(v), busy);
  else if (v.status === "unavailable") main = button(t("button.install"), "unlit", undefined, true);
  else main = button(t("button.install"), "unlit", () => a.install(v), busy);
  return [main, optionsMenu(v, s, a, false)];
}

function rackUnit(v: PluginView, s: ViewState, a: Actions): HTMLElement {
  return h(
    "article",
    { class: "panel brushed unit", "data-id": v.plugin.id },
    ...screws(),
    faceplate(v),
    h("div", { class: "unit-info" }, pluginTitle(v.plugin.name), h("div", { class: "tagline" }, v.plugin.tagline), formatLeds(v)),
    versions(v),
    status(v, s.jobs[v.plugin.id]),
    h("div", { class: "unit-action" }, ...actions(v, s, a)),
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
  return h("main", { class: "list" }, ...shown.map((v) => rackUnit(v, s, a)));
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
    s.daws === null
      ? h("span", { class: "foot-item" }, led("off"), t("footer.dawUnknown"))
      : s.daws.length
        ? h("span", { class: "foot-item foot-daw" }, led("amber"), t("footer.dawOpen", { names: s.daws.join(", ") }))
        : h("span", { class: "foot-item" }, led("green"), t("footer.dawNone")),
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
  const anyInstalled = s.views.some((v) => v.installed);

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
        h(
          "button",
          { class: "btn btn-secondary btn-led", type: "button", disabled: !anyInstalled || anyRunning(s.jobs), onclick: () => a.uninstallAll() },
          led(anyInstalled ? "red" : "off"),
          t("settings.uninstallAll"),
        ),
        h("span", { class: "note" }, anyInstalled ? t(PLATFORM === "windows" ? "settings.uninstallNoteWin" : "settings.uninstallNote") : t("settings.nothingInstalled")),
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

// ---- dialogs ------------------------------------------------------------------------------------------------------
function dawWarning(s: ViewState): HTMLElement | null {
  if (!s.daws || !s.daws.length) return null;
  const names = s.daws.join(", ");
  return h("p", { class: "warn" }, led("amber"), h("span", {}, t(PLATFORM === "windows" ? "daw.warnWin" : "daw.warnMac", { names })));
}

function dialogFrame(title: string, a: Actions, body: Array<HTMLElement | null>, okLabel: string | null, okKind: "primary" | "danger" = "primary", cancelLabel = t("update.cancel")): HTMLElement {
  const cancel = () => a.answer(false);
  return h(
    "div",
    { class: "modal-backdrop", onclick: (e: Event) => e.target === e.currentTarget && cancel() },
    h(
      "section",
      { class: "panel brushed modal confirm", role: "alertdialog", "aria-modal": "true", "aria-label": title },
      ...screws(),
      h("h2", { class: "modal-title" }, title),
      ...body,
      h(
        "div",
        { class: "row modal-foot" },
        h("span", { class: "spacer" }),
        button(cancelLabel, "secondary", cancel),
        okLabel ? button(okLabel, okKind, () => a.answer(true)) : null,
      ),
    ),
  );
}

const where = (l: Settings["location"]) => t(`location.${l}${PLATFORM === "windows" ? "Win" : ""}` as const);

function replaceDialog(s: ViewState, a: Actions, v: PluginView): HTMLElement | null {
  if (!v.installed) return null;
  const p = v.plugin;
  const formats = formatsToInstall(p, s.settings).map((f) => f.toUpperCase()).join(", ");
  const location = where(s.settings.location);
  const found = v.installed.location;
  const elsewhere = found !== null && found !== s.settings.location;
  return dialogFrame(
    t("replace.title", { name: p.name }),
    a,
    [
      h("p", {}, t("replace.found", { name: p.name, version: v.installed.version })),
      h("p", {}, t("replace.will", { version: p.latest.version, formats, location })),
      elsewhere ? h("p", { class: "warn" }, led("amber"), h("span", {}, t("replace.otherPlace", { found: where(found ?? "system"), location }))) : null,
      dawWarning(s),
    ],
    t("replace.ok"),
    "primary",
    t("replace.cancel"),
  );
}

/** Changelog lines of the catalog's newest version, at most `max`. */
function notes(v: PluginView, max: number): HTMLElement {
  const lines = v.plugin.latest.changelog;
  if (!lines.length) return h("p", { class: "note" }, t("update.noNotes"));
  const shown = lines.slice(0, max);
  return h(
    "ul",
    { class: "notes" },
    ...shown.map((l) => h("li", {}, l)),
    lines.length > shown.length ? h("li", { class: "note" }, t("update.more", { n: lines.length - shown.length })) : null,
  );
}

function updateDialog(s: ViewState, a: Actions, views: PluginView[], mode: "update" | "reinstall"): HTMLElement {
  const many = views.length > 1;
  const first = views[0]!;
  const fmts = (v: PluginView) => targetOf(v.plugin, s.settings, v.installed, true).formats.map((f) => f.toUpperCase()).join(", ");
  const needsPrompt = views.some((v) => targetOf(v.plugin, s.settings, v.installed, true).location === "system");
  const title = many ? t("update.titleMany", { n: views.length }) : t(mode === "reinstall" ? "update.titleReinstall" : "update.title", { name: first.plugin.name });
  const rows = views.map((v) =>
    h(
      "div",
      { class: "update-row" },
      h(
        "div",
        { class: "row" },
        h("b", { class: "update-name" }, v.plugin.name),
        v.installed && mode === "update" ? display(v.installed.version, "dim") : null,
        mode === "update" ? h("span", { class: "arrow" }, "→") : null,
        display(v.plugin.latest.version),
        v.installed && !v.installed.byCentral ? h("span", { class: "note" }, t("update.notByCentral")) : null,
      ),
      mode === "update" ? notes(v, many ? 4 : 12) : null,
    ),
  );
  const will = many
    ? t("update.willMany")
    : mode === "reinstall"
      ? t("update.willReinstall", { version: first.plugin.latest.version, formats: fmts(first) })
      : t("update.will", { formats: fmts(first) });
  return dialogFrame(
    title,
    a,
    [
      h("div", { class: "update-list" }, ...rows),
      h("p", {}, will),
      many && needsPrompt ? h("p", { class: "note" }, t(PLATFORM === "windows" ? "update.onePromptWin" : "update.onePrompt")) : null,
      dawWarning(s),
    ],
    t(many ? "update.okMany" : mode === "reinstall" ? "update.okReinstall" : "update.ok"),
  );
}

function whatsNewDialog(a: Actions, v: PluginView): HTMLElement {
  const p = v.plugin;
  const date = p.latest.releasedAt ? new Date(p.latest.releasedAt) : null;
  return dialogFrame(
    t("whatsNew.title", { name: p.name, version: p.latest.version }),
    a,
    [
      date && !isNaN(date.getTime()) ? h("span", { class: "tiny-label" }, t("whatsNew.released", { date: date.toLocaleDateString("en-GB") })) : null,
      notes(v, 40),
    ],
    v.status === "update" ? t("whatsNew.update") : null,
    "primary",
    t("whatsNew.close"),
  );
}

function removeDialog(s: ViewState, a: Actions, views: PluginView[], all: boolean): HTMLElement {
  const first = views[0]!;
  const notByCentral = views.some((v) => v.installed && !v.installed.byCentral);
  const body: Array<HTMLElement | null> = all
    ? [h("p", {}, t("remove.plugins")), h("ul", { class: "notes" }, ...views.map((v) => h("li", {}, `${v.plugin.name} ${v.installed?.version ?? ""}`.trim())))]
    : [h("p", {}, t("remove.files")), h("ul", { class: "paths" }, ...(first.installed?.paths ?? []).map((p) => h("li", {}, p)))];
  body.push(
    notByCentral ? h("p", { class: "note" }, t("remove.notByCentral")) : null,
    h("p", { class: "note" }, t("remove.projects")),
    views.length > 1 && views.some((v) => v.installed?.location === "system")
      ? h("p", { class: "note" }, t(PLATFORM === "windows" ? "update.onePromptWin" : "update.onePrompt"))
      : null,
    dawWarning(s),
  );
  return dialogFrame(
    all ? t("remove.titleAll", { n: views.length }) : t("remove.title", { name: first.plugin.name }),
    a,
    body,
    t(all ? "remove.okAll" : "remove.ok"),
    "danger",
    t("remove.cancel"),
  );
}

function dialog(s: ViewState, a: Actions): HTMLElement | null {
  const d = s.dialog;
  if (!d) return null;
  switch (d.kind) {
    case "replace":
      return replaceDialog(s, a, d.view);
    case "update":
      return d.views.length ? updateDialog(s, a, d.views, d.mode) : null;
    case "whatsNew":
      return whatsNewDialog(a, d.view);
    case "remove":
      return d.views.length ? removeDialog(s, a, d.views, d.all) : null;
  }
}

// ---- toast --------------------------------------------------------------------------------------------------------
let toastTimer: number | undefined;
export function toast(text: string, color: "green" | "amber" | "red" = "amber", ms = 4500): void {
  document.querySelector(".toast")?.remove();
  const el = h("div", { class: "toast", role: "status" }, led(color), h("span", {}, text));
  document.body.append(el);
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.remove(), ms);
}

// ---- whole window -------------------------------------------------------------------------------------------------
export function render(root: HTMLElement, s: ViewState, a: Actions): void {
  // keep focus and caret in the search field across re-renders
  const active = document.activeElement;
  const searchFocused = active instanceof HTMLInputElement && active.classList.contains("search");
  const caret = searchFocused ? active.selectionStart : null;
  const scroll = root.querySelector(".list")?.scrollTop ?? 0;

  root.replaceChildren(header(s, a), filterBar(s, a), banner(s, a) ?? "", list(s, a), footer(s));
  const modal = settingsPanel(s, a) ?? dialog(s, a);
  if (modal) root.append(modal);

  const listEl = root.querySelector(".list");
  if (listEl) listEl.scrollTop = scroll;
  // a menu near the bottom of the list opens upwards, so it is not cut off
  const menu = root.querySelector<HTMLElement>(".list .menu");
  if (menu && listEl) {
    const m = menu.getBoundingClientRect();
    const l = listEl.getBoundingClientRect();
    if (m.bottom > l.bottom - 4) menu.classList.add("menu-up");
  }
  if (searchFocused) {
    const input = root.querySelector<HTMLInputElement>(".search");
    input?.focus();
    if (input && caret !== null) input.setSelectionRange(caret, caret);
  }
}

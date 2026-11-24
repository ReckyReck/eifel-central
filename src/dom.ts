// Tiny DOM helper. Text always goes in as text (never as HTML), so catalog contents can never inject markup.

type Child = Node | string | null | undefined | false;
type Attrs = Record<string, string | number | boolean | EventListener | undefined>;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === false) continue;
    if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
    else if (k === "class") el.className = String(v);
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children) if (c !== null && c !== undefined && c !== false) el.append(c);
  return el;
}

export function led(color: "green" | "amber" | "red" | "off", extra = ""): HTMLSpanElement {
  return h("span", { class: `led led-${color} ${extra}`.trim(), "aria-hidden": "true" });
}

export function display(text: string, variant: "amber" | "dim" | "mint" = "amber"): HTMLSpanElement {
  return h("span", { class: `display display-${variant}` }, text);
}

export function sectionLine(label: string): HTMLDivElement {
  return h("div", { class: "section-line" }, h("span", {}, label));
}

export function screws(): HTMLSpanElement[] {
  return ["tl", "tr", "bl", "br"].map((p) => h("span", { class: `screw screw-${p}`, "aria-hidden": "true" }));
}

// "Send feedback": builds the address of a new issue in Recky's feedback repo (GitHub Issue forms) and asks the
// app to open it in the browser. No token, no server, no GitHub API: the tester fills in the form on github.com.
// Spec: claude/plan-Central-feedback.md. Text fields (title, version, os) are filled in; dropdowns cannot be (GitHub
// does not pre-select them), so the plug-in goes into the title and the label. Never send a parameter called
// "format" (GitHub answers HTTP 406) - the bug form's format field is "plugin_format" and stays empty.
// The repo is private: only invited testers can open it. Change this before a public release.

export const FEEDBACK_NEW = "https://github.com/ReckyReck/eifel-feedback/issues/new";

export type FeedbackKind = "bug" | "feature" | "idea";

export interface FeedbackTarget {
  /** "Recky's Rider" or "Eifel Mastering Central" */
  name: string;
  version: string;
  /** "plugin: reckys-rider" or "app: central" */
  label: string;
}

const TEMPLATE: Record<FeedbackKind, string> = { bug: "bug.yml", feature: "feature.yml", idea: "idea.yml" };
const PREFIX: Record<FeedbackKind, string> = { bug: "[Bug]", feature: "[Feature]", idea: "[Idea]" };

/** The form's address. `os` = "macOS 26.1 (Apple Silicon)" (bug reports only; left out when unknown). */
export function feedbackUrl(kind: FeedbackKind, target: FeedbackTarget, os: string | null): string {
  const title = kind === "idea" ? `${PREFIX[kind]} ${target.name}: ` : `${PREFIX[kind]} ${target.name} ${target.version}: `;
  // "labels" replaces the form's own labels, so type and status are always sent too
  const params: Array<[string, string]> = [
    ["template", TEMPLATE[kind]],
    ["title", title],
    ["labels", `type: ${kind},status: new,${target.label}`],
  ];
  if (kind !== "idea") params.push(["version", target.version]);
  if (kind === "bug" && os) params.push(["os", os]);
  return `${FEEDBACK_NEW}?${params.map(([k, v]) => `${k}=${encodeURIComponent(v)}`).join("&")}`;
}

/** Opens the form in the default browser (the app accepts only FEEDBACK_NEW addresses). */
export async function openFeedback(url: string): Promise<{ ok: true } | { ok: false; message: string }> {
  const invoke = window.__TAURI__?.core.invoke;
  if (!invoke) return { ok: false, message: "Opening the browser works only in the app." };
  try {
    await invoke("open_feedback", { url });
    return { ok: true };
  } catch (e) {
    return { ok: false, message: typeof e === "string" ? e : "The browser could not be opened." };
  }
}

import { en, type TextKey } from "./en.js";

const texts: Record<TextKey, string> = en;

/** Text for a key, with {placeholders} filled in. */
export function t(key: TextKey, params: Record<string, string | number> = {}): string {
  return texts[key].replace(/\{(\w+)\}/g, (_, name: string) => (name in params ? String(params[name]) : `{${name}}`));
}

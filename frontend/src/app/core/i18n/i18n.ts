import { computed, signal } from '@angular/core';

export type Language = 'en' | 'fr';
export const LANGUAGES: { id: Language; name: string }[] = [
  { id: 'en', name: 'English' },
  { id: 'fr', name: 'Français' },
];

/** The texts of each language but English, by their English text: one JSON file each,
 * loaded when the language is chosen. (English is the key: a missing translation shows in
 * English.) */
const FILES: Record<Exclude<Language, 'en'>, () => Promise<{ default: Texts }>> = {
  fr: () => import('./fr.json'),
};

type Texts = Record<string, string>;

const STORAGE_KEY = 'prisme.language';

const current = signal<{ language: Language; texts: Texts }>({ language: 'en', texts: {} });

/** The language of the UI. Read in a template or a `computed`, the text follows it. */
export const language = computed(() => current().language);

/** For dates and numbers. */
export const locale = computed(() => (language() === 'fr' ? 'fr-FR' : 'en-GB'));

function savedLanguage(): Language {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(STORAGE_KEY);
  } catch {
    // storage unavailable: follow the system
  }
  if (saved === 'en' || saved === 'fr') return saved;
  return navigator.language.toLowerCase().startsWith('fr') ? 'fr' : 'en';
}

async function load(language: Language) {
  const texts = language === 'en' ? {} : (await FILES[language]()).default;
  current.set({ language, texts });
  document.documentElement.lang = language;
}

/** The language saved, or the system's: loaded before the UI starts. */
export function loadLanguage(): Promise<void> {
  return load(savedLanguage());
}

/** Switches the language: every text of the UI follows at once. */
export async function setLanguage(language: Language) {
  try {
    localStorage.setItem(STORAGE_KEY, language);
  } catch {
    // kept for this run only
  }
  await load(language);
}

export type Params = Record<string, string | number>;

/**
 * `text` in the UI's language, its `{name}` placeholders filled from `params`. Called once
 * at load (in a constant), it would stay in the language of the start: call it where the
 * text is shown (a template, a `computed`, a getter).
 */
export function t(text: string, params?: Params): string {
  return fill(current().texts[text] || text, params);
}

/** `text` in a language whose texts are given (for the tests). */
export function translate(texts: Texts, text: string, params?: Params): string {
  return fill(texts[text] || text, params);
}

function fill(text: string, params?: Params): string {
  return params
    ? text.replace(/\{(\w+)\}/g, (all, name: string) => String(params[name] ?? all))
    : text;
}

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

const current = signal<{ language: Language; texts: Texts }>({ language: 'en', texts: {} });

/** The language of the UI. Read in a template or a `computed`, the text follows it. */
export const language = computed(() => current().language);

/** For dates and numbers. */
export const locale = computed(() => (language() === 'fr' ? 'fr-FR' : 'en-GB'));

/** Shows the UI in `chosen` (`null`, or one it does not have: the system's): every text
 * follows at once. */
export async function showLanguage(chosen: string | null) {
  const language: Language =
    chosen === 'en' || chosen === 'fr'
      ? chosen
      : navigator.language.toLowerCase().startsWith('fr')
        ? 'fr'
        : 'en';
  if (language === current().language && document.documentElement.lang === language) return;
  const texts = language === 'en' ? {} : (await FILES[language]()).default;
  current.set({ language, texts });
  document.documentElement.lang = language;
}

export type Params = Record<string, string | number>;

/** A text of the UI kept in a constant, in the language of the moment it is read:
 * `() => t('…')`. */
export type Text = () => string;

/**
 * `text` in the UI's language, its `{name}` placeholders filled from `params`. Called once
 * at load (in a constant), it would stay in the language of the start: call it where the
 * text is shown (a template, a `computed`), or keep it in a constant as a `Text`.
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

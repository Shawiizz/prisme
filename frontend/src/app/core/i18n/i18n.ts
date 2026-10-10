import { FR } from './fr';

export type Language = 'en' | 'fr';
export const LANGUAGES: { id: Language; name: string }[] = [
  { id: 'en', name: 'English' },
  { id: 'fr', name: 'Français' },
];

const STORAGE_KEY = 'prisme.language';

/**
 * The language of this run of the UI: chosen once at startup (the English text is the
 * key, so a missing translation shows in English).
 */
export const LANGUAGE: Language = chosenLanguage();

/** For dates and numbers. */
export const LOCALE = LANGUAGE === 'fr' ? 'fr-FR' : 'en-GB';

function chosenLanguage(): Language {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(STORAGE_KEY);
  } catch {
    // storage unavailable: follow the system
  }
  if (saved === 'en' || saved === 'fr') return saved;
  return navigator.language.toLowerCase().startsWith('fr') ? 'fr' : 'en';
}

/** Switches the language: the UI starts again in it (the audio keeps playing). */
export function setLanguage(language: Language) {
  try {
    localStorage.setItem(STORAGE_KEY, language);
  } catch {
    return;
  }
  location.reload();
}

export type Params = Record<string, string | number>;

/** `text` in the UI's language, its `{name}` placeholders filled from `params`. */
export function t(text: string, params?: Params): string {
  return translate(LANGUAGE, text, params);
}

export function translate(language: Language, text: string, params?: Params): string {
  const translated = (language === 'fr' && FR[text]) || text;
  return params
    ? translated.replace(/\{(\w+)\}/g, (all, name: string) => String(params[name] ?? all))
    : translated;
}

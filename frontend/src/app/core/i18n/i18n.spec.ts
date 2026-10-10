import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { FR } from './fr';
import { translate } from './i18n';

/** Every text the UI translates: `t('…')` in the code, `'…' | t` in the templates (inline ones too). */
function textsInSources(): Set<string> {
  const texts = new Set<string>();
  const files = readdirSync('src/app', { recursive: true, encoding: 'utf8' })
    // the translation machinery itself holds no UI text
    .filter((f) => /\.(ts|html)$/.test(f) && !f.endsWith('.spec.ts') && !/^core[\\/]i18n/.test(f))
    .map((f) => join('src/app', f));
  const call = /\bt\(\s*(?:'((?:[^'\\]|\\.)*)'|"((?:[^"\\]|\\.)*)")/g;
  const pipe = /'((?:[^'\\]|\\.)*)'\s*\|\s*t\b/g;
  for (const file of files) {
    const source = readFileSync(file, 'utf8');
    for (const pattern of file.endsWith('.ts') ? [call, pipe] : [pipe]) {
      for (const [, single, double] of source.matchAll(pattern)) {
        texts.add(single?.replace(/\\'/g, "'") ?? double);
      }
    }
  }
  return texts;
}

const placeholders = (text: string) => [...text.matchAll(/\{\w+\}/g)].map((m) => m[0]).sort();

describe('translations', () => {
  const texts = textsInSources();

  it('cover every text of the UI', () => {
    expect([...texts].filter((text) => !(text in FR))).toEqual([]);
  });

  it('are all used', () => {
    expect(Object.keys(FR).filter((text) => !texts.has(text))).toEqual([]);
  });

  it('keep the placeholders', () => {
    expect(
      Object.entries(FR).filter(([en, fr]) => placeholders(en).join() !== placeholders(fr).join()),
    ).toEqual([]);
  });

  it('fill the placeholders, and fall back to English', () => {
    expect(translate('en', 'Hot cue {n}', { n: 3 })).toBe('Hot cue 3');
    expect(translate('fr', 'Not a UI text {x}', { x: 1 })).toBe('Not a UI text 1');
  });
});

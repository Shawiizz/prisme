import { showLanguage } from '../core/i18n/i18n';
import type { MusicalKey } from '../core/models/backend';
import { musicalKeyName } from './camelot';

describe('musicalKeyName', () => {
  const key = (note: string, accidental: string, minor: boolean) =>
    ({ note, accidental, minor }) as MusicalKey;

  afterEach(() => showLanguage('en'));

  it('names a key by its tonic, spelled, and its mode, in the language shown', async () => {
    expect(musicalKeyName(key('C', '♯', true))).toBe('C♯ minor');
    expect(musicalKeyName(key('E', '', false))).toBe('E major');
    await showLanguage('fr');
    expect(musicalKeyName(key('D', '♭', false))).toBe('Ré♭ majeur');
    expect(musicalKeyName(key('B', '', true))).toBe('Si mineur');
  });
});

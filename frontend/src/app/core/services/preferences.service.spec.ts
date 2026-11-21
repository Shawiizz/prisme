import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { loadPreferences, PreferencesService } from './preferences.service';
import type { Preferences } from '../models/backend';

const preferences = (ui: Partial<Preferences['ui']>) =>
  ({ ui: { language: null, phoneTouch: null, screenBelow: false, ...ui } }) as Preferences;

describe('loadPreferences', () => {
  afterEach(() => {
    clearMocks();
    localStorage.clear();
  });

  it('gives the backend what the page kept once, then reads the preferences it keeps', async () => {
    localStorage.setItem('prisme.language', 'en');
    localStorage.setItem('prisme.screenBelow', 'true');
    let kept = preferences({});
    const given: unknown[] = [];
    mockIPC((command, args) => {
      if (command === 'get_preferences') return kept;
      const { setting } = args as { setting: { setting: string; value: unknown } };
      given.push(setting);
      kept = preferences({ ...kept.ui, [setting.setting]: setting.value });
      return null;
    });
    await loadPreferences();
    expect(given).toEqual([
      { setting: 'language', value: 'en' },
      { setting: 'screenBelow', value: true },
    ]);
    expect(TestBed.inject(PreferencesService).ui()).toEqual(kept.ui);
    expect(localStorage.length).toBe(0);
  });

  it('keeps what the backend could not take for the next start', async () => {
    localStorage.setItem('prisme.phone.touch', 'false');
    mockIPC((command) => {
      if (command === 'get_preferences') return preferences({});
      throw new Error('not now');
    });
    await loadPreferences();
    expect(localStorage.getItem('prisme.phone.touch')).toBe('false');
  });
});

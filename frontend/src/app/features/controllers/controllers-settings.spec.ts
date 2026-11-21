import { ComponentFixture, TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { vi } from 'vitest';
import { ControlState } from '../../core/models/engine';
import { EngineService } from '../../core/services/engine.service';
import { ControllersSettings, found } from './controllers-settings';
import type { Controller, Target } from '../../core/models/backend';

const VCI_100: Controller = {
  name: 'VCI-100',
  enabled: true,
  plugged: true,
  profile: 'Vestax VCI-100',
  profileFile: 'vestax-vci-100.json',
  offered: [
    { file: 'vestax-vci-100.json', name: 'Vestax VCI-100' },
    { file: 'vestax-vci-100-hile.json', name: 'Vestax VCI-100 (Hile Mod)' },
  ],
  verified: false,
  profileSource: 'shipped',
  replaces: null,
  exportFile: 'vestax-vci-100.json',
  listed: true,
  canExport: true,
};

/** A deck's action, of a deck shown or not. */
function deckTarget(id: string, label: string, deck: number, inUse = deck < 2): Target {
  const letter = 'ABCD'[deck];
  return {
    id: `deck${deck + 1}.${id}`,
    kind: 'button',
    deck,
    group: { key: 'Deck {deck}', params: { deck: letter } },
    label: { key: label },
    inUse,
  };
}

const TARGETS: Target[] = [
  deckTarget('play', 'Play', 0),
  deckTarget('cue', 'Cue', 0),
  deckTarget('play', 'Play', 1),
  deckTarget('play', 'Play', 2),
  { ...deckTarget('beat', 'On the beat', 0), kind: 'light' },
  {
    id: 'mixer.crossfader',
    kind: 'fader',
    deck: null,
    group: { key: 'Mixer' },
    label: { key: 'Crossfader' },
    inUse: true,
  },
];

describe('ControllersSettings', () => {
  let fixture: ComponentFixture<ControllersSettings>;
  afterEach(() => clearMocks());

  async function settings(controllers: Controller[], chosen: unknown[] = []) {
    mockIPC((command, args) => {
      if (command === 'choose_controller_profile') chosen.push(args);
      if (command === 'list_targets') return TARGETS;
      if (command === 'midi_outputs') return [];
      return undefined;
    });
    const profiles = { profiles: [], counts: { all: 0, updated: 0, own: 0 }, replacing: [] };
    const shown = { controllers, canLearn: true, profiles, mapped: [], learning: null };
    TestBed.inject(EngineService).controls.set({ controllers: shown } as unknown as ControlState);
    fixture = TestBed.createComponent(ControllersSettings);
    fixture.detectChanges();
    // the outputs asked for at the start come
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
    return fixture.nativeElement as HTMLElement;
  }

  it('offer the other profiles of a controller, the one in use chosen', async () => {
    const chosen: unknown[] = [];
    const element = await settings([VCI_100], chosen);
    const select = element.querySelector<HTMLSelectElement>('.choice select')!;
    expect([...select.options].map((o) => o.text.trim())).toEqual([
      'Vestax VCI-100',
      'Vestax VCI-100 (Hile Mod)',
    ]);
    expect(select.value).toBe('vestax-vci-100.json');

    select.value = 'vestax-vci-100-hile.json';
    select.dispatchEvent(new Event('change'));
    await Promise.resolve();
    expect(chosen).toEqual([{ name: 'VCI-100', file: 'vestax-vci-100-hile.json' }]);
  });

  it('say whether a profile was tried on the controller, never of an own profile', async () => {
    const element = await settings([
      VCI_100,
      { ...VCI_100, name: 'DDJ-SB2', verified: true },
      { ...VCI_100, name: 'Mine', profileSource: 'own' },
    ]);
    const badges = [...element.querySelectorAll('.controller')].map(
      (row) => row.querySelector('.pr-chip')?.textContent?.trim() ?? null,
    );
    expect(badges).toEqual(['Not verified', 'Verified', null]);
  });

  it('offer no choice to a controller its model alone fits', async () => {
    const element = await settings([{ ...VCI_100, offered: VCI_100.offered.slice(0, 1) }]);
    expect(element.querySelector('.choice')).toBeNull();
  });

  it('find any action by its name, its group or its id, the decks not shown too', async () => {
    const element = await settings([VCI_100]);
    const input = element.querySelector<HTMLInputElement>('input.find')!;
    const groups = () => [...element.querySelectorAll('.fold .pr-label')].map((g) => g.textContent);
    const rows = () => [...element.querySelectorAll('.mapping .name')].map((n) => n.textContent);
    const search = (text: string) => {
      input.value = text;
      input.dispatchEvent(new Event('input'));
      fixture.detectChanges();
    };
    expect(groups()).toEqual(['Deck A', 'Deck B', 'Mixer']);
    expect(rows()).toEqual([]);

    search('play');
    expect(groups()).toEqual(['Deck A', 'Deck B', 'Deck C']);
    expect(rows()).toEqual(['Play', 'Play', 'Play']);

    search(' deck   B ');
    expect(rows()).toEqual(['Play']);

    search('MIXER.CROSS');
    expect(rows()).toEqual(['Crossfader']);

    search('beat');
    expect(element.querySelector('.hint + .hint')?.textContent?.trim()).toBe('No action found.');

    search('play');
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true });
    const closed = vi.fn();
    element.addEventListener('keydown', closed);
    input.dispatchEvent(escape);
    fixture.detectChanges();
    expect(input.value).toBe('');
    expect(closed).not.toHaveBeenCalled();
    expect(groups()).toEqual(['Deck A', 'Deck B', 'Mixer']);
  });
});

describe('found', () => {
  const control = {
    id: 'deck1.play',
    group: 'Mélangeur',
    label: 'Lecture',
    kind: 'button' as const,
    inUse: true,
  };

  it('need every word in the name, the group or the id', () => {
    expect(found(['lecture'], control)).toBe(true);
    expect(found(['melangeur', 'lect'], control)).toBe(true);
    expect(found(['deck1.play'], control)).toBe(true);
    expect(found(['lecture', 'deck2'], control)).toBe(false);
  });
});

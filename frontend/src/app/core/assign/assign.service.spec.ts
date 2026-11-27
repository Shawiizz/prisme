import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { vi } from 'vitest';
import { AssignService } from './assign.service';
import { KeyboardService } from '../keyboard/keyboard.service';
import { EngineService } from '../services/engine.service';
import { tellPreferences } from '../services/preferences.service';
import type { ControlState } from '../models/engine';
import type { Preferences, Target, TakenKey } from '../models/backend';

const TARGETS: Target[] = [
  {
    id: 'deck1.play',
    kind: 'button',
    deck: 0,
    group: { key: 'Deck {deck}', params: { deck: 'A' } },
    label: { key: 'Play' },
    inUse: true,
  },
  {
    id: 'deck1.volume',
    kind: 'fader',
    deck: 0,
    group: { key: 'Deck {deck}', params: { deck: 'A' } },
    label: { key: 'Volume' },
    inUse: true,
  },
];

const CUE_B: TakenKey = {
  code: 'KeyP',
  control: 'deck2.cue',
  action: {
    id: 'deck2.cue',
    kind: 'button',
    deck: 1,
    group: { key: 'Deck {deck}', params: { deck: 'B' } },
    label: { key: 'Cue' },
    inUse: true,
  },
};

describe('AssignService', () => {
  let calls: { command: string; args: unknown }[];
  let taken: TakenKey[];

  beforeEach(async () => {
    calls = [];
    taken = [];
    mockIPC(
      (command, args) => {
        calls.push({ command, args });
        if (command === 'list_targets') return TARGETS;
        if (command === 'set_keys') return (args as { take: boolean }).take ? [] : taken;
        return undefined;
      },
      { shouldMockEvents: true },
    );
    tellPreferences({ shortcuts: [], ui: { language: null } } as unknown as Preferences);
    controllers({ canLearn: false, learning: null });
    await TestBed.inject(KeyboardService).start();
    document.body.innerHTML = `
      <button id="play" data-assign="deck1.play"></button>
      <div id="volume" data-assign="deck1.volume"></div>
      <button id="other"></button>
      <div data-assign-ui><button id="done"></button></div>`;
    // the actions listed
    TestBed.inject(AssignService);
    await settle();
  });

  afterEach(() => {
    TestBed.inject(AssignService).stop();
    clearMocks();
    document.body.replaceChildren();
  });

  function controllers(told: { canLearn: boolean; learning: string | null }) {
    const shown = { controllers: [], profiles: [], mapped: [], ...told };
    TestBed.inject(EngineService).controls.set({ controllers: shown } as unknown as ControlState);
  }

  function press(id: string) {
    const element = document.getElementById(id)!;
    element.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0 }));
    element.dispatchEvent(new MouseEvent('click', { bubbles: true }));
  }

  function key(code: string) {
    window.dispatchEvent(new KeyboardEvent('keydown', { code, key: code.slice(3), bubbles: true }));
  }

  it('hold every press back but on its own bar, and pick the button pressed', () => {
    const assign = TestBed.inject(AssignService);
    const clicked = vi.fn();
    document.getElementById('other')!.addEventListener('click', clicked);
    document.getElementById('done')!.addEventListener('click', clicked);
    assign.begin();

    press('other');
    expect(clicked).not.toHaveBeenCalled();
    expect(assign.chosen()).toBeNull();
    press('done');
    expect(clicked).toHaveBeenCalledTimes(1);
    press('play');
    expect(assign.chosen()?.control).toBe('deck1.play');
    expect(assign.step()).toEqual({ kind: 'listening' });

    assign.stop();
    press('other');
    expect(clicked).toHaveBeenCalledTimes(2);
  });

  it('give a key, and ask before taking one another action has', async () => {
    const assign = TestBed.inject(AssignService);
    assign.begin();
    taken = [CUE_B];
    press('play');
    key('KeyP');
    await settle();
    expect(assign.step()).toEqual({ kind: 'taken', key: { code: 'KeyP' }, from: [CUE_B] });
    expect(assign.pointed()).toBe('deck2.cue');

    assign.take();
    await settle();
    const given = calls.filter((c) => c.command === 'set_keys').map((c) => c.args);
    expect(given).toEqual([
      { control: 'deck1.play', keys: [{ code: 'KeyP' }], take: false },
      { control: 'deck1.play', keys: [{ code: 'KeyP' }], take: true },
    ]);
    expect(assign.chosen()).toBeNull();
  });

  it('give a key only to a key’s action, a controller’s control only with one open', () => {
    const assign = TestBed.inject(AssignService);
    assign.begin();
    expect(assign.allows('deck1.play')).toBe(true);
    expect(assign.allows('deck1.volume')).toBe(false);
    expect(assign.allows('deck9.play')).toBe(false);

    assign.choose('controller');
    expect(assign.allows('deck1.volume')).toBe(false);
    controllers({ canLearn: true, learning: null });
    expect(assign.allows('deck1.volume')).toBe(true);
  });

  it('learn the control moved on the controller', async () => {
    const assign = TestBed.inject(AssignService);
    controllers({ canLearn: true, learning: null });
    assign.begin();
    assign.choose('controller');
    press('volume');
    await settle();
    expect(calls.some((c) => c.command === 'learn_control')).toBe(true);

    controllers({ canLearn: true, learning: 'deck1.volume' });
    TestBed.tick();
    controllers({ canLearn: true, learning: null });
    TestBed.tick();
    expect(assign.step()).toEqual({ kind: 'learnt' });
  });

  it('end with Escape, after the bubble first', () => {
    const assign = TestBed.inject(AssignService);
    assign.begin();
    assign.choose('controller');
    controllers({ canLearn: true, learning: null });
    press('volume');
    key('Escape');
    expect(assign.chosen()).toBeNull();
    expect(assign.on()).toBe(true);
    key('Escape');
    expect(assign.on()).toBe(false);
  });
});

/** The promises and the backend's answers settled. */
async function settle() {
  for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve));
}

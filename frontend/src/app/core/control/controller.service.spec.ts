import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { ControllerMapping } from '../models/controllers';
import { EngineStatus } from '../models/engine';
import { EngineService } from '../services/engine.service';
import { PreferencesService } from '../services/preferences.service';
import { ControllerService } from './controller.service';

describe('ControllerService', () => {
  afterEach(() => clearMocks());

  it('puts out the lights of a deck no longer shown', async () => {
    const sent: unknown[] = [];
    mockIPC((command, args) => {
      if (command === 'set_lights') sent.push(...(args as { lights: unknown[] }).lights);
    });
    const preferences = TestBed.inject(PreferencesService);
    const deckCount = (n: number) => preferences.dj.update((dj) => ({ ...dj, deckCount: n }));
    TestBed.inject(EngineService).status.set({
      decks: [0, 1, 2, 3].map(() => ({ playing: true })),
    } as EngineStatus);
    deckCount(4);
    TestBed.inject(ControllerService).mappings.set([{ target: 'deck3.play' } as ControllerMapping]);
    TestBed.tick();
    await Promise.resolve();
    expect(sent).toEqual([{ target: 'deck3.play', state: 'on', color: null }]);

    deckCount(2);
    TestBed.tick();
    await Promise.resolve();
    expect(sent.at(-1)).toEqual({ target: 'deck3.play', state: 'off', color: null });
  });
});

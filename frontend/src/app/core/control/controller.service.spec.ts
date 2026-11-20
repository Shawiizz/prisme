import { WritableSignal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { FxService } from '../services/fx.service';
import { PreferencesService } from '../services/preferences.service';
import { ControllerService } from './controller.service';

describe('ControllerService', () => {
  afterEach(() => clearMocks());

  it('tells the lights of what it keeps, out on a deck no longer shown', async () => {
    const sent: unknown[] = [];
    mockIPC((command, args) => {
      if (command === 'set_lights') sent.push(...(args as { lights: unknown[] }).lights);
    });
    const preferences = TestBed.inject(PreferencesService);
    const deckCount = (n: number) => preferences.dj.update((dj) => ({ ...dj, deckCount: n }));
    deckCount(4);
    TestBed.inject(FxService).toggleFocus(2, 0);
    const controllers = TestBed.inject(ControllerService) as unknown as {
      lightTargets: WritableSignal<string[]>;
    };
    controllers.lightTargets.set(['deck3.fx.focus']);
    TestBed.tick();
    await Promise.resolve();
    expect(sent).toEqual([{ target: 'deck3.fx.focus', state: 'on' }]);

    deckCount(2);
    TestBed.tick();
    await Promise.resolve();
    expect(sent.at(-1)).toEqual({ target: 'deck3.fx.focus', state: 'off' });
  });
});

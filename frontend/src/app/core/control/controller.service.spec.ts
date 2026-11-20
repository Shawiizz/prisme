import { WritableSignal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { NavigationService } from '../services/navigation.service';
import { ControllerService } from './controller.service';

describe('ControllerService', () => {
  afterEach(() => clearMocks());

  it('tells the lights of what it keeps as they change', async () => {
    const sent: unknown[] = [];
    mockIPC((command, args) => {
      if (command === 'set_lights') sent.push(...(args as { lights: unknown[] }).lights);
    });
    const navigation = TestBed.inject(NavigationService);
    const controllers = TestBed.inject(ControllerService) as unknown as {
      lightTargets: WritableSignal<string[]>;
    };
    controllers.lightTargets.set(['general.library']);
    TestBed.tick();
    await Promise.resolve();
    expect(sent).toEqual([{ target: 'general.library', state: 'off' }]);

    navigation.libraryOnly.set(true);
    TestBed.tick();
    await Promise.resolve();
    expect(sent.at(-1)).toEqual({ target: 'general.library', state: 'on' });
  });
});

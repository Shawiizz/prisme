import { WritableSignal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { emit } from '@tauri-apps/api/event';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { LibraryService } from '../services/library.service';
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

  it('tells the backend the track a controller browsed to as soon as it moved there', async () => {
    const told: unknown[] = [];
    mockIPC(
      (command, args) => {
        if (command === 'set_library_selection') told.push(args);
        if (command === 'controller_mappings' || command === 'waiting_controllers') return [];
        if (command === 'controller_light_targets') return [];
        return null;
      },
      { shouldMockEvents: true },
    );
    const library = TestBed.inject(LibraryService);
    const tracks = ['a.mp3', 'b.mp3'].map((name) => ({
      name,
      path: `music/${name}`,
      details: null,
    }));
    library.listing.set({ path: 'music', parent: null, folders: [], tracks });
    library.selected.set('music/a.mp3');
    await TestBed.inject(ControllerService).start();
    await emit('ui-action', { target: 'library.next', value: 1, number: 7 });
    expect(told.at(-1)).toEqual({ path: 'music/b.mp3', applied: 7 });
  });
});

import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { EngineService } from '../services/engine.service';
import { LibraryService } from '../services/library.service';
import { NavigationService } from '../services/navigation.service';
import { ControlService } from './control.service';
import type { EngineStatus } from '../models/backend';

describe('ControlService', () => {
  let performed: unknown[];

  beforeEach(() => {
    performed = [];
    TestBed.inject(EngineService).status.set({ decks: [{}, {}] } as EngineStatus);
    mockIPC((command, args) => {
      if (command === 'perform_action') {
        const { target, value } = args as { target: string; value: number };
        performed.push([target, value]);
      }
    });
  });

  afterEach(() => clearMocks());

  it('has the backend perform an action as it is given, an encoder its steps', () => {
    const controls = TestBed.inject(ControlService);
    controls.act('deck1.play', 1);
    controls.act('deck1.play', 0);
    controls.act('deck2.eq.low', 0.25);
    controls.act('deck1.loop.move', -1);
    controls.act('deck2.jump.-8', 1);
    expect(performed).toEqual([
      ['deck1.play', 1],
      ['deck1.play', 0],
      ['deck2.eq.low', 0.25],
      ['deck1.loop.move', -1],
      ['deck2.jump.-8', 1],
    ]);
  });

  it('has the backend browse the library, and opens its windows itself', () => {
    const controls = TestBed.inject(ControlService);
    const library = TestBed.inject(LibraryService);
    const navigation = TestBed.inject(NavigationService);
    controls.act('library.browse', -3);
    controls.act('library.next', 1);
    controls.act('library.search', 1);
    controls.act('general.library', 1);
    controls.act('general.palette', 1);
    expect(performed).toEqual([
      ['library.browse', -3],
      ['library.next', 1],
      ['general.library', 1],
    ]);
    expect(library.searchRequests()).toBe(1);
    expect(navigation.palette()).toBe(true);
  });
});

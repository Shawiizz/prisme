import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { EngineStatus } from '../models/engine';
import { EngineService } from '../services/engine.service';
import { LibraryService } from '../services/library.service';
import { NavigationService } from '../services/navigation.service';
import { ControlService } from './control.service';

describe('ControlService', () => {
  let performed: unknown[];

  beforeEach(() => {
    performed = [];
    TestBed.inject(EngineService).status.set({ decks: [{}, {}] } as EngineStatus);
    mockIPC((command, args) => {
      if (command === 'perform_action') {
        const { target, value, steps } = args as { target: string; value: number; steps: boolean };
        performed.push([target, value, steps]);
      }
    });
  });

  afterEach(() => clearMocks());

  it('has the backend perform an action, an encoder giving its steps', () => {
    const controls = TestBed.inject(ControlService);
    controls.act('deck1.play', 1);
    controls.act('deck1.play', 0);
    controls.act('deck2.eq.low', 0.25);
    controls.act('deck1.loop.move', -1);
    controls.act('deck2.jump.-8', 1);
    expect(performed).toEqual([
      ['deck1.play', 1, false],
      ['deck1.play', 0, false],
      ['deck2.eq.low', 0.25, false],
      ['deck1.loop.move', -1, true],
      ['deck2.jump.-8', 1, false],
    ]);
  });

  it('does nothing for a deck not shown, an action it does not know, a light or shift', () => {
    const controls = TestBed.inject(ControlService);
    controls.act('deck3.play', 1);
    controls.act('deck1.nothing', 1);
    controls.act('deck1.beat', 1);
    controls.act('shift', 1);
    expect(performed).toEqual([]);
  });

  it('has the backend browse the library, and opens its windows itself', () => {
    const controls = TestBed.inject(ControlService);
    const library = TestBed.inject(LibraryService);
    const navigation = TestBed.inject(NavigationService);
    controls.act('library.browse', -3);
    controls.act('library.next', 1);
    controls.act('library.search', 1);
    controls.act('general.library', 1);
    expect(performed).toEqual([
      ['library.browse', -3, true],
      ['library.next', 1, false],
    ]);
    expect(library.searchRequests()).toBe(1);
    expect(navigation.libraryOnly()).toBe(true);
    expect(controls.lit('general.library')).toBe(true);
    expect(controls.lit('deck1.play')).toBeUndefined();
  });
});

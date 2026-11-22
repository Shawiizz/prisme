import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { EngineService } from '../services/engine.service';
import { LibraryService } from '../services/library.service';
import { NavigationService } from '../services/navigation.service';
import { ControlService } from './control.service';
import { OverlayService } from '../../shared/overlay';
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
    expect(library.searchWanted()).toBe(true);
    expect(navigation.libraryWanted()).toEqual({ sight: 'show' });
    expect(navigation.palette()).toBe(true);
  });

  it('closes what is open on top, as Escape does', () => {
    const controls = TestBed.inject(ControlService);
    const overlays = TestBed.inject(OverlayService);
    const closed: string[] = [];
    overlays.opened(() => closed.push('settings'));
    const question = overlays.opened(() => closed.push('question'));
    controls.act('general.close', 1);
    controls.act('general.close', 0);
    expect(closed).toEqual(['question']);
    question();
    controls.act('general.close', 1);
    expect(closed).toEqual(['question', 'settings']);
    expect(performed).toEqual([]);
  });
});

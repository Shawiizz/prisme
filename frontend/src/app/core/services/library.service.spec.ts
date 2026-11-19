import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { DirEntry, DirListing } from '../models/library';
import { EngineService } from './engine.service';
import { HistoryService } from './history.service';
import { LibraryService } from './library.service';

const track = (name: string): DirEntry => ({ name, path: `/music/${name}`, details: null });

describe('LibraryService', () => {
  afterEach(() => clearMocks());

  it('keeps the mix order while the next deck is mixed, its tempo range alone counting', async () => {
    const listing: DirListing = {
      path: '/music',
      parent: null,
      folders: [],
      tracks: [track('a.mp3'), track('b.mp3')],
    };
    mockIPC((command) => (command === 'list_dir' ? listing : undefined));
    const engine = TestBed.inject(EngineService);
    const library = TestBed.inject(LibraryService);
    await library.open('/music');
    library.sortBy('mix');
    TestBed.tick();

    const sorted = library.visibleTracks();
    engine.patchDeck(1, { volume: 0.5 });
    expect(library.visibleTracks()).toBe(sorted);
    engine.patchDeck(1, { pitchRange: 0.5 });
    expect(library.visibleTracks()).not.toBe(sorted);
  });

  /** The lists answered each when the test lets it go, in any order; the rest at once. */
  function answering() {
    const waiting = new Map<string, () => void>();
    mockIPC((command, args) => {
      const { path, id } = (args ?? {}) as { path?: string; id?: number };
      if (command === 'list_dir')
        return new Promise((done) =>
          waiting.set(`${command} ${path}`, () =>
            done({ path, parent: null, folders: [], tracks: [track(`${path}.mp3`)] }),
          ),
        );
      if (command === 'crate_tracks' || command === 'history_plays')
        return new Promise((done) =>
          waiting.set(`${command} ${id}`, () => done([track(`${command} ${id}`)])),
        );
      return undefined;
    });
    return async (list: string) => {
      waiting.get(list)!();
      waiting.delete(list);
      await new Promise((done) => setTimeout(done));
    };
  }

  it('shows the folder opened last, whichever is listed first', async () => {
    const answer = answering();
    const library = TestBed.inject(LibraryService);
    const big = library.open('/big');
    const small = library.open('/small');
    await answer('list_dir /small');
    await answer('list_dir /big');
    await Promise.all([big, small]);
    expect(library.listing()?.path).toBe('/small');
  });

  it('shows the crate opened last, not the plays of the session browsed before', async () => {
    const answer = answering();
    const library = TestBed.inject(LibraryService);
    const session = library.openSession(7);
    await answer('history_plays 7');
    await session;
    TestBed.inject(HistoryService).changes.update((n) => n + 1);
    TestBed.tick();
    const crate = library.openCrate(3);
    await answer('crate_tracks 3');
    await answer('history_plays 7');
    await crate;
    expect(library.currentCrate()).toBe(3);
    expect(library.listTracks().map((t) => t.name)).toEqual(['crate_tracks 3']);
  });
});

import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { DirEntry, DirListing } from '../models/library';
import { EngineService } from './engine.service';
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
});

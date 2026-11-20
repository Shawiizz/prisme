import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { LibraryView, LibraryWindow } from '../models/library';
import { LibraryService } from './library.service';

const window = (seq: number, first: number): LibraryWindow => ({ seq, first, rows: [] });

const view = (seq: number, filter: string): LibraryView => ({
  seq,
  source: { kind: 'prepare' },
  folder: null,
  scope: 'list',
  filter,
  sort: { column: 'order', descending: false },
  ordered: true,
  removable: true,
  reorderable: true,
  count: 0,
  cursor: null,
  cursorMoves: 0,
  selected: 0,
  focus: 'tracks',
  pointed: { kind: 'prepare' },
  reference: 0,
  unread: null,
  window: window(seq, 0),
});

describe('LibraryService', () => {
  afterEach(() => clearMocks());

  it('shows the list and the rows the backend told last, whichever answer comes first', async () => {
    const answers: Record<string, unknown> = {
      library_filter: view(4, 'deep'),
      library_viewport: window(3, 40),
    };
    mockIPC((command) => answers[command]);
    const library = TestBed.inject(LibraryService);
    await library.setFilter('deep');
    expect(library.view()?.filter).toBe('deep');
    await library.viewport(40, 30);
    // a window asked for before the search: left aside
    expect(library.window().seq).toBe(4);
    answers['library_viewport'] = window(5, 40);
    await library.viewport(40, 30);
    expect(library.window().first).toBe(40);
  });

  it('shows what is typed while the search has the focus, then the search applied', async () => {
    mockIPC((command, args) =>
      command === 'library_filter' ? view(1, (args as { text: string }).text.slice(0, 2)) : null,
    );
    const library = TestBed.inject(LibraryService);
    await library.setFilter('dee');
    expect(library.filter()).toBe('dee');
    library.endTyping();
    expect(library.filter()).toBe('de');
  });
});

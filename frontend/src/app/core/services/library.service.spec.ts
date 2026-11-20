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
  listed: 0,
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

  it('asks for each window on a clock, and keeps rows told since over an answer as old', async () => {
    const asked: number[] = [];
    mockIPC((command, args) => {
      if (command === 'library_viewport') asked.push((args as { asked: number }).asked);
      return window(7, 40);
    });
    const library = TestBed.inject(LibraryService);
    await library.viewport(40, 30);
    await library.viewport(80, 30);
    expect(asked[1]).toBeGreaterThanOrEqual(asked[0]);
    const told = { ...window(7, 40), rows: [{ path: 'told' } as never] };
    library['take']({ ...view(7, ''), window: told });
    await library.viewport(40, 30);
    expect(library.window()).toBe(told);
  });

  it('forgets what was typed once another list is shown', async () => {
    mockIPC((command) => (command === 'library_filter' ? view(1, 'de') : null));
    const library = TestBed.inject(LibraryService);
    await library.setFilter('dee');
    library['take']({ ...view(2, ''), source: { kind: 'crate', id: 3 } });
    expect(library.filter()).toBe('');
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

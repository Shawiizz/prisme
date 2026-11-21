import { cursorMoved } from './track-table';
import type { LibraryView } from '../../../core/models/backend';

const view = (cursor: number, cursorMoves: number, listed = 1): LibraryView => ({
  seq: 1,
  source: { kind: 'prepare' },
  folder: null,
  scope: 'list',
  filter: '',
  sort: { column: 'order', descending: false },
  ordered: true,
  reorderable: true,
  count: 40,
  cursor,
  cursorMoves,
  listed,
  selected: 1,
  focus: 'tracks',
  pointed: { kind: 'prepare' },
  reference: 0,
  unread: null,
  window: { seq: 1, first: 0, rows: [] },
});

describe('cursorMoved', () => {
  it('follows the cursor going to another row, not its row moving', () => {
    expect(cursorMoved(view(3, 1), view(4, 2))).toBe(true);
    expect(cursorMoved(view(3, 1), view(9, 1))).toBe(false);
  });

  it('leaves a list searched or opened again alone', () => {
    expect(cursorMoved(view(3, 1), view(0, 2, 2))).toBe(false);
    expect(cursorMoved(null, view(0, 2))).toBe(false);
  });
});

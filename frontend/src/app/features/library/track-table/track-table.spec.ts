import { LibraryView } from '../../../core/models/library';
import { cursorMoved } from './track-table';

const view = (cursor: number, cursorMoves: number, filter = ''): LibraryView => ({
  seq: 1,
  source: { kind: 'prepare' },
  folder: null,
  scope: 'list',
  filter,
  sort: { column: 'order', descending: false },
  ordered: true,
  removable: true,
  reorderable: true,
  count: 40,
  cursor,
  cursorMoves,
  selected: 1,
  focus: 'tracks',
  pointed: { kind: 'prepare' },
  reference: 0,
  window: { seq: 1, first: 0, rows: [] },
});

describe('cursorMoved', () => {
  it('follows the cursor going to another row, not its row moving', () => {
    expect(cursorMoved(view(3, 1), view(4, 2))).toBe(true);
    expect(cursorMoved(view(3, 1), view(9, 1))).toBe(false);
  });

  it('leaves a list searched or opened again alone', () => {
    expect(cursorMoved(view(3, 1), view(0, 2, 'deep'))).toBe(false);
    expect(cursorMoved(null, view(0, 2))).toBe(false);
  });
});

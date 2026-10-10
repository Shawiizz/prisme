import { SortColumn } from '../../../core/services/library.service';

export type ColumnId =
  | 'color'
  | 'cover'
  | 'title'
  | 'artist'
  | 'album'
  | 'genre'
  | 'year'
  | 'bpm'
  | 'key'
  | 'rating'
  | 'plays';

export interface Column {
  id: ColumnId;
  /** In the column menu. */
  name: string;
  /** In the header; empty for the narrow columns. */
  header: string;
  /** Grid track. */
  width: string;
  sort?: SortColumn;
  /** Always shown. */
  fixed?: boolean;
}

export const COLUMNS: readonly Column[] = [
  { id: 'color', name: 'Colour', header: '', width: '10px' },
  { id: 'cover', name: 'Cover', header: '', width: '24px' },
  {
    id: 'title',
    name: 'Title',
    header: 'Title',
    width: 'minmax(0, 1.6fr)',
    sort: 'title',
    fixed: true,
  },
  { id: 'artist', name: 'Artist', header: 'Artist', width: 'minmax(0, 1fr)', sort: 'artist' },
  { id: 'album', name: 'Album', header: 'Album', width: 'minmax(0, 1fr)', sort: 'album' },
  { id: 'genre', name: 'Genre', header: 'Genre', width: 'minmax(0, 0.6fr)', sort: 'genre' },
  { id: 'year', name: 'Year', header: 'Year', width: '40px', sort: 'year' },
  { id: 'bpm', name: 'BPM', header: 'BPM', width: '56px', sort: 'bpm' },
  { id: 'key', name: 'Key', header: 'Key', width: '52px', sort: 'key' },
  { id: 'rating', name: 'Rating', header: 'Rating', width: '74px', sort: 'rating' },
  { id: 'plays', name: 'Plays', header: 'Plays', width: '40px', sort: 'plays' },
];

/** Grid tracks of a row: the play order of ordered lists, the columns, the load buttons. */
export function gridTemplate(columns: readonly Column[], ordered: boolean): string {
  return [...(ordered ? ['30px'] : []), ...columns.map((c) => c.width), '64px'].join(' ');
}

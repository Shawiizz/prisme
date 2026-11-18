import { SortColumn } from '../../../core/services/library.service';
import { Text, t } from '../../../core/i18n/i18n';

export type ColumnId =
  | 'color'
  | 'cover'
  | 'title'
  | 'artist'
  | 'album'
  | 'genre'
  | 'year'
  | 'bpm'
  | 'pitch'
  | 'key'
  | 'rating'
  | 'plays'
  | 'stems';

export interface Column {
  id: ColumnId;
  /** In the column menu. */
  name: Text;
  /** In the header; none for the narrow columns. */
  header?: Text;
  /** Grid track. */
  width: string;
  sort?: SortColumn;
  /** Always shown. */
  fixed?: boolean;
  /** Shown on a wide table only: a narrow one gives its room to the titles. */
  wide?: boolean;
}

export const COLUMNS: readonly Column[] = [
  {
    id: 'color',
    name: () => t('Colour'),
    width: '10px',
  },
  {
    id: 'cover',
    name: () => t('Cover'),
    width: '24px',
  },
  {
    id: 'title',
    name: () => t('Title'),
    header: () => t('Title'),
    width: 'minmax(120px, 1.6fr)',
    sort: 'title',
    fixed: true,
  },
  {
    id: 'artist',
    name: () => t('Artist'),
    header: () => t('Artist'),
    width: 'minmax(72px, 1fr)',
    sort: 'artist',
  },
  {
    id: 'album',
    name: () => t('Album'),
    header: () => t('Album'),
    width: 'minmax(72px, 1fr)',
    sort: 'album',
  },
  {
    id: 'genre',
    name: () => t('Genre'),
    header: () => t('Genre'),
    width: 'minmax(56px, 0.6fr)',
    sort: 'genre',
  },
  {
    id: 'year',
    name: () => t('Year'),
    header: () => t('Year'),
    width: '40px',
    sort: 'year',
    wide: true,
  },
  { id: 'bpm', name: () => 'BPM', header: () => 'BPM', width: '60px', sort: 'bpm' },
  // its header names the reference deck
  {
    id: 'pitch',
    name: () => t('Pitch to match'),
    width: '72px',
    sort: 'mix',
  },
  {
    id: 'key',
    name: () => t('Key'),
    header: () => t('Key'),
    width: '52px',
    sort: 'key',
  },
  {
    id: 'rating',
    name: () => t('Rating'),
    header: () => t('Rating'),
    width: '74px',
    sort: 'rating',
  },
  {
    id: 'plays',
    name: () => t('Plays'),
    header: () => t('Plays'),
    width: '52px',
    sort: 'plays',
    wide: true,
  },
  {
    id: 'stems',
    name: () => t('Separated stems'),
    header: () => 'STEMS',
    width: '48px',
    wide: true,
  },
];

/** Grid tracks of a row: the play order of ordered lists, the columns, the preview key and a
 * load key for each of the `decks`. */
export function gridTemplate(columns: readonly Column[], ordered: boolean, decks: number): string {
  const load = `${(decks + 1) * 28 + 2}px`;
  return [...(ordered ? ['30px'] : []), ...columns.map((c) => c.width), load].join(' ');
}

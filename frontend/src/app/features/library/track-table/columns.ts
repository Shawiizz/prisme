import { SortColumn } from '../../../core/services/library.service';
import { t } from '../../../core/i18n/i18n';

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
  {
    id: 'color',
    get name() {
      return t('Colour');
    },
    header: '',
    width: '10px',
  },
  {
    id: 'cover',
    get name() {
      return t('Cover');
    },
    header: '',
    width: '24px',
  },
  {
    id: 'title',
    get name() {
      return t('Title');
    },
    get header() {
      return t('Title');
    },
    width: 'minmax(0, 1.6fr)',
    sort: 'title',
    fixed: true,
  },
  {
    id: 'artist',
    get name() {
      return t('Artist');
    },
    get header() {
      return t('Artist');
    },
    width: 'minmax(0, 1fr)',
    sort: 'artist',
  },
  {
    id: 'album',
    get name() {
      return t('Album');
    },
    get header() {
      return t('Album');
    },
    width: 'minmax(0, 1fr)',
    sort: 'album',
  },
  {
    id: 'genre',
    get name() {
      return t('Genre');
    },
    get header() {
      return t('Genre');
    },
    width: 'minmax(0, 0.6fr)',
    sort: 'genre',
  },
  {
    id: 'year',
    get name() {
      return t('Year');
    },
    get header() {
      return t('Year');
    },
    width: '40px',
    sort: 'year',
  },
  { id: 'bpm', name: 'BPM', header: 'BPM', width: '60px', sort: 'bpm' },
  // its header names the reference deck
  {
    id: 'pitch',
    get name() {
      return t('Pitch to match');
    },
    header: '',
    width: '72px',
    sort: 'mix',
  },
  {
    id: 'key',
    get name() {
      return t('Key');
    },
    get header() {
      return t('Key');
    },
    width: '52px',
    sort: 'key',
  },
  {
    id: 'rating',
    get name() {
      return t('Rating');
    },
    get header() {
      return t('Rating');
    },
    width: '74px',
    sort: 'rating',
  },
  {
    id: 'plays',
    get name() {
      return t('Plays');
    },
    get header() {
      return t('Plays');
    },
    width: '52px',
    sort: 'plays',
  },
  {
    id: 'stems',
    get name() {
      return t('Separated stems');
    },
    header: 'STEMS',
    width: '48px',
  },
];

/** Grid tracks of a row: the play order of ordered lists, the columns, a load button for
 * each of the `decks`. */
export function gridTemplate(columns: readonly Column[], ordered: boolean, decks: number): string {
  const load = `${decks * 28 + 2}px`;
  return [...(ordered ? ['30px'] : []), ...columns.map((c) => c.width), load].join(' ');
}

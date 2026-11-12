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
  { id: 'color', name: t('Colour'), header: '', width: '10px' },
  { id: 'cover', name: t('Cover'), header: '', width: '24px' },
  {
    id: 'title',
    name: t('Title'),
    header: t('Title'),
    width: 'minmax(0, 1.6fr)',
    sort: 'title',
    fixed: true,
  },
  { id: 'artist', name: t('Artist'), header: t('Artist'), width: 'minmax(0, 1fr)', sort: 'artist' },
  { id: 'album', name: t('Album'), header: t('Album'), width: 'minmax(0, 1fr)', sort: 'album' },
  { id: 'genre', name: t('Genre'), header: t('Genre'), width: 'minmax(0, 0.6fr)', sort: 'genre' },
  { id: 'year', name: t('Year'), header: t('Year'), width: '40px', sort: 'year' },
  { id: 'bpm', name: 'BPM', header: 'BPM', width: '60px', sort: 'bpm' },
  // its header names the reference deck
  { id: 'pitch', name: t('Pitch to match'), header: '', width: '72px', sort: 'mix' },
  { id: 'key', name: t('Key'), header: t('Key'), width: '52px', sort: 'key' },
  { id: 'rating', name: t('Rating'), header: t('Rating'), width: '74px', sort: 'rating' },
  { id: 'plays', name: t('Plays'), header: t('Plays'), width: '52px', sort: 'plays' },
  { id: 'stems', name: t('Separated stems'), header: 'STEMS', width: '48px' },
];

/** Grid tracks of a row: the play order of ordered lists, the columns, a load button for
 * each of the `decks`. */
export function gridTemplate(columns: readonly Column[], ordered: boolean, decks: number): string {
  const load = `${decks * 28 + 2}px`;
  return [...(ordered ? ['30px'] : []), ...columns.map((c) => c.width), load].join(' ');
}

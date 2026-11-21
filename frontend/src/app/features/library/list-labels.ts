import { Text, t } from '../../core/i18n/i18n';
import { errorText } from '../../core/errors';
import type { LibraryView, TrackSource } from '../../core/models/backend';

/** What an empty list says; the prepare list's names the key that adds to it, if it has one. */
const EMPTY_LIST: Record<TrackSource['kind'], (key: string | null) => string> = {
  folder: () => t('No audio file in this folder'),
  crate: () => t('This crate is empty: drag tracks onto it'),
  history: () => t('No track in this set'),
  prepare: (key) =>
    key
      ? t('Drag tracks here or press {key} to line them up for the set', { key })
      : t('Drag tracks here to line them up for the set'),
  duplicates: () => t('No two tracks share an artist and a title'),
  elsewhere: () => t('Every track is in your folders'),
};

/** What the list `view` says when it shows no track, searched for `filter`: why the music
 * folder could not be read, else what its kind of list says (`prepareKey`, the key adding to
 * the prepare list). */
export function emptyText(view: LibraryView | null, filter: string, prepareKey: string | null) {
  if (view?.unread)
    return t('The music folder could not be read: {why}', { why: errorText(view.unread) });
  if ((view?.scope ?? 'list') === 'list')
    return EMPTY_LIST[view?.source.kind ?? 'folder'](prepareKey);
  return filter.trim() ? t('No analysed track matches') : t('Type to search every analysed track');
}

/** What each kind of list is called. */
export const LIST_LABEL: Record<TrackSource['kind'], Text> = {
  folder: () => t('Folder'),
  crate: () => t('Crate'),
  history: () => t('Set'),
  prepare: () => t('Prepare'),
  duplicates: () => t('Duplicates'),
  elsewhere: () => t('Elsewhere'),
};
/** Whole sentences: the words around the list's name change with it in other languages. */
export const SEARCH_PLACEHOLDER: Record<TrackSource['kind'], Text> = {
  folder: () => t('Search in this folder'),
  crate: () => t('Search in this crate'),
  history: () => t('Search in this set'),
  prepare: () => t('Search in the prepare list'),
  duplicates: () => t('Search in the duplicates'),
  elsewhere: () => t('Search in the tracks from elsewhere'),
};

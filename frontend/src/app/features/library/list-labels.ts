import { TrackSource } from '../../core/models/library';
import { Text, t } from '../../core/i18n/i18n';

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

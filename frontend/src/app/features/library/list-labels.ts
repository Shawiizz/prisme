import { TrackSource } from '../../core/services/library.service';
import { t } from '../../core/i18n/i18n';

/** What each kind of list is called. */
export const LIST_LABEL: Record<TrackSource['kind'], string> = {
  get folder() {
    return t('Folder');
  },
  get crate() {
    return t('Crate');
  },
  get history() {
    return t('Set');
  },
  get prepare() {
    return t('Prepare');
  },
  get duplicates() {
    return t('Duplicates');
  },
};
/** Whole sentences: the words around the list's name change with it in other languages. */
export const SEARCH_PLACEHOLDER: Record<TrackSource['kind'], string> = {
  get folder() {
    return t('Search in this folder');
  },
  get crate() {
    return t('Search in this crate');
  },
  get history() {
    return t('Search in this set');
  },
  get prepare() {
    return t('Search in the prepare list');
  },
  get duplicates() {
    return t('Search in the duplicates');
  },
};

import { Component, ElementRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { LibraryService, TrackSource } from '../../core/services/library.service';
import { ImportDialog } from './import-dialog/import-dialog';
import { LibrarySidebar } from './library-sidebar/library-sidebar';
import { TrackTable } from './track-table/track-table';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';

const LIST_LABEL: Record<TrackSource['kind'], string> = {
  folder: t('Folder'),
  crate: t('Crate'),
  history: t('Set'),
  prepare: t('Prepare'),
  duplicates: t('Duplicates'),
};
/** Whole sentences: the words around the list's name change with it in other languages. */
const SEARCH_PLACEHOLDER: Record<TrackSource['kind'], string> = {
  folder: t('Search in this folder'),
  crate: t('Search in this crate'),
  history: t('Search in this set'),
  prepare: t('Search in the prepare list'),
  duplicates: t('Search in the duplicates'),
};

/** Crates, history and folders (left); search, analysis progress and tracks (right). */
@Component({
  selector: 'app-library',
  templateUrl: './library.html',
  styleUrl: './library.scss',
  imports: [ImportDialog, LibrarySidebar, TrackTable, TPipe],
})
export class Library {
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  protected readonly tracks = this.library.visibleTracks;
  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('search');

  protected readonly importOpen = signal(false);
  protected readonly progress = this.library.progress;
  protected readonly progressPercent = computed(() => {
    const p = this.progress();
    return p.total > 0 ? (p.done / p.total) * 100 : 0;
  });

  protected readonly listLabel = computed(() => LIST_LABEL[this.library.source().kind]);
  protected readonly searchPlaceholder = computed(
    () => SEARCH_PLACEHOLDER[this.library.source().kind],
  );

  constructor() {
    effect(() => {
      if (this.library.searchRequests() > 0) this.search().nativeElement.focus();
    });
  }
}

import { Component, ElementRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { LibraryService, TrackSource } from '../../core/services/library.service';
import { ImportDialog } from './import-dialog/import-dialog';
import { LibrarySidebar } from './library-sidebar/library-sidebar';
import { TrackTable } from './track-table/track-table';

const LIST_LABEL: Record<TrackSource['kind'], string> = {
  folder: 'Folder',
  crate: 'Crate',
  history: 'Set',
  prepare: 'Prepare',
};

/** Crates, history and folders (left); search, analysis progress and tracks (right). */
@Component({
  selector: 'app-library',
  templateUrl: './library.html',
  styleUrl: './library.scss',
  imports: [ImportDialog, LibrarySidebar, TrackTable],
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

  constructor() {
    effect(() => {
      if (this.library.searchRequests() > 0) this.search().nativeElement.focus();
    });
  }
}

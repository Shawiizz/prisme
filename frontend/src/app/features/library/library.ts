import { Component, ElementRef, computed, effect, inject, viewChild } from '@angular/core';
import { LibraryService, SortColumn, trackTitle } from '../../core/services/library.service';
import { EngineService } from '../../core/services/engine.service';
import { DECK_COUNT } from '../../core/models/engine';
import { deckColor } from '../../shared/time';
import { camelotColor, camelotCompatible } from '../../shared/camelot';
import { Cover } from '../../shared/cover/cover';
import { TRACKS_DRAG_TYPE } from '../../shared/drag';
import { LibrarySidebar } from './library-sidebar/library-sidebar';

const COLUMNS: { id: SortColumn; label: string }[] = [
  { id: 'title', label: 'Title' },
  { id: 'artist', label: 'Artist' },
  { id: 'album', label: 'Album' },
  { id: 'genre', label: 'Genre' },
  { id: 'year', label: 'Year' },
  { id: 'bpm', label: 'BPM' },
  { id: 'key', label: 'Key' },
];

/** Crates and folders (left), the tracks being browsed or found (right). */
@Component({
  selector: 'app-library',
  templateUrl: './library.html',
  styleUrl: './library.scss',
  imports: [Cover, LibrarySidebar],
})
export class Library {
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  protected readonly deckIndexes = Array.from({ length: DECK_COUNT }, (_, i) => i);
  protected readonly color = deckColor;
  protected readonly keyColor = camelotColor;
  protected readonly columns = COLUMNS;
  protected readonly title = trackTitle;
  protected readonly tracks = this.library.visibleTracks;
  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('search');
  private readonly host = inject(ElementRef<HTMLElement>);

  /** Keys of the tracks on the decks, to point out the harmonic matches. */
  private readonly deckKeys = computed(() =>
    this.engine
      .tracks()
      .map((t) => t?.key?.camelot)
      .filter((k): k is string => !!k),
  );

  protected readonly progress = this.library.progress;
  protected readonly progressPercent = computed(() => {
    const p = this.progress();
    return p.total > 0 ? (p.done / p.total) * 100 : 0;
  });

  protected readonly listLabel = computed(() =>
    this.library.source().kind === 'crate' ? 'Crate' : 'Folder',
  );

  protected readonly emptyMessage = computed(() => {
    if (this.library.scope() === 'list') {
      return this.library.source().kind === 'crate'
        ? 'This crate is empty: drag tracks onto it'
        : 'No audio file in this folder';
    }
    return this.library.filter().trim()
      ? 'No analysed track matches'
      : 'Type to search every analysed track';
  });

  constructor() {
    effect(() => {
      if (this.library.searchRequests() > 0) this.search().nativeElement.focus();
    });
    effect(() => {
      const path = this.library.selected();
      const row =
        path && this.host.nativeElement.querySelector(`[data-path="${CSS.escape(path)}"]`);
      row?.scrollIntoView({ block: 'nearest' });
    });
  }

  sortMark(column: SortColumn): string {
    const s = this.library.sort();
    return s.column === column ? (s.direction === 1 ? '▲' : '▼') : '';
  }

  /** Whether the key mixes with a loaded track (`null` when no deck has a key). */
  keyMatches(camelot: string): boolean | null {
    const keys = this.deckKeys();
    return keys.length ? keys.some((k) => camelotCompatible(k, camelot)) : null;
  }

  dragStart(event: DragEvent, path: string) {
    this.library.selected.set(path);
    event.dataTransfer?.setData(TRACKS_DRAG_TYPE, JSON.stringify([path]));
  }
}

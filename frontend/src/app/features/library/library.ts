import { Component, ElementRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import {
  LibraryService,
  SortColumn,
  TrackSource,
  trackTitle,
} from '../../core/services/library.service';
import { EngineService } from '../../core/services/engine.service';
import { DECK_COUNT } from '../../core/models/engine';
import { DirEntry } from '../../core/models/library';
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

const SOURCE_TEXT: Record<TrackSource['kind'], { label: string; empty: string }> = {
  folder: { label: 'Folder', empty: 'No audio file in this folder' },
  crate: { label: 'Crate', empty: 'This crate is empty: drag tracks onto it' },
  history: { label: 'Set', empty: 'No track in this set' },
  prepare: {
    label: 'Prepare',
    empty: 'Drag tracks here or press Ctrl + P to line them up for the set',
  },
};

/** Crates, history and folders (left), the tracks being browsed or found (right). */
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

  /** Row a dragged track would be put before (`null`: the end of the list). */
  protected readonly insertBefore = signal<DirEntry | null>(null);
  /** The prepare list, shown in its own order, is reordered by dragging. */
  private readonly reorderable = computed(() => {
    const sort = this.library.sort();
    return (
      this.library.source().kind === 'prepare' &&
      this.library.scope() === 'list' &&
      sort.column === 'order' &&
      sort.direction === 1
    );
  });

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

  protected readonly listLabel = computed(() => SOURCE_TEXT[this.library.source().kind].label);

  protected readonly emptyMessage = computed(() => {
    if (this.library.scope() === 'list') return SOURCE_TEXT[this.library.source().kind].empty;
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

  /** A set can hold the same track twice: its rows are told apart by play time. */
  rowKey(t: DirEntry): string {
    return 'playedAtMs' in t ? `${t.path}@${t.playedAtMs}` : t.path;
  }

  dragStart(event: DragEvent, path: string) {
    this.library.selected.set(path);
    event.dataTransfer?.setData(TRACKS_DRAG_TYPE, JSON.stringify([path]));
  }

  listDragOver(event: DragEvent, before: DirEntry | null) {
    if (!this.reorderable() || !event.dataTransfer?.types.includes(TRACKS_DRAG_TYPE)) return;
    event.preventDefault();
    event.stopPropagation();
    this.insertBefore.set(before);
  }

  listDrop(event: DragEvent, before: DirEntry | null) {
    this.insertBefore.set(null);
    const data = event.dataTransfer?.getData(TRACKS_DRAG_TYPE);
    if (!this.reorderable() || !data) return;
    event.preventDefault();
    event.stopPropagation();
    const [path] = JSON.parse(data) as string[];
    const list = this.library.listTracks();
    const from = list.findIndex((t) => t.path === path);
    let to = before ? list.indexOf(before) : list.length;
    // the moved track leaves its place first: the ones after it move up by one
    if (from >= 0 && from < to) to -= 1;
    this.library.placeInPrepare(path, to);
  }
}

import { Component, ElementRef, computed, effect, inject, signal } from '@angular/core';
import { DECK_COUNT } from '../../../core/models/engine';
import { DirEntry } from '../../../core/models/library';
import { EngineService } from '../../../core/services/engine.service';
import {
  LibraryService,
  SortColumn,
  TrackSource,
  trackTitle,
} from '../../../core/services/library.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { camelotColor, camelotCompatible } from '../../../shared/camelot';
import { Cover } from '../../../shared/cover/cover';
import { TRACKS_DRAG_TYPE } from '../../../shared/drag';
import { deckColor } from '../../../shared/time';
import { TRACK_COLORS, hexColor } from '../../../shared/track-colors';
import { COLUMNS, Column, ColumnId, gridTemplate } from './columns';

const EMPTY_LIST: Record<TrackSource['kind'], string> = {
  folder: 'No audio file in this folder',
  crate: 'This crate is empty: drag tracks onto it',
  history: 'No track in this set',
  prepare: 'Drag tracks here or press Ctrl + P to line them up for the set',
};

const STARS = [1, 2, 3, 4, 5];

/** A popover opened at the pointer: below it, or above it in the lower half of the window. */
interface Popover {
  left: number;
  top: number | null;
  bottom: number | null;
}

function popoverAt(event: MouseEvent): Popover {
  const below = event.clientY < window.innerHeight / 2;
  return {
    left: event.clientX,
    top: below ? event.clientY : null,
    bottom: below ? null : window.innerHeight - event.clientY,
  };
}

/**
 * The tracks being browsed or found: configurable columns (right-click the header),
 * colour and rating set in place, rows dragged onto crates or within the prepare list.
 */
@Component({
  selector: 'app-track-table',
  templateUrl: './track-table.html',
  styleUrl: './track-table.scss',
  imports: [Cover],
})
export class TrackTable {
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  private readonly preferences = inject(PreferencesService);
  private readonly host = inject(ElementRef<HTMLElement>);

  protected readonly tracks = this.library.visibleTracks;
  protected readonly deckIndexes = Array.from({ length: DECK_COUNT }, (_, i) => i);
  protected readonly stars = STARS;
  protected readonly trackColors = TRACK_COLORS;
  protected readonly allColumns = COLUMNS;
  protected readonly deckColor = deckColor;
  protected readonly keyColor = camelotColor;
  protected readonly hexColor = hexColor;
  protected readonly title = trackTitle;

  private readonly hidden = computed(() => new Set(this.preferences.library().hiddenColumns));
  protected readonly columns = computed(() =>
    COLUMNS.filter((c) => c.fixed || !this.hidden().has(c.id)),
  );
  protected readonly template = computed(() =>
    gridTemplate(this.columns(), this.library.ordered()),
  );

  protected readonly columnMenu = signal<Popover | null>(null);
  protected readonly palette = signal<(Popover & { path: string }) | null>(null);
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

  protected readonly emptyMessage = computed(() => {
    if (this.library.scope() === 'list') return EMPTY_LIST[this.library.source().kind];
    return this.library.filter().trim()
      ? 'No analysed track matches'
      : 'Type to search every analysed track';
  });

  constructor() {
    effect(() => {
      const path = this.library.selected();
      const row =
        path && this.host.nativeElement.querySelector(`[data-path="${CSS.escape(path)}"]`);
      row?.scrollIntoView({ block: 'nearest' });
    });
  }

  sortMark(column: SortColumn | undefined): string {
    const s = this.library.sort();
    return column && s.column === column ? (s.direction === 1 ? '▲' : '▼') : '';
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

  isShown(column: Column): boolean {
    return column.fixed || !this.hidden().has(column.id);
  }

  toggleColumn(id: ColumnId) {
    const hidden = new Set(this.hidden());
    if (!hidden.delete(id)) hidden.add(id);
    this.preferences.updateLibrary({ hiddenColumns: [...hidden] });
  }

  openColumnMenu(event: MouseEvent) {
    event.preventDefault();
    this.columnMenu.set(popoverAt(event));
  }

  openPalette(event: MouseEvent, t: DirEntry) {
    event.stopPropagation();
    this.palette.set({ ...popoverAt(event), path: t.path });
  }

  setColor(color: number | null) {
    const palette = this.palette();
    this.palette.set(null);
    if (palette) this.library.setColor([palette.path], color);
  }

  /** Clicking the current rating again clears it. */
  rate(t: DirEntry, stars: number) {
    this.library.setRating([t.path], t.details?.rating === stars ? 0 : stars);
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

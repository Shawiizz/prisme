import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  afterRenderEffect,
  computed,
  effect,
  inject,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { DirEntry } from '../../../core/models/library';
import { EngineService } from '../../../core/services/engine.service';
import {
  LibraryService,
  SortColumn,
  TrackSource,
  trackTitle,
} from '../../../core/services/library.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { camelotCompatible, camelotHue, camelotMajor, keyName } from '../../../shared/camelot';
import { Cover } from '../../../shared/cover/cover';
import { TRACKS_DRAG_TYPE } from '../../../shared/drag';
import { decimal, deckLetter } from '../../../shared/time';
import { octaveRatio } from '../../../core/sync';
import { HistoryService } from '../../../core/services/history.service';
import { StemsService } from '../../../core/services/stems.service';
import { TRACK_COLORS, hexColor } from '../../../shared/track-colors';
import { COLUMNS, Column, ColumnId, gridTemplate } from './columns';
import { TrackMenu, TrackMenuPanel } from '../track-menu/track-menu';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';

const EMPTY_LIST: Record<TrackSource['kind'], string> = {
  folder: t('No audio file in this folder'),
  crate: t('This crate is empty: drag tracks onto it'),
  history: t('No track in this set'),
  prepare: t('Drag tracks here or press Ctrl + P to line them up for the set'),
  duplicates: t('No two tracks share an artist and a title'),
};

const STARS = [1, 2, 3, 4, 5];
/** Rows drawn beyond those in view, above and below: a quick scroll shows no gap. */
const OVERSCAN = 12;

/**
 * A popover opened at a point: below it, or above it in the lower half of the window; to
 * its right, or to its left in the right half.
 */
interface Popover {
  left: number | null;
  right: number | null;
  top: number | null;
  bottom: number | null;
}

function popoverAt(x: number, y: number): Popover {
  const below = y < window.innerHeight / 2;
  const rightwards = x < window.innerWidth / 2;
  return {
    left: rightwards ? x : null,
    right: rightwards ? null : window.innerWidth - x,
    top: below ? y : null,
    bottom: below ? null : window.innerHeight - y,
  };
}

function popoverAtPointer(event: MouseEvent): Popover {
  return popoverAt(event.clientX, event.clientY);
}

/** A popover above an element (the buttons of the selection bar). */
function popoverAbove(element: HTMLElement): Popover {
  const r = element.getBoundingClientRect();
  return { ...popoverAt(r.left, r.top), top: null, bottom: window.innerHeight - r.top + 4 };
}

/**
 * The tracks being browsed or found: configurable columns (right-click the header),
 * colour and rating set in place, rows dragged onto crates or within the prepare list.
 */
@Component({
  selector: 'app-track-table',
  templateUrl: './track-table.html',
  styleUrl: './track-table.scss',
  imports: [Cover, TrackMenu, TPipe],
})
export class TrackTable {
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  protected readonly stems = inject(StemsService);
  private readonly preferences = inject(PreferencesService);
  private readonly history = inject(HistoryService);
  private readonly table = viewChild.required<ElementRef<HTMLElement>>('table');

  protected readonly tracks = this.library.visibleTracks;
  /** Only the rows in view are drawn (a library of thousands would be hundreds of thousands
   * of elements): the others are spaces of their height. */
  private readonly rowHeight =
    parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--pr-row')) || 29;
  private readonly scrollTop = signal(0);
  private readonly viewHeight = signal(0);
  private readonly firstShown = computed(() =>
    Math.max(0, Math.floor(this.scrollTop() / this.rowHeight) - OVERSCAN),
  );
  private readonly shownCount = computed(
    () => Math.ceil(this.viewHeight() / this.rowHeight) + 2 * OVERSCAN,
  );
  protected readonly shown = computed(() => {
    const tracks = this.tracks();
    const first = Math.min(this.firstShown(), tracks.length);
    const last = Math.min(first + this.shownCount(), tracks.length);
    return {
      rows: tracks.slice(first, last),
      before: first * this.rowHeight,
      after: (tracks.length - last) * this.rowHeight,
    };
  });
  protected readonly deckIndexes = this.engine.deckIndexes;
  protected readonly stars = STARS;
  protected readonly trackColors = TRACK_COLORS;
  protected readonly allColumns = COLUMNS;
  protected readonly letter = deckLetter;
  protected readonly decimal = decimal;
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
  protected readonly keyName = keyName;
  protected readonly hexColor = hexColor;
  protected readonly title = trackTitle;

  private readonly hidden = computed(() => new Set(this.preferences.library().hiddenColumns));
  protected readonly columns = computed(() =>
    COLUMNS.filter((c) => c.fixed || !this.hidden().has(c.id)),
  );
  protected readonly template = computed(() =>
    gridTemplate(this.columns(), this.library.ordered(), this.deckIndexes().length),
  );

  protected readonly columnMenu = signal<Popover | null>(null);
  protected readonly palette = signal<(Popover & { path: string }) | null>(null);
  /** The actions on the selection, opened on one of their panels. */
  protected readonly trackMenu = signal<(Popover & { panel: TrackMenuPanel }) | null>(null);
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

  /** The reference deck's key as heard and its tempo now: what the rows are compared with. */
  private readonly referenceKey = computed(
    () => this.engine.heardKey(this.library.reference())?.camelot ?? null,
  );
  /** To a hundredth of a BPM: the rows follow a tempo change, not every status tick. */
  private readonly referenceBpm = computed(() => {
    const bpm = this.engine.status()?.decks[this.library.reference()]?.bpm;
    return bpm == null ? null : Math.round(bpm * 100) / 100;
  });
  /** Which decks play: the deck marks change with it, not with every status tick. */
  protected readonly playing = computed(
    () => (this.engine.status()?.decks ?? []).map((d) => d?.playing ?? false),
    { equal: (a, b) => a.length === b.length && a.every((v, i) => v === b[i]) },
  );
  protected readonly pitchHeader = computed(() =>
    t('Pitch → {deck}', { deck: deckLetter(this.library.reference()) }),
  );
  protected readonly mixTitle = computed(() =>
    t('Sort by how well the tracks follow deck {deck}: key as heard, then tempo', {
      deck: deckLetter(this.library.reference()),
    }),
  );
  /** Path of the track on each deck. */
  private readonly deckPaths = computed(() => this.engine.tracks().map((track) => track?.path));

  protected readonly emptyMessage = computed(() => {
    if (this.library.scope() === 'list') return EMPTY_LIST[this.library.source().kind];
    return this.library.filter().trim()
      ? t('No analysed track matches')
      : t('Type to search every analysed track');
  });

  constructor() {
    effect(() => {
      const path = this.library.selected();
      if (path) untracked(() => this.scrollTo(path));
    });
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      const table = this.table().nativeElement;
      this.viewHeight.set(table.clientHeight);
      const resized = new ResizeObserver(() => this.viewHeight.set(table.clientHeight));
      resized.observe(table);
      destroyRef.onDestroy(() => resized.disconnect());
    });
    // another list may leave the table scrolled elsewhere (back to its end if shorter)
    afterRenderEffect(() => {
      this.tracks();
      untracked(() => this.scrolled());
    });
  }

  scrolled() {
    this.scrollTop.set(this.table().nativeElement.scrollTop);
  }

  /** Scrolls the least that shows the row of `path` whole, under the column headers. */
  private scrollTo(path: string) {
    const index = this.tracks().findIndex((t) => t.path === path);
    if (index < 0) return;
    const table = this.table().nativeElement;
    const head = table.querySelector<HTMLElement>('.head')?.offsetHeight ?? 0;
    const top = index * this.rowHeight;
    if (top < table.scrollTop) {
      table.scrollTop = top;
    } else if (head + top + this.rowHeight > table.scrollTop + table.clientHeight) {
      table.scrollTop = head + top + this.rowHeight - table.clientHeight;
    }
  }

  sortMark(column: SortColumn | undefined): string {
    const s = this.library.sort();
    return column && s.column === column ? (s.direction === 1 ? '▲' : '▼') : '';
  }

  /** Whether the key mixes with the reference deck's. */
  keyMatches(camelot: string): boolean {
    const key = this.referenceKey();
    return key !== null && camelotCompatible(key, camelot);
  }

  /** The pitch that brings a track to the reference deck's tempo, and whether a turntable reaches it. */
  pitchToMatch(bpm: number | null | undefined): { text: string; reachable: boolean } | null {
    const reference = this.referenceBpm();
    if (!bpm || !reference) return null;
    const change = (octaveRatio(bpm, reference) * reference) / bpm - 1;
    const percent = Math.round(change * 1000) / 10 || 0;
    return {
      text: `${percent >= 0 ? '+' : '−'}${decimal(Math.abs(percent), 1)} %`,
      reachable: Math.abs(change) <= this.preferences.dj().pitchRange,
    };
  }

  /** The decks a track is loaded on. */
  decksOf(path: string): number[] {
    return this.deckPaths().flatMap((p, deck) => (p === path ? [deck] : []));
  }

  played(path: string): boolean {
    return this.history.playedTonight().has(path);
  }

  /** A set can hold the same track twice: its rows are told apart by play time. */
  /** `0.42` → `42 %` */
  percent(fraction: number): string {
    return `${Math.round(fraction * 100)} %`;
  }

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
    this.columnMenu.set(popoverAtPointer(event));
  }

  openPalette(event: MouseEvent, t: DirEntry) {
    event.stopPropagation();
    this.palette.set({ ...popoverAtPointer(event), path: t.path });
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

  /** Click: this track; Ctrl click: add or remove it; Shift click: up to it. */
  click(event: MouseEvent, path: string) {
    const mode = event.shiftKey ? 'range' : event.ctrlKey || event.metaKey ? 'toggle' : 'only';
    this.library.select(path, mode);
  }

  /** Right click: the actions on the selection, or on this track alone if it is not in it. */
  openTrackMenu(event: MouseEvent, path: string) {
    event.preventDefault();
    if (!this.library.selection().has(path)) this.library.select(path, 'only');
    this.trackMenu.set({ ...popoverAtPointer(event), panel: 'main' });
  }

  /** A button of the selection bar: its panel of the actions, above it. */
  openPanel(event: MouseEvent, panel: TrackMenuPanel) {
    this.trackMenu.set({ ...popoverAbove(event.currentTarget as HTMLElement), panel });
  }

  separateSelection() {
    this.stems.separate(this.library.selectedPaths());
  }

  /** Dragging a selected track drags the whole selection. */
  dragStart(event: DragEvent, path: string) {
    if (!this.library.selection().has(path)) this.library.select(path, 'only');
    event.dataTransfer?.setData(TRACKS_DRAG_TYPE, JSON.stringify(this.library.selectedPaths()));
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
    const paths = JSON.parse(data) as string[];
    this.library.placeInPrepare(paths, before?.path ?? null);
  }
}

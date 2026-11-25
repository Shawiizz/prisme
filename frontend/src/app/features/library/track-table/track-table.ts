import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  effect,
  inject,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { LibraryService, cursorMoved } from '../../../core/services/library.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { musicalKeyName } from '../../../shared/musical-key';
import { KeyChip } from '../../../shared/key-chip/key-chip';
import { Cover } from '../../../shared/cover/cover';
import { TRACKS_DRAG_TYPE } from '../../../shared/drag';
import { decimal, deckLetter, formatPitch } from '../../../shared/time';
import { percent } from '../../../shared/readout';
import { StemsService } from '../../../core/services/stems.service';
import { PreviewService } from '../../../core/services/preview.service';
import { TRACK_COLORS, hexColor } from '../../../shared/track-colors';
import { COLUMNS, Column, ColumnId, gridTemplate } from './columns';
import { TrackMenu, TrackMenuPanel } from '../track-menu/track-menu';
import { t } from '../../../core/i18n/i18n';
import { emptyText } from '../list-labels';
import { TPipe } from '../../../core/i18n/t.pipe';
import { KeyboardService } from '../../../core/keyboard/keyboard.service';
import { Dismiss, overlay } from '../../../shared/overlay';
import { lastName, parentOf, shortPath } from '../../../core/folders';
import { Popover, popoverAbove, popoverAtPointer } from '../../../shared/popover';
import type { LibraryRow, LibraryView, SortColumn } from '../../../core/models/backend';

const STARS = [1, 2, 3, 4, 5];
/** Rows asked for beyond those in view, above and below: a quick scroll shows no gap... */
const OVERSCAN = 30;
/** ...asked again only once the view comes this near the edge of those held: the rows are
 * not drawn again at each row scrolled. */
const REFILL = 10;

/**
 * The tracks being browsed or found, as the backend lists them: only the rows in view are
 * drawn, asked for as the table scrolls. Configurable columns (right-click the header), colour
 * and rating set in place, rows dragged onto crates, decks or within the prepare list.
 */
@Component({
  selector: 'app-track-table',
  templateUrl: './track-table.html',
  styleUrl: './track-table.scss',
  imports: [Cover, KeyChip, TrackMenu, TPipe, Dismiss],
})
export class TrackTable {
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  protected readonly stems = inject(StemsService);
  protected readonly preview = inject(PreviewService);
  private readonly preferences = inject(PreferencesService);
  private readonly keyboard = inject(KeyboardService);
  private readonly table = viewChild.required<ElementRef<HTMLElement>>('table');

  private readonly rowHeight =
    parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--pr-row')) || 29;
  private readonly scrollTop = signal(0);
  /** The frame that reads the scroll, asked for. */
  private scrollRead = 0;
  private readonly viewHeight = signal(0);
  /** The rows in view. */
  private readonly inView = computed(() => ({
    first: Math.floor(this.scrollTop() / this.rowHeight),
    count: Math.ceil(this.viewHeight() / this.rowHeight) + 1,
  }));
  protected readonly shown = computed(() => {
    const { first, rows } = this.library.window();
    const count = this.library.count();
    return {
      rows,
      before: first * this.rowHeight,
      after: Math.max(0, count - first - rows.length) * this.rowHeight,
    };
  });
  protected readonly deckIndexes = this.engine.deckIndexes;
  protected readonly stars = STARS;
  protected readonly trackColors = TRACK_COLORS;
  protected readonly allColumns = COLUMNS;
  protected readonly letter = deckLetter;
  protected readonly decimal = decimal;
  protected readonly keyName = musicalKeyName;
  protected readonly hexColor = hexColor;
  protected readonly parentOf = parentOf;
  protected readonly shortPath = shortPath;
  protected readonly lastName = lastName;

  private readonly hidden = computed(() => new Set(this.preferences.library().hiddenColumns));
  /** The tracks from elsewhere are listed: where each one is says it, not a mark. */
  protected readonly elsewhere = computed(
    () => this.library.source()?.kind === 'elsewhere' && this.library.scope() === 'list',
  );
  protected readonly columns = computed(() =>
    COLUMNS.filter((c) => (c.elsewhere ? this.elsewhere() : c.fixed || !this.hidden().has(c.id))),
  );
  protected readonly template = computed(() =>
    gridTemplate(this.columns(), this.library.ordered(), this.deckIndexes().length),
  );
  /** The tracks of a narrow table, without the columns shown on a wide one only. */
  protected readonly narrowTemplate = computed(() =>
    gridTemplate(
      this.columns().filter((c) => !c.wide),
      this.library.ordered(),
      this.deckIndexes().length,
    ),
  );

  protected readonly columnMenu = signal<Popover | null>(null);
  protected readonly palette = signal<(Popover & { path: string }) | null>(null);
  /** The actions on the selection, opened on one of their panels. */
  protected readonly trackMenu = signal<(Popover & { panel: TrackMenuPanel }) | null>(null);
  /** Row a dragged track would be put before (`null`: the end of the list). */
  protected readonly insertBefore = signal<string | null>(null);

  protected readonly pitchHeader = computed(() =>
    t('Pitch → {deck}', { deck: deckLetter(this.library.reference()) }),
  );
  protected readonly mixTitle = computed(() =>
    t('Sort by how well the tracks follow deck {deck}: key as heard, then tempo', {
      deck: deckLetter(this.library.reference()),
    }),
  );

  protected readonly emptyMessage = computed(() =>
    emptyText(
      this.library.view(),
      this.library.filter(),
      this.keyboard.firstKey('library.prepare'),
    ),
  );

  constructor() {
    overlay(
      () => this.columnMenu() !== null,
      () => this.columnMenu.set(null),
    );
    overlay(
      () => this.palette() !== null,
      () => this.palette.set(null),
    );
    overlay(
      () => this.trackMenu() !== null,
      () => this.trackMenu.set(null),
    );
    // the rows in view and some more asked for as the table scrolls or grows near the edge of
    // those held, once a frame at most; the backend sends them again as they change
    let frame = 0;
    effect(() => {
      const { first, count } = this.inView();
      const held = untracked(this.library.window);
      const end = held.first + held.rows.length;
      const above = held.first === 0 || first - held.first >= REFILL;
      const below = end >= untracked(this.library.count) || end - (first + count) >= REFILL;
      if (held.seq >= 0 && above && below) return;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(
        () => void this.library.viewport(Math.max(0, first - OVERSCAN), count + 2 * OVERSCAN),
      );
    });
    // the cursor moved in the list (a key, a controller): its row brought into view
    let before: LibraryView | null = null;
    effect(() => {
      const view = this.library.view();
      const moved = cursorMoved(before, view);
      before = view;
      if (moved && view?.cursor != null) untracked(() => this.scrollTo(view.cursor!));
    });
    const destroyRef = inject(DestroyRef);
    destroyRef.onDestroy(() => {
      cancelAnimationFrame(frame);
      cancelAnimationFrame(this.scrollRead);
    });
    afterNextRender(() => {
      const table = this.table().nativeElement;
      this.viewHeight.set(table.clientHeight);
      const resized = new ResizeObserver(() => this.viewHeight.set(table.clientHeight));
      resized.observe(table);
      destroyRef.onDestroy(() => resized.disconnect());
    });
  }

  /** The scroll read once a frame, with the frame's own layout: read at each scroll event, it
   * made the page lay itself out again for it, and the list moved at half the screen's rate. */
  scrolled() {
    if (this.scrollRead) return;
    this.scrollRead = requestAnimationFrame(() => {
      this.scrollRead = 0;
      this.scrollTop.set(this.table().nativeElement.scrollTop);
    });
  }

  /** Scrolls the least that shows the row at `index` whole, under the column headers. */
  private scrollTo(index: number) {
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
    return column && s.column === column ? (s.descending ? '▼' : '▲') : '';
  }

  /** `+2.4 %`, `−0.5 %` */
  pitchText(percent: number): string {
    return formatPitch(1 + percent / 100, 1);
  }

  /** What the preview key of a row does, or why it cannot. */
  previewTitle(previewed: boolean): string {
    if (previewed) return t('Stop the preview');
    return this.preview.heard()
      ? t('Preview in the headphones')
      : t('No headphones of their own to preview in: choose their outputs in the settings');
  }

  protected readonly percent = percent;

  /** A set can hold the same track twice: its rows are told apart by play time. */
  rowKey(row: LibraryRow): string {
    return row.playedAtMs === null ? row.path : `${row.path}@${row.playedAtMs}`;
  }

  isShown(column: Column): boolean {
    return column.fixed || !this.hidden().has(column.id);
  }

  toggleColumn(id: ColumnId) {
    this.preferences.set('column', { column: id, shown: this.hidden().has(id) });
  }

  openColumnMenu(event: MouseEvent) {
    event.preventDefault();
    this.columnMenu.set(popoverAtPointer(event));
  }

  openPalette(event: MouseEvent, row: LibraryRow) {
    event.stopPropagation();
    this.palette.set({ ...popoverAtPointer(event), path: row.path });
  }

  setColor(color: number | null) {
    const palette = this.palette();
    this.palette.set(null);
    if (palette) this.library.setColor([palette.path], color);
  }

  /** Click: this track; Ctrl click: add or remove it; Shift click: up to it. */
  click(event: MouseEvent, row: LibraryRow) {
    const pick = event.shiftKey ? 'range' : event.ctrlKey || event.metaKey ? 'toggle' : 'only';
    this.library.select(row, pick);
  }

  /** Right click: the actions on the selection, or on this track alone if it is not in it. */
  openTrackMenu(event: MouseEvent, row: LibraryRow) {
    event.preventDefault();
    this.library.select(row, 'context');
    this.trackMenu.set({ ...popoverAtPointer(event), panel: 'main' });
  }

  /** A button of the selection bar: its panel of the actions, above it. */
  openPanel(event: MouseEvent, panel: TrackMenuPanel) {
    this.trackMenu.set({ ...popoverAbove(event.currentTarget as HTMLElement), panel });
  }

  separateSelection() {
    return this.stems.separate(null);
  }

  /** Dragging a track drags the selection, the track in it: the one dragged onto a deck. */
  dragStart(event: DragEvent, row: LibraryRow) {
    this.library.select(row, 'context');
    event.dataTransfer?.setData(TRACKS_DRAG_TYPE, row.path);
  }

  listDragOver(event: DragEvent, before: LibraryRow | null) {
    if (!this.library.reorderable() || !event.dataTransfer?.types.includes(TRACKS_DRAG_TYPE))
      return;
    event.preventDefault();
    event.stopPropagation();
    this.insertBefore.set(before ? this.rowKey(before) : null);
  }

  listDrop(event: DragEvent, before: LibraryRow | null) {
    this.insertBefore.set(null);
    if (!this.library.reorderable() || !event.dataTransfer?.getData(TRACKS_DRAG_TYPE)) return;
    event.preventDefault();
    event.stopPropagation();
    this.library.placeInPrepare(null, before?.path ?? null);
  }
}

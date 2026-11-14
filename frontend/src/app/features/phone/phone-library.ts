import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  booleanAttribute,
  computed,
  effect,
  inject,
  input,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { DirEntry } from '../../core/models/library';
import { CratesService } from '../../core/services/crates.service';
import { EngineService } from '../../core/services/engine.service';
import { LibraryService, SortColumn, trackTitle } from '../../core/services/library.service';
import { PrepareService } from '../../core/services/prepare.service';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { camelotCompatible } from '../../shared/camelot';
import { Cover } from '../../shared/cover/cover';
import { decimal, deckLetter } from '../../shared/time';
import { ImportDialog } from '../library/import-dialog/import-dialog';
import { LibrarySidebar } from '../library/library-sidebar/library-sidebar';
import { LIST_LABEL, SEARCH_PLACEHOLDER } from '../library/list-labels';
import { TrackMenu } from '../library/track-menu/track-menu';
import { COLUMNS } from '../library/track-table/columns';

/** Height of a row: a finger's, and room for a cover. */
const ROW_PX = 60;
/** Rows drawn beyond the view, each way. */
const OVERSCAN = 6;
/** A finger has left where it landed, then gone far enough to load the track (pixels). */
const MOVED_PX = 10;
const LOAD_PX = 84;

/** What the tracks can be ordered by, as the columns of the table name it. */
const SORTS: readonly { sort: SortColumn; name: string }[] = COLUMNS.flatMap((c) =>
  c.sort ? [{ sort: c.sort, name: c.name }] : [],
);
const LIST_ORDER = { sort: 'order', name: t('Order of the list') } as const;

type Row =
  { kind: 'folder'; name: string; path: string; up: boolean } | { kind: 'track'; track: DirEntry };

/** A finger on a track: where it landed, how far it has slid it, and whether it still may tap. */
interface Slide {
  path: string;
  pointer: number;
  x: number;
  y: number;
  dx: number;
  sliding: boolean;
}

/**
 * The library on a phone: a search and the order, the lists to go to, then large rows, a
 * cover, the tempo and the key (lit when it fits the deck). A row slid to the right loads
 * on the first deck, to the left on the second; touched, it opens what can be done with the
 * track. `peek` leaves the search and the rows, under the decks. Only the rows in view are
 * drawn.
 */
@Component({
  selector: 'app-phone-library',
  templateUrl: './phone-library.html',
  styleUrl: './phone-library.scss',
  imports: [Cover, ImportDialog, LibrarySidebar, TPipe, TrackMenu],
})
export class PhoneLibrary {
  readonly peek = input(false, { transform: booleanAttribute });

  protected readonly library = inject(LibraryService);
  private readonly engine = inject(EngineService);
  private readonly crates = inject(CratesService);
  protected readonly prepared = inject(PrepareService).list;
  private readonly list = viewChild.required<ElementRef<HTMLElement>>('list');
  private readonly scrollTop = signal(0);
  private readonly viewHeight = signal(0);

  /** The decks a slide loads on: to the right the first, to the left the second. */
  protected readonly right = computed(() => this.engine.deckIndexes()[0]);
  protected readonly left = computed(() => this.engine.deckIndexes()[1] ?? null);

  /** What the list being browsed is called: its folder, its crate, else its kind. */
  protected readonly listName = computed(() => {
    const source = this.library.source();
    if (source.kind === 'crate') return this.crates.name(source.id) ?? LIST_LABEL.crate;
    if (source.kind !== 'folder') return LIST_LABEL[source.kind];
    const path = this.library.listing()?.path;
    return path?.split(/[\\/]/).filter(Boolean).pop() ?? LIST_LABEL.folder;
  });
  protected readonly searchPlaceholder = computed(
    () => SEARCH_PLACEHOLDER[this.library.source().kind],
  );
  protected readonly sorts = computed(() =>
    this.library.ordered() ? [LIST_ORDER, ...SORTS] : SORTS,
  );

  /** The folders come first, while a folder is browsed and nothing is searched. */
  private readonly folders = computed<Row[]>(() => {
    const listing = this.library.listing();
    const browsing = this.library.source().kind === 'folder' && this.library.scope() === 'list';
    if (!browsing || !listing || this.library.filter().trim()) return [];
    const up: Row[] =
      listing.parent && listing.path !== this.library.root()
        ? [{ kind: 'folder', name: '..', path: listing.parent, up: true }]
        : [];
    const folders = listing.folders.map((f): Row => ({
      kind: 'folder',
      name: f.name,
      path: f.path,
      up: false,
    }));
    return [...up, ...folders];
  });
  private readonly rows = computed<Row[]>(() => [
    ...this.folders(),
    ...this.library.visibleTracks().map((track): Row => ({ kind: 'track', track })),
  ]);
  protected readonly empty = computed(() => this.rows().length === 0);
  protected readonly window = computed(() => {
    const rows = this.rows();
    const first = Math.max(0, Math.floor(this.scrollTop() / ROW_PX) - OVERSCAN);
    const last = Math.min(
      rows.length,
      first + Math.ceil(this.viewHeight() / ROW_PX) + 2 * OVERSCAN,
    );
    return {
      rows: rows.slice(first, last),
      before: first * ROW_PX,
      after: (rows.length - last) * ROW_PX,
    };
  });

  /** The key the reference deck is heard in: the keys that fit it are lit. */
  private readonly deckKey = computed(
    () => this.engine.heardKey(this.library.reference())?.camelot ?? null,
  );

  protected readonly slide = signal<Slide | null>(null);
  /** The track whose menu is open. */
  protected readonly menu = signal<string | null>(null);
  protected readonly menuPaths = computed(() => {
    const path = this.menu();
    return path ? [path] : [];
  });
  /** The lists of the library are open, to go to another. */
  protected readonly browsing = signal(false);
  protected readonly importing = signal(false);

  protected readonly ROW_PX = ROW_PX;
  protected readonly title = trackTitle;
  protected readonly letter = deckLetter;

  constructor() {
    const destroyed = inject(DestroyRef);
    afterNextRender(() => {
      const list = this.list().nativeElement;
      const resized = new ResizeObserver(() => this.viewHeight.set(list.clientHeight));
      resized.observe(list);
      destroyed.onDestroy(() => resized.disconnect());
    });
    // a list chosen among those of the library is the one shown: its panel closes
    effect(() => {
      this.library.source();
      this.library.listing();
      untracked(() => this.browsing.set(false));
    });
  }

  protected scrolled() {
    this.scrollTop.set(this.list().nativeElement.scrollTop);
  }

  protected bpm(track: DirEntry): string {
    const bpm = track.details?.bpm;
    return bpm == null ? '' : decimal(bpm, 0);
  }

  protected fits(track: DirEntry): boolean {
    const deck = this.deckKey();
    const key = track.details?.key?.camelot;
    return !!deck && !!key && camelotCompatible(deck, key);
  }

  /** How far the row of `track` is slid, and the deck it would load on if let go there. */
  protected slid(track: DirEntry): { dx: number; deck: number | null } | null {
    const slide = this.slide();
    if (!slide?.sliding || slide.path !== track.path) return null;
    return { dx: slide.dx, deck: this.deckAt(slide.dx) };
  }

  /** The deck a row slid by `dx` loads on: none until it has gone far enough. */
  private deckAt(dx: number): number | null {
    if (Math.abs(dx) < LOAD_PX) return null;
    return dx > 0 ? this.right() : this.left();
  }

  protected press(track: DirEntry, event: PointerEvent) {
    if (event.button !== 0) return;
    const { clientX: x, clientY: y, pointerId: pointer } = event;
    this.slide.set({ path: track.path, pointer, x, y, dx: 0, sliding: false });
  }

  protected move(event: PointerEvent) {
    const slide = this.slide();
    if (!slide || slide.pointer !== event.pointerId) return;
    const dx = event.clientX - slide.x;
    if (!slide.sliding) {
      // sideways first: the row follows the finger; up or down, the list scrolls
      if (Math.abs(dx) < MOVED_PX) return;
      (event.currentTarget as Element).setPointerCapture(event.pointerId);
    }
    // no second deck: nothing to slide to the left for
    this.slide.set({ ...slide, sliding: true, dx: this.left() === null ? Math.max(dx, 0) : dx });
  }

  protected release(track: DirEntry) {
    const slide = this.slide();
    this.slide.set(null);
    if (!slide) return;
    if (!slide.sliding) {
      this.menu.set(track.path);
      return;
    }
    const deck = this.deckAt(slide.dx);
    if (deck !== null) this.library.load(track.path, deck);
  }

  /** The finger went on to scroll the list: nothing was touched. */
  protected cancel() {
    this.slide.set(null);
  }

  /** The folder read again: the phone may only now let its music be read. */
  protected async lookAgain() {
    const folder = this.library.listing()?.path ?? this.library.root();
    if (folder && (await this.library.open(folder))) await this.library.rescan();
  }
}

import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { DirEntry } from '../../core/models/library';
import { EngineService } from '../../core/services/engine.service';
import { LibraryService, SortColumn, trackTitle } from '../../core/services/library.service';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { camelotHue, camelotMajor } from '../../shared/camelot';
import { decimal, deckLetter } from '../../shared/time';
import { LIST_LABEL } from '../library/list-labels';
import { TrackMenu } from '../library/track-menu/track-menu';
import { COLUMNS } from '../library/track-table/columns';

/** Height of a row: a finger's. */
const ROW_PX = 52;
/** Rows drawn beyond the view, each way. */
const OVERSCAN = 6;

/** What the tracks can be ordered by, as the columns of the table name it. */
const SORTS: readonly { sort: SortColumn; name: string }[] = COLUMNS.flatMap((c) =>
  c.sort ? [{ sort: c.sort, name: c.name }] : [],
);
const LIST_ORDER = { sort: 'order', name: t('Order of the list') } as const;

type Row =
  { kind: 'folder'; name: string; path: string; up: boolean } | { kind: 'track'; track: DirEntry };

/**
 * The tracks on a phone: a search, where it looks and the order, then the folders of the
 * folder being browsed and its tracks, each with a button per deck to load it; touched, a
 * track opens what can be done with it. Only the rows in view are drawn.
 */
@Component({
  selector: 'app-phone-tracks',
  templateUrl: './phone-tracks.html',
  styleUrl: './phone-tracks.scss',
  imports: [TPipe, TrackMenu],
})
export class PhoneTracks {
  protected readonly library = inject(LibraryService);
  protected readonly decks = inject(EngineService).deckIndexes;
  private readonly list = viewChild.required<ElementRef<HTMLElement>>('list');
  private readonly scrollTop = signal(0);
  private readonly viewHeight = signal(0);

  /** The folders come first, while a folder is browsed and nothing is searched. */
  private readonly folders = computed<Row[]>(() => {
    const listing = this.library.listing();
    if (this.library.source().kind !== 'folder' || !listing || this.library.filter().trim()) {
      return [];
    }
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
  protected readonly listLabel = computed(() => LIST_LABEL[this.library.source().kind]);
  protected readonly sorts = computed(() =>
    this.library.ordered() ? [LIST_ORDER, ...SORTS] : SORTS,
  );
  /** The track whose menu is open. */
  protected readonly menu = signal<string | null>(null);
  protected readonly menuPaths = computed(() => {
    const path = this.menu();
    return path ? [path] : [];
  });
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

  protected readonly ROW_PX = ROW_PX;
  protected readonly title = trackTitle;
  protected readonly letter = deckLetter;
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;

  constructor() {
    const destroyed = inject(DestroyRef);
    afterNextRender(() => {
      const list = this.list().nativeElement;
      const resized = new ResizeObserver(() => this.viewHeight.set(list.clientHeight));
      resized.observe(list);
      destroyed.onDestroy(() => resized.disconnect());
    });
  }

  protected scrolled() {
    this.scrollTop.set(this.list().nativeElement.scrollTop);
  }

  /** The folder read again: the phone may only now let its music be read. */
  protected async lookAgain() {
    const folder = this.library.listing()?.path ?? this.library.root();
    if (folder && (await this.library.open(folder))) await this.library.rescan();
  }

  protected bpm(track: DirEntry): string {
    const bpm = track.details?.bpm;
    return bpm == null ? '' : decimal(bpm, 0);
  }
}

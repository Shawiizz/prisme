import {
  Component,
  DestroyRef,
  ElementRef,
  computed,
  effect,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { LibraryService } from '../../core/services/library.service';
import { StemsService } from '../../core/services/stems.service';
import { PreviewService } from '../../core/services/preview.service';
import { AutoDjBar } from './auto-dj-bar/auto-dj-bar';
import { ExportDialog } from './export-dialog/export-dialog';
import { ImportDialog } from './import-dialog/import-dialog';
import { LibrarySidebar } from './library-sidebar/library-sidebar';
import { PreviewStrip } from './preview-strip/preview-strip';
import { dragSize, saveSize, savedSize } from '../../shared/resize';
import { TrackTable } from './track-table/track-table';
import { TPipe } from '../../core/i18n/t.pipe';
import { deckLetter } from '../../shared/time';
import { KeyboardService } from '../../core/keyboard/keyboard.service';
import { LIST_LABEL, SEARCH_PLACEHOLDER } from './list-labels';
import { PathStep, folderName, lastName, stepsFrom } from '../../core/folders';

/** How long "up to date" stays once a rescan asked for has ended (ms). */
const RESCANNED_MS = 3000;

/** The width of the side panel, as it is at first and as the DJ may set it (px). */
const SIDE_WIDTH = 230;
const SIDE_WIDTHS = { min: 180, max: 520 };
const SIDE_WIDTH_KEY = 'prisme.library.sideWidth';

/** Crates, history and folders (left); search, analysis progress, the way to the folder
 * browsed and tracks (right). */
@Component({
  selector: 'app-library',
  templateUrl: './library.html',
  styleUrl: './library.scss',
  imports: [AutoDjBar, ExportDialog, ImportDialog, LibrarySidebar, PreviewStrip, TrackTable, TPipe],
})
export class Library {
  protected readonly SIDE_WIDTH = SIDE_WIDTH;
  /** The width of the side panel, kept from one start to the next. */
  protected readonly sideWidth = signal(savedSize<number>(SIDE_WIDTH_KEY) ?? SIDE_WIDTH);
  protected readonly keyboard = inject(KeyboardService);
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  protected readonly stems = inject(StemsService);
  protected readonly preview = inject(PreviewService);
  protected readonly deckIndexes = this.engine.deckIndexes;
  protected readonly letter = deckLetter;
  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('search');

  protected readonly importOpen = signal(false);
  protected readonly exportOpen = signal(false);
  protected readonly progress = this.library.progress;
  /** The run of the analysis a rescan started, while it runs. */
  private readonly rescan = signal<number | null>(null);
  /** The rescan asked for runs, or is not shown yet. */
  protected readonly rescanning = computed(() => {
    const asked = this.rescan();
    const scan = this.library.scan();
    return asked !== null && (scan < asked || (scan === asked && this.progress().running));
  });
  protected readonly rescanned = signal(false);
  private rescannedTimer: ReturnType<typeof setTimeout> | undefined;
  protected readonly progressPercent = computed(() => {
    const p = this.progress();
    return p.total > 0 ? (p.done / p.total) * 100 : 0;
  });

  private readonly kind = computed(() => this.library.source()?.kind ?? 'folder');
  protected readonly listLabel = computed(() => LIST_LABEL[this.kind()]());
  protected readonly searchPlaceholder = computed(() => SEARCH_PLACEHOLDER[this.kind()]());

  /** The folders from the music folder down to the one browsed, as a file manager's address
   * bar shows them. */
  protected readonly steps = computed((): PathStep[] => {
    const folder = this.library.currentFolder();
    if (!folder) return [];
    const root = this.library.folders().find((f) => f.path === folder.root);
    if (!root) return [{ name: lastName(folder.path), path: folder.path }];
    const [top, ...below] = stepsFrom(root.path, folder.path);
    return [{ ...top, name: folderName(root) }, ...below];
  });
  /** There is a folder above the one browsed, inside its music folder. */
  protected readonly canGoUp = computed(() => !!this.library.currentFolder()?.up);

  protected resizeSide(event: PointerEvent) {
    dragSize(event, this.sideWidth(), 'x', (width) =>
      this.sideWidth.set(Math.min(Math.max(width, SIDE_WIDTHS.min), SIDE_WIDTHS.max)),
    );
  }

  constructor() {
    effect(() => saveSize(SIDE_WIDTH_KEY, this.sideWidth()));
    effect(() => {
      if (this.library.searchRequests() > 0) this.search().nativeElement.focus();
    });
    effect(() => {
      if (this.rescan() === null || this.rescanning()) return;
      this.rescan.set(null);
      this.rescanned.set(true);
      clearTimeout(this.rescannedTimer);
      this.rescannedTimer = setTimeout(() => this.rescanned.set(false), RESCANNED_MS);
    });
    inject(DestroyRef).onDestroy(() => clearTimeout(this.rescannedTimer));
  }

  protected startRescan() {
    this.rescanned.set(false);
    this.library.rescan().then(
      (scan) => this.rescan.set(scan),
      () => this.rescan.set(null),
    );
  }
}

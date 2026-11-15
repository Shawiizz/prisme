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
import { ImportDialog } from './import-dialog/import-dialog';
import { LibrarySidebar } from './library-sidebar/library-sidebar';
import { PreviewStrip } from './preview-strip/preview-strip';
import { TrackTable } from './track-table/track-table';
import { TPipe } from '../../core/i18n/t.pipe';
import { deckLetter } from '../../shared/time';
import { CTRL } from '../../core/keyboard/shortcuts';
import { LIST_LABEL, SEARCH_PLACEHOLDER } from './list-labels';

/** How long "up to date" stays once a rescan asked for has ended (ms). */
const RESCANNED_MS = 3000;

/** Crates, history and folders (left); search, analysis progress and tracks (right). */
@Component({
  selector: 'app-library',
  templateUrl: './library.html',
  styleUrl: './library.scss',
  imports: [ImportDialog, LibrarySidebar, PreviewStrip, TrackTable, TPipe],
})
export class Library {
  protected readonly CTRL = CTRL;
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  protected readonly stems = inject(StemsService);
  protected readonly preview = inject(PreviewService);
  protected readonly separationTotal = computed(() => {
    const p = this.stems.progress();
    return p.done + p.failed + p.queued;
  });
  protected readonly tracks = this.library.visibleTracks;
  protected readonly deckIndexes = this.engine.deckIndexes;
  protected readonly letter = deckLetter;
  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('search');

  protected readonly importOpen = signal(false);
  protected readonly progress = this.library.progress;
  /** The scans ended when a rescan was asked for, while it runs. */
  private readonly rescanFrom = signal<number | null>(null);
  protected readonly rescanning = computed(() => {
    const from = this.rescanFrom();
    return from !== null && this.library.scansEnded() === from;
  });
  protected readonly rescanned = signal(false);
  private rescannedTimer: ReturnType<typeof setTimeout> | undefined;
  protected readonly progressPercent = computed(() => {
    const p = this.progress();
    return p.total > 0 ? (p.done / p.total) * 100 : 0;
  });

  protected readonly listLabel = computed(() => LIST_LABEL[this.library.source().kind]());
  protected readonly searchPlaceholder = computed(() =>
    SEARCH_PLACEHOLDER[this.library.source().kind](),
  );

  constructor() {
    effect(() => {
      if (this.library.searchRequests() > 0) this.search().nativeElement.focus();
    });
    effect(() => {
      const from = this.rescanFrom();
      if (from === null || this.library.scansEnded() === from) return;
      this.rescanFrom.set(null);
      this.rescanned.set(true);
      clearTimeout(this.rescannedTimer);
      this.rescannedTimer = setTimeout(() => this.rescanned.set(false), RESCANNED_MS);
    });
    inject(DestroyRef).onDestroy(() => clearTimeout(this.rescannedTimer));
  }

  protected rescan() {
    this.rescanned.set(false);
    this.rescanFrom.set(this.library.scansEnded());
    this.library.rescan().catch(() => this.rescanFrom.set(null));
  }
}

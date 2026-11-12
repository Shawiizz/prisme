import {
  Component,
  ElementRef,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import { CratesService } from '../../../core/services/crates.service';
import { EngineService } from '../../../core/services/engine.service';
import { LibraryService } from '../../../core/services/library.service';
import { NavigationService } from '../../../core/services/navigation.service';
import { StemsService } from '../../../core/services/stems.service';
import { TRACK_COLORS, hexColor } from '../../../shared/track-colors';
import { deckLetter } from '../../../shared/time';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';

export type TrackMenuPanel = 'main' | 'crates' | 'color' | 'genre';

/** The folder a file is in: "C:/Music/House/a.mp3" → "House". */
function folderName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts.length > 1 ? parts[parts.length - 2] : '';
}

/**
 * What can be done with the selected tracks, in panels: the actions, then the crates,
 * the colours or the genre to give them. Opened by a right click on a row, or from the
 * selection bar straight on a panel.
 */
@Component({
  selector: 'app-track-menu',
  templateUrl: './track-menu.html',
  styleUrl: './track-menu.scss',
  imports: [TPipe],
})
export class TrackMenu {
  protected readonly library = inject(LibraryService);
  private readonly engine = inject(EngineService);
  protected readonly stems = inject(StemsService);
  private readonly crates = inject(CratesService);
  private readonly navigation = inject(NavigationService);

  readonly paths = input.required<string[]>();
  readonly start = input<TrackMenuPanel>('main');
  readonly closed = output<void>();

  protected readonly panel = signal<TrackMenuPanel>('main');
  protected readonly decks = this.engine.deckIndexes;
  protected readonly letter = deckLetter;
  protected readonly trackColors = TRACK_COLORS;
  protected readonly hexColor = hexColor;
  protected readonly genre = signal('');
  private readonly genreInput = viewChild<ElementRef<HTMLInputElement>>('genreInput');

  /** Crates tracks can be put in: not the smart ones, which pick their own. */
  protected readonly targetCrates = computed(() => this.crates.crates().filter((c) => !c.rules));
  private readonly tracks = computed(() => {
    const paths = new Set(this.paths());
    return this.library.visibleTracks().filter((t) => paths.has(t.path));
  });
  /** Tracks of the selection the installed models have stems to add to. */
  protected readonly toSeparate = computed(() =>
    this.tracks()
      .filter((t) => t.details && !t.details.failed && this.stems.canAdd(t.details.separation))
      .map((t) => t.path)
      .filter((p) => !this.stems.pending().has(p)),
  );
  private readonly removeFromPrepare = t('Remove from the prepare list');
  private readonly removeFromCrate = t('Remove from the crate');
  protected readonly removeLabel = computed(() =>
    this.library.source().kind === 'prepare' ? this.removeFromPrepare : this.removeFromCrate,
  );

  constructor() {
    effect(() => this.panel.set(this.start()));
    effect(() => {
      if (this.panel() !== 'genre') return;
      const input = this.genreInput()?.nativeElement;
      input?.focus();
      input?.select();
    });
  }

  show(panel: TrackMenuPanel) {
    if (panel === 'genre') this.prepareGenre();
    this.panel.set(panel);
  }

  load(deck: number) {
    this.library.load(this.paths()[0], deck);
    this.closed.emit();
  }

  prepare() {
    this.library.addToPrepare(this.paths());
    this.closed.emit();
  }

  addToCrate(id: number) {
    this.library.addToCrate(id, this.paths());
    this.closed.emit();
  }

  setColor(color: number | null) {
    this.library.setColor(this.paths(), color);
    this.closed.emit();
  }

  separate() {
    this.stems.separate(this.toSeparate());
    this.closed.emit();
  }

  installModel() {
    this.navigation.openSettings('library', 'library.stemsModel');
    this.closed.emit();
  }

  remove() {
    this.library.removeSelected();
    this.closed.emit();
  }

  applyGenre(genre: string | null) {
    this.library.setGenre(this.paths(), genre?.trim() || null);
    this.closed.emit();
  }

  /**
   * The genre field starts with the genre the tracks share, else the name of the folder
   * they share (where tracks are often sorted by genre).
   */
  private prepareGenre() {
    this.library.loadGenres();
    const tracks = this.tracks();
    const genres = new Set(tracks.map((t) => t.details?.tags.genre ?? ''));
    const folders = new Set(this.paths().map(folderName));
    const shared = genres.size === 1 ? [...genres][0] : '';
    this.genre.set(shared || (folders.size === 1 ? [...folders][0] : ''));
  }
}

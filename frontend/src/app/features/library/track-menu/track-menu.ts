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
import { EngineService } from '../../../core/services/engine.service';
import { LibraryService } from '../../../core/services/library.service';
import { StemsService } from '../../../core/services/stems.service';
import { LibraryMenu } from '../../../core/models/library';
import { TRACK_COLORS, hexColor } from '../../../shared/track-colors';
import { deckLetter } from '../../../shared/time';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { PHONE } from '../../../core/platform';
import { lastName } from '../../../core/folders';
import { errorText } from '../../../core/errors';

export type TrackMenuPanel = 'main' | 'crates' | 'color' | 'genre';

/**
 * What can be done with tracks, in panels: the actions, then the crates, the colours or the
 * genre to give them. Opened on the selection by a right click on a row or from the selection
 * bar, straight on a panel; on a phone, on the row touched (`paths`). What it offers is the
 * backend's to say (`LibraryMenu`).
 */
@Component({
  selector: 'app-track-menu',
  templateUrl: './track-menu.html',
  styleUrl: './track-menu.scss',
  imports: [TPipe],
})
export class TrackMenu {
  /** On a phone the menu leaves the separation out. */
  protected readonly PHONE = PHONE;
  protected readonly library = inject(LibraryService);
  private readonly engine = inject(EngineService);
  protected readonly stems = inject(StemsService);

  /** The tracks it acts on; `null`: those selected. */
  readonly paths = input.required<string[] | null>();
  readonly start = input<TrackMenuPanel>('main');
  readonly closed = output<void>();

  protected readonly panel = signal<TrackMenuPanel>('main');
  /** What the backend says can be done with the tracks. */
  protected readonly menu = signal<LibraryMenu | null>(null);
  protected readonly decks = this.engine.deckIndexes;
  protected readonly letter = deckLetter;
  protected readonly trackColors = TRACK_COLORS;
  protected readonly hexColor = hexColor;
  protected readonly lastName = lastName;
  protected readonly genre = signal('');
  /** The DJ typed a genre: the one the tracks share does not replace it. */
  private genreTyped = false;
  private readonly genreInput = viewChild<ElementRef<HTMLInputElement>>('genreInput');

  /** Crates tracks can be put in: not the smart ones, which pick their own. */
  protected readonly targetCrates = computed(() => this.library.crates().filter((c) => !c.rules));
  /** Every track is from outside the music folders. */
  protected readonly elsewhere = computed(() => {
    const menu = this.menu();
    return !!menu && menu.elsewhere > 0 && menu.elsewhere === menu.count;
  });
  protected readonly removeLabel = computed(() =>
    this.menu()?.removable?.kind === 'prepare'
      ? t('Remove from the prepare list')
      : t('Remove from the crate'),
  );

  constructor() {
    effect(() => this.panel.set(this.start()));
    effect(() => {
      const paths = this.paths();
      this.menu.set(null);
      this.library.menu(paths).then(
        (menu) => {
          this.menu.set(menu);
          if (!this.genreTyped) this.genre.set(menu.genre);
        },
        (e) => {
          this.library.error.set(errorText(e));
          this.closed.emit();
        },
      );
    });
    effect(() => {
      if (this.panel() !== 'genre') return;
      const input = this.genreInput()?.nativeElement;
      input?.focus();
      input?.select();
    });
  }

  show(panel: TrackMenuPanel) {
    if (panel === 'genre') this.library.loadGenres();
    this.panel.set(panel);
  }

  load(deck: number) {
    const first = this.menu()?.first;
    if (first) this.library.load(first.path, deck);
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

  /** The stems separated of those tracks the models can add to; without a model, the models
   * offered for them. */
  separate() {
    this.closed.emit();
    return this.stems.separate(this.paths());
  }

  remove() {
    this.library.remove(this.paths());
    this.closed.emit();
  }

  showInFileManager() {
    const first = this.menu()?.first;
    if (first) this.library.showInFileManager(first.path);
    this.closed.emit();
  }

  addFolder(folder: string) {
    this.library.addFolder(folder);
    this.closed.emit();
  }

  removeFromPrisme() {
    const menu = this.menu();
    if (menu) this.library.removeFromPrisme(this.paths(), menu);
    this.closed.emit();
  }

  typeGenre(genre: string) {
    this.genreTyped = true;
    this.genre.set(genre);
  }

  applyGenre(genre: string | null) {
    this.library.setGenre(this.paths(), genre);
    this.closed.emit();
  }
}

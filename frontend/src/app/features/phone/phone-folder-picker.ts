import { Component, computed, inject, output, signal, viewChild } from '@angular/core';
import { errorText } from '../../core/errors';
import { folderName, stepsFrom } from '../../core/folders';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { DirListing, StoragePlace } from '../../core/models/library';
import { LibraryService } from '../../core/services/library.service';
import { Dismiss, overlay } from '../../shared/overlay';
import { Sheet } from '../../shared/sheet';

/** What a place is called: by what holds it, a drive by its own name. */
const PLACE_NAME: Record<StoragePlace['kind'], (place: StoragePlace) => string> = {
  phone: () => t('Phone'),
  removable: () => t('SD card or USB drive'),
  home: () => t('Home folder'),
  drive: (place) => place.name,
};

/**
 * A folder to add to the music folders, chosen on a phone, whose own picker gives no path: the
 * phone's storage and each card or USB drive first, then the folders in the one gone into,
 * where they are said at the top. Not offered: Android's folder of application data, and
 * hidden folders.
 */
@Component({
  selector: 'app-phone-folder-picker',
  template: `
    <div class="pr-veil pr-veil--dim" animate.enter="pr-fade-in" (dismiss)="sheet.close()"></div>
    <section
      class="pr-sheet panel"
      prSheet
      #sheet="prSheet"
      (dismissed)="closed.emit()"
      animate.enter="pr-rise-in"
      role="dialog"
      [attr.aria-label]="'Add a folder' | t"
    >
      <header>
        @if (at()) {
          <button
            class="up pr-btn pr-btn--icon pr-btn--ghost"
            [attr.aria-label]="'Up' | t"
            (click)="up()"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>
          </button>
        }
        <div class="where">
          <h4>{{ 'Add a folder' | t }}</h4>
          <p>{{ trail() }}</p>
        </div>
      </header>
      <div class="rows">
        @if (at(); as here) {
          @for (folder of folders(); track folder.path) {
            <button class="row" (click)="go(here.place, folder.path)">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h6l2 2h10v11H3z" /></svg>
              <span class="name">{{ folder.name }}</span>
            </button>
          } @empty {
            <p class="help">{{ 'No folder in it' | t }}</p>
          }
        } @else {
          @for (place of places(); track place.path) {
            <button class="row" (click)="go(place, place.path)">
              <svg viewBox="0 0 24 24" aria-hidden="true">
                @switch (place.kind) {
                  @case ('phone') {
                    <path d="M7 2h10v20H7zM11 18h2" />
                  }
                  @case ('removable') {
                    <path d="M6 2h9l3 3v17H6zM9 6v4M12 6v4M15 6v4" />
                  }
                  @case ('home') {
                    <path d="M3 11l9-8 9 8M5 9v12h14V9" />
                  }
                  @default {
                    <path d="M3 14h18v6H3zM3 14l3-10h12l3 10M17 17h.01" />
                  }
                }
              </svg>
              <span class="name">
                {{ placeName(place) }}
                <small>{{ place.path }}</small>
              </span>
            </button>
          }
        }
      </div>
      @if (at(); as here) {
        <footer>
          @if (error(); as message) {
            <p class="error">{{ message }}</p>
          } @else if (within(); as folder) {
            <p class="help">{{ 'Already among your folders, in {folder}' | t: { folder } }}</p>
          } @else if (here.listing.tracks === 1) {
            <p class="help">{{ '1 track here' | t }}</p>
          } @else if (here.listing.tracks) {
            <p class="help">{{ '{n} tracks here' | t: { n: here.listing.tracks } }}</p>
          }
          <button
            class="pr-btn pr-btn--lg pr-btn--light add"
            [disabled]="adding() || within() !== null"
            (click)="add()"
          >
            {{ 'Add this folder' | t }}
          </button>
        </footer>
      } @else if (error(); as message) {
        <p class="error">{{ message }}</p>
      }
    </section>
  `,
  styles: `
    .panel {
      display: flex;
      flex-direction: column;
      height: 80%;
      padding: 0 16px;
    }
    header {
      flex: none;
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 12px 0 8px;
    }
    .up {
      flex: none;
      width: 44px;
      height: 44px;
      margin-left: -10px;
      svg {
        width: 22px;
        height: 22px;
        fill: none;
        stroke: currentColor;
        stroke-width: 2;
      }
    }
    .where {
      min-width: 0;
      h4 {
        margin: 0;
        font: 600 var(--pr-size-label) / 1 var(--pr-font);
        letter-spacing: 0.08em;
        text-transform: uppercase;
        color: var(--pr-text-3);
      }
      p {
        margin: 6px 0 0;
        overflow-wrap: anywhere;
        font-size: 15px;
        font-weight: 600;
        color: var(--pr-text);
      }
    }
    .rows {
      flex: 1 0 auto;
    }
    .row {
      display: flex;
      align-items: center;
      gap: 14px;
      width: 100%;
      min-height: 52px;
      padding: 6px 0;
      border: 0;
      background: none;
      font: inherit;
      font-size: 15px;
      text-align: left;
      color: var(--pr-text);
      & + & {
        border-top: 1px solid var(--pr-line);
      }
      svg {
        flex: none;
        width: 22px;
        height: 22px;
        fill: none;
        stroke: var(--pr-text-2);
        stroke-width: 1.6;
        stroke-linecap: round;
        stroke-linejoin: round;
      }
    }
    .name {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      small {
        display: block;
        overflow: hidden;
        text-overflow: ellipsis;
        font-size: 12px;
        color: var(--pr-text-3);
      }
    }
    footer {
      flex: none;
      position: sticky;
      bottom: 0;
      display: grid;
      gap: 8px;
      padding: 10px 0 calc(14px + env(safe-area-inset-bottom));
      border-top: 1px solid var(--pr-line);
      background: var(--pr-base);
    }
    .help,
    .error {
      margin: 0;
      font-size: 13px;
      line-height: 1.4;
      color: var(--pr-text-3);
    }
    .rows .help {
      padding: 14px 0;
    }
    .error {
      color: var(--pr-alert);
    }
    .panel > .error {
      padding: 10px 0 calc(14px + env(safe-area-inset-bottom));
    }
  `,
  imports: [Dismiss, Sheet, TPipe],
})
export class PhoneFolderPicker {
  readonly closed = output<void>();
  private readonly sheet = viewChild.required(Sheet);
  private readonly library = inject(LibraryService);

  protected readonly places = signal<StoragePlace[]>([]);
  /** The place gone into and the folder listed in it; `null`: the places. */
  protected readonly at = signal<{ place: StoragePlace; listing: DirListing } | null>(null);
  protected readonly error = signal<string | null>(null);
  protected readonly adding = signal(false);

  protected readonly folders = computed(() => this.at()?.listing.folders ?? []);
  /** Where the folder listed is: "Phone / Download / Sets". */
  protected readonly trail = computed(() => {
    const at = this.at();
    if (!at) return t('Where is the folder?');
    const [, ...steps] = stepsFrom(at.place.path, at.listing.path);
    return [this.placeName(at.place), ...steps.map((step) => step.name)].join(' / ');
  });
  /** The music folder the folder listed is already in, by name. */
  protected readonly within = computed(() => {
    const within = this.at()?.listing.within;
    const folder = this.library.folders().find((f) => f.path === within);
    return folder ? folderName(folder) : null;
  });

  constructor() {
    overlay(
      () => true,
      () => this.sheet().close(),
    );
    this.library.storagePlaces().then(
      (places) => this.places.set(places),
      (e) => this.error.set(errorText(e)),
    );
  }

  protected placeName(place: StoragePlace): string {
    return PLACE_NAME[place.kind](place);
  }

  protected async go(place: StoragePlace, path: string) {
    try {
      this.at.set({ place, listing: await this.library.listDir(path) });
      this.error.set(null);
    } catch (e) {
      this.error.set(errorText(e));
    }
  }

  /** To the folder holding the one listed; from the top of a place, to the places. */
  protected up() {
    const at = this.at();
    this.error.set(null);
    if (!at?.listing.parent || at.listing.path === at.place.path) this.at.set(null);
    else this.go(at.place, at.listing.parent);
  }

  protected async add() {
    const path = this.at()?.listing.path;
    if (!path) return;
    this.adding.set(true);
    try {
      await this.library.joinFolder(path);
      this.sheet().close();
    } catch (e) {
      this.error.set(errorText(e));
    } finally {
      this.adding.set(false);
    }
  }
}

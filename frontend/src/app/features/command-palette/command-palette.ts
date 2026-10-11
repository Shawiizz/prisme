import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  afterRenderEffect,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { ControlService } from '../../core/control/control.service';
import { KeyboardService } from '../../core/keyboard/keyboard.service';
import { CratesService } from '../../core/services/crates.service';
import { LibraryService, trackTitle } from '../../core/services/library.service';
import { NavigationService } from '../../core/services/navigation.service';
import { DECK_COUNT } from '../../core/models/engine';
import { DirEntry } from '../../core/models/library';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { camelotHue, camelotMajor } from '../../shared/camelot';
import { decimal, deckLetter } from '../../shared/time';
import { SETTINGS, SETTINGS_PAGES, normalize } from '../settings/settings-catalog';

type Section = 'actions' | 'settings' | 'crates' | 'tracks';

/** One line of the palette. */
interface Item {
  section: Section;
  title: string;
  detail: string;
  /** Keyboard shortcut of an action. */
  keys?: string;
  track?: DirEntry;
  run(): void;
}

const SECTIONS: { id: Section; label: string; limit: number }[] = [
  { id: 'actions', label: t('Actions'), limit: 8 },
  { id: 'settings', label: t('Settings'), limit: 6 },
  { id: 'crates', label: t('Crates'), limit: 5 },
  { id: 'tracks', label: t('Tracks'), limit: 8 },
];
/** Tracks are looked up once this many characters are typed, after a pause. */
const TRACK_QUERY_MIN = 2;
const TRACK_SEARCH_DELAY_MS = 120;

/**
 * Command palette (Ctrl + K): type to find an action, a setting, a crate or a track, and
 * Enter runs it. A track goes on a free deck, or on A or B with Alt + A / Alt + B.
 */
@Component({
  selector: 'app-command-palette',
  templateUrl: './command-palette.html',
  styleUrl: './command-palette.scss',
  imports: [TPipe],
})
export class CommandPalette {
  private readonly navigation = inject(NavigationService);
  private readonly controls = inject(ControlService);
  private readonly keyboard = inject(KeyboardService);
  private readonly crates = inject(CratesService);
  private readonly library = inject(LibraryService);

  private readonly input = viewChild.required<ElementRef<HTMLInputElement>>('input');
  private readonly list = viewChild.required<ElementRef<HTMLElement>>('list');
  protected readonly query = signal('');
  protected readonly active = signal(0);
  private readonly tracks = signal<DirEntry[]>([]);
  private searchTimer = 0;
  private searchId = 0;

  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
  protected readonly decimal = decimal;

  /** The pages of the settings: what the palette offers before anything is typed. */
  private readonly pages: Item[] = SETTINGS_PAGES.map((p) => ({
    section: 'settings',
    title: p.label,
    detail: p.description,
    run: () => this.navigation.openSettings(p.id),
  }));

  /** Everything the palette can do, before the search. */
  private readonly everything = computed<Item[]>(() => [
    ...this.controls.controls
      .filter((c) => c.kind === 'button' && c.id !== 'general.palette')
      .map((c) => ({
        section: 'actions' as const,
        title: c.label,
        detail: c.group,
        keys: this.keyboard.keysOf(c.id) ?? undefined,
        run: () => {
          c.apply(1);
          c.apply(0);
        },
      })),
    {
      section: 'actions',
      title: t('Analyse new and changed files'),
      detail: t('Library'),
      run: () => this.library.rescan(),
    },
    ...Array.from({ length: DECK_COUNT }, (_, deck) => ({
      section: 'actions' as const,
      title: t('Compare the tracks with deck {deck}', { deck: deckLetter(deck) }),
      detail: t('Library'),
      run: () => this.library.reference.set(deck),
    })),
    ...this.pages,
    ...SETTINGS.map((s) => ({
      section: 'settings' as const,
      title: s.label,
      detail: SETTINGS_PAGES.find((p) => p.id === s.page)?.label ?? '',
      run: () => this.navigation.openSettings(s.page, s.id),
    })),
    ...this.crates.crates().map((c) => ({
      section: 'crates' as const,
      title: c.name,
      detail: t('{n} tracks', { n: c.trackCount }),
      run: () => this.library.openCrate(c.id),
    })),
  ]);

  /** What matches, by section, best first; without a search, the pages of the settings. */
  protected readonly groups = computed(() => {
    const words = normalize(this.query()).split(/\s+/).filter(Boolean);
    const items = words.length
      ? [...this.everything(), ...this.tracks().map((track) => this.trackItem(track))]
      : this.pages;
    const scored = items
      .map((item) => ({ item, score: words.length ? score(words, item) : 1 }))
      .filter((s) => s.score > 0);
    return SECTIONS.map((section) => ({
      ...section,
      items: scored
        .filter((s) => s.item.section === section.id)
        .sort((a, b) => b.score - a.score)
        .slice(0, words.length ? section.limit : undefined)
        .map((s) => s.item),
    })).filter((g) => g.items.length > 0);
  });
  /** The items in the order they are shown, for the arrow keys. */
  protected readonly flat = computed(() => this.groups().flatMap((g) => g.items));

  constructor() {
    afterNextRender(() => this.input().nativeElement.focus());
    afterRenderEffect(() => {
      const index = this.active();
      this.list().nativeElement.querySelector(`[data-index="${index}"]`)?.scrollIntoView({
        block: 'nearest',
      });
    });
    inject(DestroyRef).onDestroy(() => clearTimeout(this.searchTimer));
  }

  type(value: string) {
    this.query.set(value);
    this.active.set(0);
    clearTimeout(this.searchTimer);
    const id = ++this.searchId;
    if (value.trim().length < TRACK_QUERY_MIN) {
      this.tracks.set([]);
      return;
    }
    this.searchTimer = window.setTimeout(async () => {
      const tracks = await invoke<DirEntry[]>('search_library', { query: value.trim() }).catch(
        () => [],
      );
      if (id === this.searchId) this.tracks.set(tracks);
    }, TRACK_SEARCH_DELAY_MS);
  }

  key(event: KeyboardEvent) {
    const count = this.flat().length;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (count)
        this.active.set((this.active() + (event.key === 'ArrowDown' ? 1 : -1) + count) % count);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const item = this.flat()[this.active()];
      if (item) this.run(item);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      this.close();
    } else if (event.altKey && /^[ab]$/i.test(event.key)) {
      const item = this.flat()[this.active()];
      if (!item?.track) return;
      event.preventDefault();
      this.load(item.track, event.key.toLowerCase() === 'a' ? 0 : 1);
    }
  }

  run(item: Item) {
    this.close();
    item.run();
  }

  load(track: DirEntry, deck: number) {
    this.close();
    this.library.load(track.path, deck);
  }

  close() {
    this.navigation.palette.set(false);
  }

  private trackItem(track: DirEntry): Item {
    const tags = track.details?.tags;
    return {
      section: 'tracks',
      title: trackTitle(track),
      detail: tags?.artist ?? '',
      track,
      run: () => this.library.load(track.path, null),
    };
  }
}

/**
 * How well the words match an item: every word must be found in its title or detail,
 * better at the start of a word and in the title. Tracks were found by the library.
 */
function score(words: string[], item: Item): number {
  if (item.section === 'tracks') return 1;
  const title = normalize(item.title);
  const detail = normalize(item.detail);
  let total = 0;
  for (const word of words) {
    const inTitle = title.indexOf(word);
    const inDetail = detail.indexOf(word);
    if (inTitle < 0 && inDetail < 0) return 0;
    const at = inTitle >= 0 ? inTitle : inDetail;
    const text = inTitle >= 0 ? title : detail;
    const wordStart = at === 0 || /[\s\-/(·]/.test(text[at - 1]);
    total += (inTitle >= 0 ? 2 : 1) * (wordStart ? 2 : 1);
  }
  return total;
}

import { Component, computed, inject, signal } from '@angular/core';
import { LibraryService } from '../../core/services/library.service';
import { EngineService } from '../../core/services/engine.service';
import { DirEntry } from '../../core/models/library';
import { DECK_COUNT } from '../../core/models/engine';
import { deckColor } from '../../shared/time';
import { camelotColor, camelotCompatible, camelotRank } from '../../shared/camelot';

type SortColumn = 'name' | 'bpm' | 'key';

/** Folder browser (left) and the tracks of the current folder (right). */
@Component({
  selector: 'app-library',
  templateUrl: './library.html',
  styleUrl: './library.scss',
})
export class Library {
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  protected readonly deckIndexes = Array.from({ length: DECK_COUNT }, (_, i) => i);
  protected readonly color = deckColor;
  protected readonly keyColor = camelotColor;
  protected readonly filter = signal('');
  protected readonly selected = signal<string | null>(null);
  protected readonly sort = signal<{ column: SortColumn; direction: 1 | -1 }>({ column: 'name', direction: 1 });

  protected readonly tracks = computed(() => {
    const all = this.library.listing()?.tracks ?? [];
    const f = this.filter().trim().toLowerCase();
    const shown = f ? all.filter((t) => t.name.toLowerCase().includes(f)) : [...all];
    const { column, direction } = this.sort();
    const value = (t: DirEntry): number | string => {
      switch (column) {
        case 'bpm':
          return t.analysis?.bpm ?? Number.MAX_SAFE_INTEGER;
        case 'key':
          return camelotRank(t.analysis?.key?.camelot);
        default:
          return t.name.toLowerCase();
      }
    };
    return shown.sort((a, b) => {
      const va = value(a);
      const vb = value(b);
      return (va < vb ? -1 : va > vb ? 1 : 0) * direction;
    });
  });

  /** Keys of the tracks on the decks, to point out the harmonic matches. */
  private readonly deckKeys = computed(() =>
    this.engine
      .tracks()
      .map((t) => t?.key?.camelot)
      .filter((k): k is string => !!k),
  );

  protected readonly progress = this.library.progress;
  protected readonly progressPercent = computed(() => {
    const p = this.progress();
    return p.total > 0 ? (p.done / p.total) * 100 : 0;
  });

  /** Path segments from the music root to the current folder, for the breadcrumb. */
  protected readonly crumbs = computed(() => {
    const listing = this.library.listing();
    const root = this.library.root();
    if (!listing || !root) return [];
    const rel = listing.path.startsWith(root) ? listing.path.slice(root.length) : '';
    const parts = rel.split(/[\\/]/).filter(Boolean);
    const sep = root.includes('\\') ? '\\' : '/';
    const crumbs = [{ name: 'Music', path: root }];
    let acc = root;
    for (const p of parts) {
      acc = acc + sep + p;
      crumbs.push({ name: p, path: acc });
    }
    return crumbs;
  });

  open(path: string) {
    this.filter.set('');
    this.library.open(path);
  }

  load(track: DirEntry, deck: number) {
    this.selected.set(track.path);
    this.engine.load(deck, track.path);
  }

  /** Double-click: first empty deck, else deck 1. */
  loadDefault(track: DirEntry) {
    const empty = this.engine.tracks().findIndex((t) => !t);
    this.load(track, empty >= 0 ? empty : 0);
  }

  /** Click on a column title: sort by it, again to reverse. */
  sortBy(column: SortColumn) {
    const s = this.sort();
    this.sort.set({ column, direction: s.column === column ? (-s.direction as 1 | -1) : 1 });
  }

  sortMark(column: SortColumn): string {
    const s = this.sort();
    return s.column === column ? (s.direction === 1 ? '▲' : '▼') : '';
  }

  /** Whether the key mixes with a loaded track (`null` when no deck has a key). */
  keyMatches(camelot: string): boolean | null {
    const keys = this.deckKeys();
    return keys.length ? keys.some((k) => camelotCompatible(k, camelot)) : null;
  }

  displayName(name: string): string {
    return name.replace(/\.[^.]+$/, '');
  }

  size(bytes: number): string {
    return (bytes / 1_048_576).toFixed(1) + ' MB';
  }
}

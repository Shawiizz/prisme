import { Component, ElementRef, computed, effect, inject, viewChild } from '@angular/core';
import { LibraryService, SortColumn, trackTitle } from '../../core/services/library.service';
import { EngineService } from '../../core/services/engine.service';
import { DECK_COUNT } from '../../core/models/engine';
import { deckColor } from '../../shared/time';
import { camelotColor, camelotCompatible } from '../../shared/camelot';
import { Cover } from '../../shared/cover/cover';

const COLUMNS: { id: SortColumn; label: string }[] = [
  { id: 'title', label: 'Title' },
  { id: 'artist', label: 'Artist' },
  { id: 'album', label: 'Album' },
  { id: 'genre', label: 'Genre' },
  { id: 'year', label: 'Year' },
  { id: 'bpm', label: 'BPM' },
  { id: 'key', label: 'Key' },
];

/** Folder browser (left) and the tracks of the current folder or library search (right). */
@Component({
  selector: 'app-library',
  templateUrl: './library.html',
  styleUrl: './library.scss',
  imports: [Cover],
})
export class Library {
  protected readonly library = inject(LibraryService);
  protected readonly engine = inject(EngineService);
  protected readonly deckIndexes = Array.from({ length: DECK_COUNT }, (_, i) => i);
  protected readonly color = deckColor;
  protected readonly keyColor = camelotColor;
  protected readonly columns = COLUMNS;
  protected readonly title = trackTitle;
  protected readonly tracks = this.library.visibleTracks;
  private readonly search = viewChild.required<ElementRef<HTMLInputElement>>('search');
  private readonly host = inject(ElementRef<HTMLElement>);

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

  constructor() {
    effect(() => {
      if (this.library.searchRequests() > 0) this.search().nativeElement.focus();
    });
    effect(() => {
      const path = this.library.selected();
      const row =
        path && this.host.nativeElement.querySelector(`[data-path="${CSS.escape(path)}"]`);
      row?.scrollIntoView({ block: 'nearest' });
    });
  }

  sortMark(column: SortColumn): string {
    const s = this.library.sort();
    return s.column === column ? (s.direction === 1 ? '▲' : '▼') : '';
  }

  /** Whether the key mixes with a loaded track (`null` when no deck has a key). */
  keyMatches(camelot: string): boolean | null {
    const keys = this.deckKeys();
    return keys.length ? keys.some((k) => camelotCompatible(k, camelot)) : null;
  }

  protected readonly emptyMessage = computed(() => {
    if (this.library.scope() === 'folder') return 'No audio file in this folder';
    return this.library.filter().trim()
      ? 'No analysed track matches'
      : 'Type to search every analysed track';
  });
}

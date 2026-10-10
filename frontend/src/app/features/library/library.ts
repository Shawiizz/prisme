import { Component, computed, inject, signal } from '@angular/core';
import { LibraryService } from '../../core/services/library.service';
import { EngineService } from '../../core/services/engine.service';
import { DirEntry } from '../../core/models/library';
import { DECK_COUNT } from '../../core/models/engine';
import { deckColor } from '../../shared/time';

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
  protected readonly filter = signal('');
  protected readonly selected = signal<string | null>(null);

  protected readonly tracks = computed(() => {
    const all = this.library.listing()?.tracks ?? [];
    const f = this.filter().trim().toLowerCase();
    return f ? all.filter((t) => t.name.toLowerCase().includes(f)) : all;
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

  displayName(name: string): string {
    return name.replace(/\.[^.]+$/, '');
  }

  size(bytes: number): string {
    return (bytes / 1_048_576).toFixed(1) + ' MB';
  }
}

import { computed, effect, inject, signal, untracked } from '@angular/core';
import { folderName, stepsFrom } from '../../../core/folders';
import { Folder, MusicFolder } from '../../../core/models/library';
import { LibraryService } from '../../../core/services/library.service';

const EXPANDED_KEY = 'prisme.sidebar.expanded';

/** A line of the tree: a music folder at the top of its own, or a folder in it. */
export interface FolderRow {
  path: string;
  name: string;
  depth: number;
  /** The music folder, on the line at the top of its tree. */
  root: MusicFolder | null;
  open: boolean;
  /** Known to hold no folder, or not there: nothing to open. */
  leaf: boolean;
}

/** The folders opened, as left last time. */
function savedExpanded(): string[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(EXPANDED_KEY) ?? '[]');
    return Array.isArray(saved) ? saved.filter((p): p is string => typeof p === 'string') : [];
  } catch {
    return [];
  }
}

/**
 * The music folders as trees: a folder opens on the folders in it, listed each time it opens.
 * The folders on the way to the one browsed open with it; those opened stay open from one
 * start to the next. To be made where `inject` works.
 */
export class FolderTree {
  private readonly library = inject(LibraryService);
  private readonly expanded = signal<ReadonlySet<string>>(new Set(savedExpanded()));
  /** The folders in each folder listed so far. */
  private readonly children = signal<ReadonlyMap<string, Folder[]>>(new Map());
  /** Folders being listed. */
  private readonly listing = new Set<string>();

  readonly rows = computed(() => {
    const rows: FolderRow[] = [];
    const add = (path: string, name: string, depth: number, root: MusicFolder | null) => {
      const children = this.children().get(path);
      const absent = root?.present === false;
      const open = !absent && this.expanded().has(path);
      rows.push({ path, name, depth, root, open, leaf: absent || children?.length === 0 });
      if (open) for (const c of children ?? []) add(c.path, c.name, depth + 1, null);
    };
    for (const folder of this.library.folders()) add(folder.path, folderName(folder), 0, folder);
    return rows;
  });

  constructor() {
    effect(() => {
      const folder = this.library.currentFolder();
      if (folder) untracked(() => this.known(folder.path, folder.folders));
    });
    effect(() => {
      const folder = this.library.currentFolder();
      if (!folder?.root) return;
      const way = stepsFrom(folder.root, folder.path).slice(0, -1);
      untracked(() => this.expand(way.map((step) => step.path)));
    });
    // opened last time, or on the way: listed once shown
    effect(() => {
      const unknown = this.rows().filter((row) => row.open && !this.children().has(row.path));
      untracked(() => unknown.forEach((row) => this.list(row.path)));
    });
  }

  toggle(path: string) {
    const expanded = new Set(this.expanded());
    if (expanded.delete(path)) {
      this.save(expanded);
    } else {
      this.save(expanded.add(path));
      this.list(path);
    }
  }

  private expand(paths: string[]) {
    const expanded = this.expanded();
    if (paths.every((p) => expanded.has(p))) return;
    this.save(new Set([...expanded, ...paths]));
  }

  private save(expanded: ReadonlySet<string>) {
    this.expanded.set(expanded);
    try {
      localStorage.setItem(EXPANDED_KEY, JSON.stringify([...expanded]));
    } catch {
      // storage unavailable: open until the next start only
    }
  }

  private async list(path: string) {
    if (this.listing.has(path)) return;
    this.listing.add(path);
    try {
      this.known(path, (await this.library.listDir(path)).folders);
    } catch {
      // gone meanwhile: nothing in it to open
      this.known(path, []);
    } finally {
      this.listing.delete(path);
    }
  }

  private known(path: string, folders: Folder[]) {
    this.children.update((children) => new Map(children).set(path, folders));
  }
}

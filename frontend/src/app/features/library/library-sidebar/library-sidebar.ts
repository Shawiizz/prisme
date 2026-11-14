import { Component, ElementRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { CratesService } from '../../../core/services/crates.service';
import { HistoryService } from '../../../core/services/history.service';
import { LibraryService } from '../../../core/services/library.service';
import { PrepareService } from '../../../core/services/prepare.service';
import { TRACKS_DRAG_TYPE } from '../../../shared/drag';
import { Crate } from '../../../core/models/library';
import { SmartCrateDialog } from '../smart-crate-dialog/smart-crate-dialog';
import { locale, t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';

type Deletable = 'crate' | 'session';
type DropTarget = number | 'prepare';
type Foldable = 'crates' | 'history';

const FOLDED_KEY = 'prisme.sidebar.folded';

/** The sections the user folded, as left last time. */
function savedFolded(): Foldable[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(FOLDED_KEY) ?? '[]');
    return Array.isArray(saved)
      ? saved.filter((s): s is Foldable => s === 'crates' || s === 'history')
      : [];
  } catch {
    return [];
  }
}

const SESSION_DATE = computed(
  () =>
    new Intl.DateTimeFormat(locale(), {
      weekday: 'short',
      day: 'numeric',
      month: 'short',
      hour: '2-digit',
      minute: '2-digit',
    }),
);

/**
 * Left panel of the library: the prepare list, the folders, the crates (tracks are dropped
 * on them) and the history of the sets, the last two foldable.
 */
@Component({
  selector: 'app-library-sidebar',
  templateUrl: './library-sidebar.html',
  styleUrl: './library-sidebar.scss',
  imports: [SmartCrateDialog, TPipe],
})
export class LibrarySidebar {
  protected readonly library = inject(LibraryService);
  protected readonly crates = inject(CratesService).crates;
  protected readonly sessions = inject(HistoryService).sessions;
  private readonly prepare = inject(PrepareService).list;
  protected readonly prepareCount = computed(() => this.prepare().length);

  /** The smart crate dialog, open on a crate (`null`: a new one). */
  protected readonly smartDialog = signal<{ crate: Crate | null } | null>(null);
  /** Crate whose name is being typed: `'new'` for a crate being created. */
  protected readonly editing = signal<number | 'new' | null>(null);
  /** Item whose delete button was clicked once: a second click deletes it. */
  private readonly armed = signal<{ kind: Deletable; id: number } | null>(null);
  private readonly folded = signal<Foldable[]>(savedFolded());
  /** Where dragged tracks would go: a crate or the prepare list. */
  protected readonly dropTarget = signal<DropTarget | null>(null);
  private readonly nameInput = viewChild<ElementRef<HTMLInputElement>>('nameInput');

  protected readonly browsingFolder = computed(() => this.library.source().kind === 'folder');

  /** Path segments from the music root to the current folder, for the breadcrumb. */
  protected readonly crumbs = computed(() => {
    const listing = this.library.listing();
    const root = this.library.root();
    if (!listing || !root) return [];
    const rel = listing.path.startsWith(root) ? listing.path.slice(root.length) : '';
    const parts = rel.split(/[\\/]/).filter(Boolean);
    const sep = root.includes('\\') ? '\\' : '/';
    const crumbs = [{ name: t('Music'), path: root }];
    let acc = root;
    for (const p of parts) {
      // a root at the top of a drive (D:\) ends with the separator already
      acc = acc.endsWith(sep) ? acc + p : acc + sep + p;
      crumbs.push({ name: p, path: acc });
    }
    return crumbs;
  });

  constructor() {
    effect(() => {
      const input = this.nameInput()?.nativeElement;
      input?.focus();
      input?.select();
    });
  }

  isFolded(section: Foldable): boolean {
    return this.folded().includes(section);
  }

  toggle(section: Foldable) {
    this.folded.update((f) =>
      f.includes(section) ? f.filter((s) => s !== section) : [...f, section],
    );
    try {
      localStorage.setItem(FOLDED_KEY, JSON.stringify(this.folded()));
    } catch {
      // storage unavailable: folded until the next start only
    }
  }

  newCrate() {
    if (this.isFolded('crates')) this.toggle('crates');
    this.editing.set('new');
  }

  open(id: number) {
    this.armed.set(null);
    this.library.openCrate(id);
  }

  /** `Sat 10 Oct, 21:30` */
  sessionLabel(ms: number): string {
    return SESSION_DATE().format(ms);
  }

  async commitEdit(text: string) {
    const editing = this.editing();
    this.editing.set(null);
    const name = text.trim();
    if (!name || editing === null) return;
    if (editing === 'new') await this.library.createCrate(name);
    else await this.library.renameCrate(editing, name);
  }

  isArmed(kind: Deletable, id: number): boolean {
    const armed = this.armed();
    return armed?.kind === kind && armed.id === id;
  }

  delete(kind: Deletable, id: number) {
    if (!this.isArmed(kind, id)) {
      this.armed.set({ kind, id });
      return;
    }
    this.armed.set(null);
    if (kind === 'crate') this.library.deleteCrate(id);
    else this.library.deleteSession(id);
  }

  /** A smart crate picks its own tracks: nothing can be dropped on it. */
  dragOverCrate(event: DragEvent, crate: Crate) {
    if (!crate.rules) this.dragOver(event, crate.id);
  }

  dragOver(event: DragEvent, target: DropTarget) {
    if (!event.dataTransfer?.types.includes(TRACKS_DRAG_TYPE)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
    this.dropTarget.set(target);
  }

  drop(event: DragEvent, target: DropTarget) {
    this.dropTarget.set(null);
    const data = event.dataTransfer?.getData(TRACKS_DRAG_TYPE);
    if (!data) return;
    event.preventDefault();
    const paths = JSON.parse(data) as string[];
    if (target === 'prepare') this.library.addToPrepare(paths);
    else this.library.addToCrate(target, paths);
  }
}

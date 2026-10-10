import { Component, ElementRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { CratesService } from '../../../core/services/crates.service';
import { HistoryService } from '../../../core/services/history.service';
import { LibraryService } from '../../../core/services/library.service';
import { TRACKS_DRAG_TYPE } from '../../../shared/drag';

type Deletable = 'crate' | 'session';

const SESSION_DATE = new Intl.DateTimeFormat('en-GB', {
  weekday: 'short',
  day: 'numeric',
  month: 'short',
  hour: '2-digit',
  minute: '2-digit',
});

/** Left panel of the library: the crates (tracks are dropped on them), the history of the sets and the folders. */
@Component({
  selector: 'app-library-sidebar',
  templateUrl: './library-sidebar.html',
  styleUrl: './library-sidebar.scss',
})
export class LibrarySidebar {
  protected readonly library = inject(LibraryService);
  protected readonly crates = inject(CratesService).crates;
  protected readonly sessions = inject(HistoryService).sessions;

  /** Crate whose name is being typed: `'new'` for a crate being created. */
  protected readonly editing = signal<number | 'new' | null>(null);
  /** Item whose delete button was clicked once: a second click deletes it. */
  private readonly armed = signal<{ kind: Deletable; id: number } | null>(null);
  protected readonly dropTarget = signal<number | null>(null);
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
      const input = this.nameInput()?.nativeElement;
      input?.focus();
      input?.select();
    });
  }

  open(id: number) {
    this.armed.set(null);
    this.library.openCrate(id);
  }

  /** `Sat 10 Oct, 21:30` */
  sessionLabel(ms: number): string {
    return SESSION_DATE.format(ms);
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

  dragOver(event: DragEvent, id: number) {
    if (!event.dataTransfer?.types.includes(TRACKS_DRAG_TYPE)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
    this.dropTarget.set(id);
  }

  drop(event: DragEvent, id: number) {
    this.dropTarget.set(null);
    const data = event.dataTransfer?.getData(TRACKS_DRAG_TYPE);
    if (!data) return;
    event.preventDefault();
    this.library.addToCrate(id, JSON.parse(data) as string[]);
  }
}

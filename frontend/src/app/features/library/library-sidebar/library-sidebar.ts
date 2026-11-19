import {
  Component,
  ElementRef,
  computed,
  effect,
  inject,
  output,
  signal,
  viewChild,
} from '@angular/core';
import { CratesService } from '../../../core/services/crates.service';
import { HistoryService } from '../../../core/services/history.service';
import { LibraryService } from '../../../core/services/library.service';
import { PrepareService } from '../../../core/services/prepare.service';
import { AutoDjService } from '../../../core/services/auto-dj.service';
import { TRACKS_DRAG_TYPE } from '../../../shared/drag';
import { Crate, MusicFolder } from '../../../core/models/library';
import { FileDropService, fileDropAttribute } from '../../../core/services/file-drop.service';
import { shortPath } from '../../../core/folders';
import { errorText } from '../../../core/errors';
import { PHONE } from '../../../core/platform';
import { Dismiss, overlay } from '../../../shared/overlay';
import { Popover, popoverBeside } from '../../../shared/popover';
import { FolderRow, FolderTree } from './folder-tree';
import { SmartCrateDialog } from '../smart-crate-dialog/smart-crate-dialog';
import { locale, t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { dragSize, saveSize, savedSize } from '../../../shared/resize';

type Deletable = 'crate' | 'session';
type DropTarget = number | 'prepare';
type Foldable = 'folders' | 'crates' | 'history';
type FolderAction = 'show' | 'rescan' | 'remove';

const FOLDED_KEY = 'prisme.sidebar.folded';

/** The sections whose height the DJ sets. */
type Sized = 'crates' | 'history';
const HEIGHTS_KEY = 'prisme.sidebar.heights';
/** A section keeps its header and a row. */
const LEAST_HEIGHT = 64;

/** The sections the user folded, as left last time. */
function savedFolded(): Foldable[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(FOLDED_KEY) ?? '[]');
    return Array.isArray(saved)
      ? saved.filter((s): s is Foldable => s === 'folders' || s === 'crates' || s === 'history')
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
 * Left panel of the library: the prepare list, the music folders as trees and the tracks from
 * elsewhere after them, the crates (tracks are dropped on them) and the history of the sets,
 * the last three foldable. Files from the
 * file manager are dropped on the prepare list and the crates too, folders on the music
 * folders. A phone picks a folder to add in its own sheet (`pickFolder`).
 */
@Component({
  selector: 'app-library-sidebar',
  templateUrl: './library-sidebar.html',
  styleUrl: './library-sidebar.scss',
  imports: [Dismiss, SmartCrateDialog, TPipe],
})
export class LibrarySidebar {
  /** On a phone: a folder to add is to be picked. */
  readonly pickFolder = output<void>();

  protected readonly PHONE = PHONE;
  protected readonly library = inject(LibraryService);
  protected readonly drops = inject(FileDropService);
  protected readonly tree = new FolderTree();
  protected readonly fileDrop = fileDropAttribute;
  protected readonly crates = inject(CratesService).crates;
  protected readonly sessions = inject(HistoryService).sessions;
  private readonly prepare = inject(PrepareService).list;
  protected readonly prepareCount = computed(() => this.prepare().length);
  /** Playing the prepare list, said beside it wherever the library is. */
  protected readonly autoDj = inject(AutoDjService);

  /** The smart crate dialog, open on a crate (`null`: a new one). */
  protected readonly smartDialog = signal<{ crate: Crate | null } | null>(null);
  /** Crate whose name is being typed: `'new'` for a crate being created. */
  protected readonly editing = signal<number | 'new' | null>(null);
  /** Item whose delete button was clicked once: a second click deletes it. */
  private readonly armed = signal<{ kind: Deletable; id: number } | null>(null);
  private readonly folded = signal<Foldable[]>(savedFolded());
  /** The heights the DJ gave the sections (px); a section without one takes its own. */
  protected readonly heights = signal<Partial<Record<Sized, number>>>(savedSize(HEIGHTS_KEY) ?? {});
  private readonly host: HTMLElement = inject(ElementRef).nativeElement;
  /** Where dragged tracks would go: a crate or the prepare list. */
  protected readonly dropTarget = signal<DropTarget | null>(null);
  private readonly nameInput = viewChild<ElementRef<HTMLInputElement>>('nameInput');

  /** The menu of a music folder, opened from its button. */
  protected readonly folderMenu = signal<(Popover & { folder: MusicFolder }) | null>(null);

  /** The folder browsed, lit in the tree. */
  protected readonly currentFolder = computed(() =>
    this.library.source().kind === 'folder' ? (this.library.listing()?.path ?? null) : null,
  );

  constructor() {
    effect(() => {
      const input = this.nameInput()?.nativeElement;
      input?.focus();
      input?.select();
    });
    overlay(
      () => this.folderMenu() !== null,
      () => this.folderMenu.set(null),
    );
  }

  /** Drags the line above `section`: it grows as the line goes up, as tall as its rows or into
   * the room left free at the bottom of the column. */
  resize(event: PointerEvent, section: Sized) {
    const bounds = (element: Element) => element.getBoundingClientRect();
    const element = (event.currentTarget as HTMLElement).parentElement!;
    const from = bounds(element).height;
    const list = element.querySelector('.list')!;
    const whole = from - list.clientHeight + list.scrollHeight;
    const last = this.host.querySelector(':scope > section:last-of-type')!;
    const most = Math.max(
      whole,
      from + Math.max(bounds(this.host).bottom - bounds(last).bottom, 0),
    );
    dragSize(
      event,
      from,
      'y',
      (wanted) => {
        this.heights.update((h) => ({
          ...h,
          [section]: Math.round(Math.min(Math.max(wanted, LEAST_HEIGHT), most)),
        }));
        saveSize(HEIGHTS_KEY, this.heights());
      },
      -1,
    );
  }

  /** The section back to its own height. */
  unsize(section: Sized) {
    this.heights.update(({ [section]: _, ...others }) => others);
    saveSize(HEIGHTS_KEY, this.heights());
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

  /** Where a music folder is, said under its name. */
  place(folder: MusicFolder): string {
    const path = shortPath(folder.path);
    return folder.present ? path : `${path} · ${t('unplugged')}`;
  }

  /** A folder on a drive not plugged in cannot be browsed. */
  openFolder(row: FolderRow) {
    if (row.root?.present !== false) this.library.open(row.path);
  }

  openFolderMenu(event: MouseEvent, folder: MusicFolder) {
    this.folderMenu.set({ ...popoverBeside(event.currentTarget as HTMLElement), folder });
  }

  /** Does what was chosen in the menu of a music folder, and closes it. */
  fromFolderMenu(action: FolderAction) {
    const folder = this.folderMenu()?.folder;
    this.folderMenu.set(null);
    if (!folder) return;
    if (action === 'show') this.library.showInFileManager(folder.path);
    else if (action === 'rescan')
      this.library.rescan().catch((e) => this.library.error.set(errorText(e)));
    else this.library.removeFolder(folder);
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

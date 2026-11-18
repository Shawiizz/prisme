import { Injectable, inject, signal } from '@angular/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { TRACKS_DRAG_TYPE } from '../../shared/drag';
import { isAudioFile } from '../audio-files';
import { lastName } from '../folders';
import { PHONE } from '../platform';
import { LibraryService } from './library.service';

/** Where files from the file manager are dropped. */
export type FileDropTarget =
  | { kind: 'deck'; deck: number }
  | { kind: 'prepare' }
  | { kind: 'crate'; id: number }
  | { kind: 'folders' };

/** What is dragged over the window, as far as it can be told before the drop. */
export interface FileDrag {
  /** Tracks, for the decks and the lists. */
  audio: boolean;
  /** Folders, for the music folders. */
  folders: boolean;
  /** The name of the folder dragged, when it is one and its name is known. */
  name: string | null;
}

/** Files dropped, with what they were dropped on (`files-dropped`, Windows). */
interface FilesDropped {
  target: unknown;
  paths: string[];
}

/** WebView2's link to the application (Windows): files posted through it keep their paths. */
interface WebView2 {
  postMessageWithAdditionalObjects?(message: unknown, objects: FileList): void;
}

/** The attribute of the elements files are dropped on: what they take, as JSON. */
const TARGET_ATTRIBUTE = 'data-file-drop';

/** The value of `data-file-drop` on an element that takes files dropped as `target`. */
export function fileDropAttribute(target: FileDropTarget): string {
  return JSON.stringify(target);
}

function sameTarget(a: FileDropTarget | null, b: FileDropTarget | null): boolean {
  return a === b || (a !== null && b !== null && fileDropAttribute(a) === fileDropAttribute(b));
}

/** `value` if it is a target. */
function asTarget(value: unknown): FileDropTarget | null {
  if (typeof value !== 'object' || value === null) return null;
  const { kind, deck, id } = value as Record<string, unknown>;
  if (kind === 'deck' && typeof deck === 'number') return { kind, deck };
  if (kind === 'crate' && typeof id === 'number') return { kind, id };
  if (kind === 'prepare' || kind === 'folders') return { kind };
  return null;
}

/** What the element or the nearest around it takes. */
function targetOf(element: EventTarget | null): FileDropTarget | null {
  if (!(element instanceof Element)) return null;
  const value = element.closest(`[${TARGET_ATTRIBUTE}]`)?.getAttribute(TARGET_ATTRIBUTE);
  return value ? asTarget(JSON.parse(value)) : null;
}

/** Files dragged from outside the page, not its own tracks. */
function fromOutside(data: DataTransfer): boolean {
  return data.types.includes('Files') && !data.types.includes(TRACKS_DRAG_TYPE);
}

/** Before the drop, the page knows the dragged files by their types only: a folder has none,
 * nor has a file of a type the browser does not know (it may be audio). */
function draggedTypes(items: DataTransferItemList): FileDrag {
  const types = [...items].filter((item) => item.kind === 'file').map((item) => item.type);
  return {
    audio: types.some((type) => type === '' || type.startsWith('audio/') || type === 'video/mp4'),
    folders: types.includes(''),
    name: null,
  };
}

/** Anything that is not a track may be a folder. */
function draggedPaths(paths: string[]): FileDrag {
  const folders = paths.filter((path) => !isAudioFile(path));
  return {
    audio: folders.length < paths.length,
    folders: folders.length > 0,
    name: folders.length === 1 ? lastName(folders[0]) : null,
  };
}

/**
 * Files and folders dragged from the computer's file manager. While they are over the window
 * the decks, the lists and the music folders say what a drop would do there, the one under
 * the pointer lit; dropped, a track loads on a deck, tracks join a list, folders join the
 * music folders. The window tells the drag and the paths on macOS and Linux. On Windows the
 * page handles drops itself (its tracks drag onto the crates), and the paths of the files it
 * gets are read by the application through WebView2 (`files-dropped`).
 */
@Injectable({ providedIn: 'root' })
export class FileDropService {
  private readonly library = inject(LibraryService);
  /** What is being dragged over the window; `null` while nothing comes from outside. */
  readonly dragged = signal<FileDrag | null>(null);
  /** Where it would be dropped, if dropping it there does something. */
  readonly over = signal<FileDropTarget | null>(null, { equal: sameTarget });

  async start(): Promise<void> {
    if (PHONE) return;
    const webview = (window as { chrome?: { webview?: WebView2 } }).chrome?.webview;
    if (webview?.postMessageWithAdditionalObjects) {
      this.followPage(webview.postMessageWithAdditionalObjects.bind(webview));
      await listen<FilesDropped>('files-dropped', ({ payload }) => {
        const target = asTarget(payload.target);
        if (target) void this.drop(target, payload.paths);
      });
    } else {
      await this.followWindow();
    }
  }

  /** Whether files dropped now would go to `target`. */
  isOver(target: FileDropTarget): boolean {
    return sameTarget(this.over(), target);
  }

  private followPage(post: (message: unknown, files: FileList) => void) {
    document.addEventListener('dragover', (event) => {
      const data = event.dataTransfer;
      if (!data || !fromOutside(data)) return;
      // a file dropped anywhere else must not open in the window
      event.preventDefault();
      if (!this.dragged()) this.dragged.set(draggedTypes(data.items));
      data.dropEffect = this.point(event.target) ? 'copy' : 'none';
    });
    document.addEventListener('dragleave', (event) => {
      if (event.relatedTarget === null) this.end();
    });
    document.addEventListener('drop', (event) => {
      const data = event.dataTransfer;
      if (!data || !fromOutside(data)) return;
      event.preventDefault();
      const target = this.point(event.target);
      this.end();
      if (target) post({ filesDropped: target }, data.files);
    });
  }

  private async followWindow() {
    await getCurrentWebview().onDragDropEvent(({ payload }) => {
      // the page's own tracks pass here too, without paths
      if (payload.type === 'enter' && payload.paths.length) {
        this.dragged.set(draggedPaths(payload.paths));
      }
      if (payload.type === 'leave') {
        this.end();
        return;
      }
      // in the page's pixels, though called physical: the view's points
      const target = this.point(document.elementFromPoint(payload.position.x, payload.position.y));
      if (payload.type === 'drop') {
        this.end();
        if (target) void this.drop(target, payload.paths);
      }
    });
  }

  /** Lights what is under the pointer if dropping there does something, and gives it. */
  private point(element: EventTarget | null): FileDropTarget | null {
    const dragged = this.dragged();
    const target = targetOf(element);
    const takes = target?.kind === 'folders' ? dragged?.folders : dragged?.audio;
    this.over.set(takes ? target : null);
    return this.over();
  }

  private end() {
    this.dragged.set(null);
    this.over.set(null);
  }

  /** The first track onto a deck, the tracks into a list, the folders into the music folders. */
  private async drop(target: FileDropTarget, paths: string[]) {
    const tracks = paths.filter(isAudioFile);
    if (target.kind === 'folders') {
      for (const folder of paths.filter((path) => !isAudioFile(path))) {
        await this.library.addFolder(folder);
      }
    } else if (!tracks.length) {
      return;
    } else if (target.kind === 'deck') {
      await this.library.load(tracks[0], target.deck);
    } else if (target.kind === 'prepare') {
      await this.library.addToPrepare(tracks);
    } else {
      await this.library.addToCrate(target.id, tracks);
    }
  }
}

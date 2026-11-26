import { Directive, ElementRef, computed, inject, input, signal } from '@angular/core';
import { LibraryService } from '../../../core/services/library.service';
import { fileDropAttribute } from '../../../core/services/file-drop.service';
import { TRACKS_DRAG_TYPE } from '../../../shared/drag';

/**
 * Where a track dragged from the library or the file manager is loaded on deck `deck`: its
 * host, outlined while a track is over it.
 */
@Directive({
  selector: '[appDeckDropTarget]',
  host: {
    '[class.dropping]': 'dropping()',
    '[attr.data-file-drop]': 'fileDrop()',
    '(dragover)': 'dragOver($event)',
    '(dragleave)': 'dragLeave($event)',
    '(drop)': 'drop($event)',
  },
})
export class DeckDropTarget {
  readonly deck = input.required<number>({ alias: 'appDeckDropTarget' });
  private readonly library = inject(LibraryService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  protected readonly dropping = signal(false);
  protected readonly fileDrop = computed(() =>
    fileDropAttribute({ kind: 'deck', deck: this.deck() }),
  );

  protected dragOver(event: DragEvent) {
    if (!event.dataTransfer?.types.includes(TRACKS_DRAG_TYPE)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
    this.dropping.set(true);
  }

  protected dragLeave(event: DragEvent) {
    if (!this.host.contains(event.relatedTarget as Node | null)) this.dropping.set(false);
  }

  /** The track dragged from the library: a deck plays one. */
  protected drop(event: DragEvent) {
    this.dropping.set(false);
    const path = event.dataTransfer?.getData(TRACKS_DRAG_TYPE);
    if (!path) return;
    event.preventDefault();
    this.library.load(path, this.deck());
  }
}

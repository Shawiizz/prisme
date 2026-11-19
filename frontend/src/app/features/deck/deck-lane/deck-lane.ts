import {
  Component,
  ElementRef,
  booleanAttribute,
  computed,
  inject,
  input,
  signal,
} from '@angular/core';
import { LibraryService } from '../../../core/services/library.service';
import { MixerService } from '../../../core/services/mixer.service';
import { fileDropAttribute } from '../../../core/services/file-drop.service';
import { TRACKS_DRAG_TYPE } from '../../../shared/drag';
import { ChannelSlot } from '../../mixer/channel-slot/channel-slot';
import { DeckCard } from '../deck-card/deck-card';
import { DeckDrop } from '../deck-drop/deck-drop';
import { DeckControls } from '../deck-controls/deck-controls';
import { DeckPads } from '../deck-pads/deck-pads';
import { DeckWaveform } from '../deck-waveform/deck-waveform';
import { PitchFader } from '../pitch-fader/pitch-fader';

/**
 * Everything about a deck in one band, read from left to right: its panel, its waveform,
 * its channel when the software mixer runs, its pitch fader at the end where a turntable
 * has it; below, a rail with what plays it and its pads. A track dragged from the library
 * or the file manager onto it is loaded there.
 */
@Component({
  selector: 'app-deck-lane',
  template: `
    <app-deck-card [deck]="deck()" [compact]="compact()" />
    <app-deck-waveform [deck]="deck()" [compact]="compact()" />
    @if (mixer.internal()) {
      <app-channel-slot [deck]="deck()" [compact]="compact()" />
    }
    <app-pitch-fader [deck]="deck()" [compact]="compact()" />
    <div class="rail pr-panel">
      <app-deck-controls [deck]="deck()" />
      <i class="divider"></i>
      <app-deck-pads [deck]="deck()" />
    </div>
    <app-deck-drop [deck]="deck()" />
  `,
  styles: `
    :host {
      position: relative;
      display: grid;
      grid-template-columns: var(--pr-lane-columns);
      grid-template-rows: var(--pr-lane-top, 124px) 44px;
      gap: 4px 10px;
      min-width: 0;
      transition:
        grid-template-rows var(--pr-relayout),
        grid-template-columns var(--pr-relayout);
    }
    app-pitch-fader {
      grid-column: -2;
      grid-row: 1 / span 2;
    }
    // four decks: lower, though taller on a taller window; the channel of the software mixer
    // over both rows
    :host(.compact) {
      grid-template-rows: var(--pr-lane-top-compact, clamp(48px, 4.4vh + 16px, 76px)) 44px;
      app-channel-slot {
        grid-column: 3;
        grid-row: 1 / span 2;
      }
      .rail {
        grid-column: 1 / 3;
      }
    }
    .rail {
      grid-column: 1 / -2;
      grid-row: 2;
      container: rail / inline-size;
      display: flex;
      align-items: center;
      gap: 5px;
      padding: 0 4px;
      min-width: 0;
    }
    :host(.dropping) {
      outline: 1px solid var(--pr-light);
      outline-offset: 2px;
      border-radius: var(--pr-radius);
    }
    .divider {
      align-self: stretch;
      width: 1px;
      margin: 6px 3px;
      background: var(--pr-line);
    }
  `,
  host: {
    '[class.compact]': 'compact()',
    '[class.dropping]': 'dropping()',
    '[attr.data-file-drop]': 'fileDrop()',
    '(dragover)': 'dragOver($event)',
    '(dragleave)': 'dragLeave($event)',
    '(drop)': 'drop($event)',
  },
  imports: [ChannelSlot, DeckCard, DeckControls, DeckDrop, DeckPads, DeckWaveform, PitchFader],
})
export class DeckLane {
  readonly deck = input.required<number>();
  /** Half the height, for four decks. */
  readonly compact = input(false, { transform: booleanAttribute });
  protected readonly mixer = inject(MixerService);
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

  /** The first of the tracks dragged: a deck plays one. */
  protected drop(event: DragEvent) {
    this.dropping.set(false);
    const data = event.dataTransfer?.getData(TRACKS_DRAG_TYPE);
    if (!data) return;
    event.preventDefault();
    const [path] = JSON.parse(data) as string[];
    if (path) this.library.load(path, this.deck());
  }
}

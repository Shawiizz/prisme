import { Component, booleanAttribute, computed, inject, input, output } from '@angular/core';
import { deckReadouts } from '../../core/deck-readouts';
import { deckVinyl } from '../../core/deck-vinyl';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { camelotHue, camelotMajor } from '../../shared/camelot';
import { Cover } from '../../shared/cover/cover';
import { deckLetter } from '../../shared/time';
import { DeckWaveform } from '../deck/deck-waveform/deck-waveform';

/** The record's two ways of driving a deck, as its light names them. */
const VINYL_LABEL = { relative: 'REL', absolute: 'ABS' } as const;

/**
 * A deck on a phone, for eyes that are a step away: its track, a wide waveform, the time
 * left in large figures, its tempo and its key. Touched, it opens what is set less often.
 * `compact` leaves the waveform and one line, when the thumbs need the screen.
 */
@Component({
  selector: 'app-phone-deck',
  template: `
    <button class="head" (click)="opened.emit()" [attr.aria-label]="'Deck settings' | t">
      <span class="letter" [class.playing]="status()?.playing">{{ letter() }}</span>
      @if (track(); as t) {
        @if (!compact()) {
          <app-cover [path]="t.path" />
        }
        <span class="names">
          <span class="title">{{ t.title }}</span>
          @if (!compact() && t.artist) {
            <span class="artist">{{ t.artist }}</span>
          }
        </span>
      } @else {
        <span class="names empty">{{
          loading() ? ('Loading…' | t) : ('No track loaded' | t)
        }}</span>
      }
      @if (compact()) {
        <span class="bpm pr-num">{{ bpm() ?? '' }}</span>
      }
      @if (key(); as k) {
        <span
          class="pr-key pr-key--fill"
          [class.pr-key--maj]="major(k.camelot)"
          [style.--h]="hue(k.camelot)"
          >{{ k.camelot }}</span
        >
      }
      @if (compact()) {
        <span class="time pr-num" [class]="ending() ?? ''">{{ remaining() }}</span>
      }
    </button>
    <app-deck-waveform [deck]="deck()" />
    @if (!compact()) {
      <button class="readouts" (click)="opened.emit()" [attr.aria-label]="'Deck settings' | t">
        <span class="time pr-num" [class]="ending() ?? ''">{{ remaining() }}</span>
        @if (vinyl(); as v) {
          <span class="vinyl" [class]="signal()" [title]="signalTitle()">{{ v }}</span>
        }
        <span class="bpm pr-num">{{ bpm() ?? '—' }}</span>
        <span class="pitch pr-num">{{ pitch() }}</span>
      </button>
    }
  `,
  styleUrl: './phone-deck.scss',
  host: { '[class.compact]': 'compact()' },
  imports: [Cover, DeckWaveform, TPipe],
})
export class PhoneDeck {
  readonly deck = input.required<number>();
  readonly compact = input(false, { transform: booleanAttribute });
  readonly opened = output<void>();

  private readonly engine = inject(EngineService);
  private readonly readouts = deckReadouts(this.deck);
  private readonly driven = deckVinyl(this.deck);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly loading = computed(() => this.engine.loading()[this.deck()]);
  protected readonly track = this.readouts.track;
  protected readonly status = this.readouts.status;
  protected readonly remaining = this.readouts.remaining;
  protected readonly ending = this.readouts.ending;
  protected readonly bpm = this.readouts.bpm;
  protected readonly pitch = this.readouts.pitch;
  protected readonly key = this.readouts.key;
  /** `REL` or `ABS` while a record drives the deck. */
  protected readonly vinyl = computed(() => {
    const mode = this.driven.mode();
    return mode === 'int' ? null : VINYL_LABEL[mode];
  });
  protected readonly signal = this.driven.signal;
  protected readonly signalTitle = this.driven.signalTitle;
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
}

import { Component, computed, inject, input } from '@angular/core';
import { deckReadouts } from '../../core/deck-readouts';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { camelotHue, camelotMajor } from '../../shared/camelot';
import { deckLetter } from '../../shared/time';
import { DeckWaveform } from '../deck/deck-waveform/deck-waveform';

/**
 * A deck on a phone: its letter (lit while playing), its track and its key, its waveform,
 * and the time left, the tempo and the pitch. On a wide screen, all but the waveform on
 * one line.
 */
@Component({
  selector: 'app-phone-deck',
  template: `
    <span class="pr-tile" [class.pr-tile--outline]="!status()?.playing">{{ letter() }}</span>
    @if (track(); as t) {
      <span class="title"
        >{{ t.title }}
        @if (t.artist) {
          <span>· {{ t.artist }}</span>
        }
      </span>
    } @else {
      <span class="title empty">{{ loading() ? ('Loading…' | t) : ('No track loaded' | t) }}</span>
    }
    @if (key(); as k) {
      <span
        class="pr-key pr-key--fill"
        [class.pr-key--maj]="major(k.camelot)"
        [style.--h]="hue(k.camelot)"
        >{{ k.camelot }}</span
      >
    }
    <app-deck-waveform [deck]="deck()" />
    <span
      class="remaining pr-num"
      [class.warn]="ending() === 'warn'"
      [class.alert]="ending() === 'alert'"
      >{{ remaining() }}</span
    >
    <span class="tempo pr-num"
      >{{ bpm() ?? '—' }} <span>{{ pitch() }}</span></span
    >
  `,
  styleUrl: './phone-deck.scss',
  imports: [DeckWaveform, TPipe],
})
export class PhoneDeck {
  readonly deck = input.required<number>();

  private readonly engine = inject(EngineService);
  private readonly readouts = deckReadouts(this.deck);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly loading = computed(() => this.engine.loading()[this.deck()]);
  protected readonly track = this.readouts.track;
  protected readonly status = this.readouts.status;
  protected readonly remaining = this.readouts.remaining;
  protected readonly ending = this.readouts.ending;
  protected readonly bpm = this.readouts.bpm;
  protected readonly pitch = this.readouts.pitch;
  protected readonly key = this.readouts.key;
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
}

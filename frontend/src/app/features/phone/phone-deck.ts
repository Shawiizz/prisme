import { Component, computed, inject, input, signal } from '@angular/core';
import { deckReadouts } from '../../core/deck-readouts';
import { DECK_MODES, deckVinyl } from '../../core/deck-vinyl';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { camelotHue, camelotMajor } from '../../shared/camelot';
import { deckLetter } from '../../shared/time';
import { DeckWaveform } from '../deck/deck-waveform/deck-waveform';
import { GridEditor } from '../deck/grid-editor/grid-editor';

/**
 * A deck on a phone: its letter (lit while playing), its track and its key, its waveform,
 * and the time left, who drives it (itself or its record), the tempo and the pitch. On a
 * wide screen, all but the waveform on one line.
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
    <div class="vinyl">
      <span
        class="pr-sig"
        [class.pr-sig--ok]="signal() === 'ok'"
        [class.pr-sig--weak]="signal() === 'bounce'"
        [class.pr-sig--none]="signal() === 'none'"
        [title]="signalTitle()"
      ></span>
      <div class="pr-seg" role="radiogroup" [attr.aria-label]="'Vinyl control' | t">
        @for (m of modes; track m.id) {
          <button role="radio" [attr.aria-checked]="mode() === m.id" (click)="setMode(m.id)">
            {{ m.label }}
          </button>
        }
      </div>
    </div>
    <span
      class="remaining pr-num"
      [class.warn]="ending() === 'warn'"
      [class.alert]="ending() === 'alert'"
      >{{ remaining() }}</span
    >
    <button
      class="tempo pr-num"
      [disabled]="!track()"
      (click)="editing.set(true)"
      [attr.aria-label]="'Beat grid, key and key shift' | t"
    >
      {{ bpm() ?? '—' }} <span>{{ pitch() }}</span>
    </button>
    @if (editing() && track()) {
      <app-grid-editor [deck]="deck()" (closed)="editing.set(false)" />
    }
  `,
  styleUrl: './phone-deck.scss',
  imports: [DeckWaveform, GridEditor, TPipe],
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
  /** The grid and key editor is open. */
  protected readonly editing = signal(false);
  private readonly vinyl = deckVinyl(this.deck);
  protected readonly modes = DECK_MODES;
  protected readonly mode = this.vinyl.mode;
  protected readonly signal = this.vinyl.signal;
  protected readonly signalTitle = this.vinyl.signalTitle;
  protected readonly setMode = this.vinyl.setMode;
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
}

import { Component, DestroyRef, computed, inject, input, output } from '@angular/core';
import { deckTempo } from '../../core/deck-tempo';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { deckLetter } from '../../shared/time';
import { PhoneTempo } from './phone-tempo';

/**
 * The tempo of a deck, floating over the table of a phone: its fader, − and + held to push
 * or hold it back a moment, 0 to bring it back, and its key lock.
 */
@Component({
  selector: 'app-phone-pitch',
  template: `
    <div class="head">
      <span class="pr-tile" [class.pr-tile--outline]="!tempo.status()?.playing">{{
        letter()
      }}</span>
      <b>Pitch</b>
      <span class="value pr-num">{{ tempo.pitch() }}</span>
      <button
        class="pr-btn pr-btn--sm pr-btn--icon pr-btn--ghost"
        (click)="closed.emit()"
        [attr.aria-label]="'Close' | t"
      >
        ×
      </button>
    </div>
    @if (tempo.toMatch(); as to) {
      <p class="target pr-num">{{ 'Pitch to match' | t }} → {{ to }}</p>
    }
    <app-phone-tempo [deck]="deck()" />
    <div class="keys">
      <button
        class="pr-btn"
        (pointerdown)="bend(-1, $event)"
        (pointerup)="unbend()"
        (lostpointercapture)="unbend()"
        [attr.aria-label]="'Pitch bend down' | t"
      >
        −
      </button>
      <button class="pr-btn" [disabled]="tempo.held()" (click)="engine.setPitch(deck(), 1)">
        0
      </button>
      <button
        class="pr-btn"
        (pointerdown)="bend(1, $event)"
        (pointerup)="unbend()"
        (lostpointercapture)="unbend()"
        [attr.aria-label]="'Pitch bend up' | t"
      >
        +
      </button>
    </div>
    <button
      class="pr-btn lock"
      [class.is-on]="tempo.settings().keyLock"
      (click)="engine.setKeyLock(deck(), !tempo.settings().keyLock)"
    >
      KEY LOCK
    </button>
  `,
  styleUrl: './phone-float.scss',
  imports: [PhoneTempo, TPipe],
})
export class PhonePitch {
  readonly deck = input.required<number>();
  readonly closed = output<void>();

  protected readonly engine = inject(EngineService);
  protected readonly tempo = deckTempo(this.deck);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  /** The deck pushed or held back: the one let go, even if the float changed decks since. */
  private bent: number | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.unbend());
  }

  protected bend(direction: -1 | 1, event: PointerEvent) {
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    this.unbend();
    this.bent = this.deck();
    this.engine.pitchBend(this.bent, direction);
  }

  protected unbend() {
    if (this.bent === null) return;
    this.engine.pitchBend(this.bent, 0);
    this.bent = null;
  }
}

import { Component, computed, inject, input, output } from '@angular/core';
import { deckTempo } from '../../core/deck-tempo';
import { EngineService } from '../../core/services/engine.service';
import { PreferencesService } from '../../core/services/preferences.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { Fader } from '../../shared/fader/fader';
import { deckLetter, formatPitch } from '../../shared/time';

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
      <span class="value pr-num">{{ pitch() }}</span>
      <button
        class="pr-btn pr-btn--sm pr-btn--icon pr-btn--ghost"
        (click)="closed.emit()"
        [attr.aria-label]="'Close' | t"
      >
        ×
      </button>
    </div>
    @if (target(); as to) {
      <p class="target pr-num">{{ 'Pitch to match' | t }} → {{ to }}</p>
    }
    <div class="fader">
      <span class="pr-num">−{{ rangePercent() }}</span>
      <app-fader
        touch
        centered
        [min]="-range()"
        [max]="range()"
        [value]="offset()"
        (valueChange)="engine.setPitch(deck(), 1 + $event)"
        [disabled]="tempo.held()"
        [label]="'Tempo' | t"
      />
      <span class="pr-num">+{{ rangePercent() }}</span>
    </div>
    <div class="keys">
      <button
        class="pr-btn"
        (pointerdown)="engine.pitchBend(deck(), -1)"
        (pointerup)="engine.pitchBend(deck(), 0)"
        (pointercancel)="engine.pitchBend(deck(), 0)"
        [attr.aria-label]="'Pitch bend' | t"
      >
        −
      </button>
      <button class="pr-btn" [disabled]="tempo.held()" (click)="engine.setPitch(deck(), 1)">
        0
      </button>
      <button
        class="pr-btn"
        (pointerdown)="engine.pitchBend(deck(), 1)"
        (pointerup)="engine.pitchBend(deck(), 0)"
        (pointercancel)="engine.pitchBend(deck(), 0)"
        [attr.aria-label]="'Pitch bend' | t"
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
  imports: [Fader, TPipe],
})
export class PhonePitch {
  readonly deck = input.required<number>();
  readonly closed = output<void>();

  protected readonly engine = inject(EngineService);
  private readonly preferences = inject(PreferencesService);
  protected readonly tempo = deckTempo(this.deck);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly range = computed(() => this.preferences.dj().pitchRange);
  protected readonly rangePercent = computed(() => Math.round(this.range() * 100));
  /** A fader the deck does not obey shows the rate it plays at. */
  protected readonly offset = computed(
    () => (this.tempo.held() ? this.tempo.rate() : this.tempo.settings().pitch) - 1,
  );
  protected readonly pitch = computed(() => formatPitch(1 + this.offset()));
  protected readonly target = computed(() => {
    const target = this.tempo.target();
    return target === null ? null : formatPitch(target);
  });
}

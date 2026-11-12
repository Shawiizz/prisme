import { Component, computed, inject, input } from '@angular/core';
import { deckTempo } from '../../../core/deck-tempo';
import { EngineService } from '../../../core/services/engine.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Fader } from '../../../shared/fader/fader';

/** A press of − or + moves the tempo this much; held, it goes on, a step this often (ms)
 * after the first wait. (The temporary bend is on the keyboard and the controllers.) */
const PITCH_STEP = 0.001;
const STEP_WAIT_MS = 400;
const STEP_EVERY_MS = 60;

/**
 * The tempo fader at the end of a deck's lane, where a turntable has it and the same way
 * up: faster is down. A record or sync holding the tempo, it shows the rate played and, on
 * a record, where to bring the turntable's own fader to match the other deck.
 */
@Component({
  selector: 'app-pitch-fader',
  template: `
    <span class="pr-label">Pitch</span>
    @if (!tempo.vinyl()) {
      <button
        class="pr-btn pr-btn--sm pr-btn--icon"
        [title]="'Tempo down 0.1 % (held, on down)' | t"
        [attr.aria-label]="'Tempo down 0.1 % (held, on down)' | t"
        [disabled]="tempo.held()"
        (pointerdown)="stepDown($event, -1)"
        (pointerup)="stepStop()"
        (pointercancel)="stepStop()"
        (click)="stepKey($event, -1)"
      >
        −
      </button>
    }
    <div class="travel">
      <!-- the fader's top is its highest value: the pitch goes in negated -->
      <app-fader
        orientation="vertical"
        centered
        [min]="-range()"
        [max]="range()"
        [value]="-offset()"
        (valueChange)="engine.setPitch(deck(), 1 - $event)"
        [disabled]="tempo.held()"
        [label]="'Tempo' | t"
      />
      @if (targetAt(); as at) {
        <i
          class="notch"
          [style.--at]="at.fraction"
          [title]="'Where to bring the turntable’s pitch' | t"
        ></i>
      }
    </div>
    @if (tempo.vinyl()) {
      <span class="range pr-num">±{{ rangePercent() }}</span>
    } @else {
      <button
        class="pr-btn pr-btn--sm pr-btn--icon"
        [title]="'Tempo up 0.1 % (held, on up)' | t"
        [attr.aria-label]="'Tempo up 0.1 % (held, on up)' | t"
        [disabled]="tempo.held()"
        (pointerdown)="stepDown($event, 1)"
        (pointerup)="stepStop()"
        (pointercancel)="stepStop()"
        (click)="stepKey($event, 1)"
      >
        +
      </button>
    }
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      align-items: center;
      gap: 4px;
      min-height: 0;
    }
    .pr-label,
    .range {
      line-height: 12px;
    }
    .pr-label {
      letter-spacing: 0.04em;
    }
    .range {
      font-size: 11px;
      color: var(--pr-text-3);
    }
    .pr-btn--icon {
      flex: none;
      width: 32px;
      font-size: 15px;
      font-weight: 500;
      color: var(--pr-text-2);
    }
    .travel {
      position: relative;
      flex: 1;
      min-height: 0;
      display: flex;
      app-fader {
        height: auto;
      }
    }
    // the rate to reach: a hollow mark on both sides of the track
    .notch {
      --cap: 14px;
      position: absolute;
      inset-inline: -4px;
      top: calc((100% - var(--cap)) * var(--at) + var(--cap) / 2 - 1px);
      height: 2px;
      pointer-events: none;
      background: linear-gradient(
        to right,
        var(--pr-light) 0 7px,
        transparent 7px calc(100% - 7px),
        var(--pr-light) calc(100% - 7px)
      );
    }
  `,
  imports: [Fader, TPipe],
})
export class PitchFader {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  private readonly preferences = inject(PreferencesService);
  protected readonly tempo = deckTempo(this.deck);
  protected readonly range = computed(() => this.preferences.dj().pitchRange);
  protected readonly rangePercent = computed(() => Math.round(this.range() * 100));
  /** A fader the deck does not obey shows the rate it plays at. */
  protected readonly offset = computed(
    () => (this.tempo.held() ? this.tempo.rate() : this.tempo.settings().pitch) - 1,
  );
  /** Where the rate to reach is along the travel, from the top; `null` without one or out
   * of the fader's range. */
  protected readonly targetAt = computed(() => {
    const target = this.tempo.target();
    if (target === null) return null;
    const fraction = 0.5 + (target - 1) / (2 * this.range());
    return fraction >= 0 && fraction <= 1 ? { fraction } : null;
  });

  private stepping: ReturnType<typeof setTimeout> | null = null;

  stepDown(event: PointerEvent, direction: -1 | 1) {
    if (event.button !== 0) return;
    this.stepStop();
    this.step(direction);
    const goOn = (wait: number) => {
      this.stepping = setTimeout(() => {
        this.step(direction);
        goOn(STEP_EVERY_MS);
      }, wait);
    };
    goOn(STEP_WAIT_MS);
  }

  stepStop() {
    if (this.stepping !== null) clearTimeout(this.stepping);
    this.stepping = null;
  }

  /** From the keyboard (a click with no pointer behind it): one step. */
  stepKey(event: MouseEvent, direction: -1 | 1) {
    if (event.detail === 0) this.step(direction);
  }

  private step(direction: -1 | 1) {
    const range = this.range();
    const pitch = this.tempo.settings().pitch + direction * PITCH_STEP;
    const kept = Math.min(Math.max(pitch, 1 - range), 1 + range);
    this.engine.setPitch(this.deck(), Math.round(kept * 1e4) / 1e4);
  }
}

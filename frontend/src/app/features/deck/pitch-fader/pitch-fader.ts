import { Component, DestroyRef, booleanAttribute, inject, input } from '@angular/core';
import { deckTempo } from '../../../core/deck-tempo';
import { EngineService } from '../../../core/services/engine.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Fader } from '../../../shared/fader/fader';
import { Press } from '../../../shared/press';

/** A press of − or + moves the tempo a step (0.1 %); held, it goes on, a step this often (ms)
 * after the first wait. (The temporary bend is on the keyboard and the controllers.) */
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
    <span class="pr-label">{{ 'Pitch' | t }}</span>
    @if (!tempo.vinyl()) {
      <button
        class="pr-btn pr-btn--sm pr-btn--icon"
        [title]="'Tempo down 0.1 % (held, on down)' | t"
        [attr.aria-label]="'Tempo down 0.1 % (held, on down)' | t"
        [disabled]="tempo.held()"
        (press)="stepDown(-1)"
        (release)="stepStop()"
      >
        −
      </button>
    }
    <div class="travel">
      <!-- the fader's top is its highest value: the position goes in upside down -->
      <app-fader
        orientation="vertical"
        centered
        [value]="1 - tempo.position()"
        [defaultValue]="0.5"
        (valueChange)="engine.moveTempo(deck(), 1 - $event)"
        [disabled]="tempo.held()"
        [label]="'Tempo' | t"
      />
      @if (tempo.targetAt() !== null) {
        <i
          class="notch"
          [style.--at]="tempo.targetAt()"
          [title]="'Where to bring the turntable’s pitch' | t"
        ></i>
      }
    </div>
    @if (tempo.vinyl()) {
      <span class="range pr-num">±{{ tempo.rangePercent() }}</span>
    } @else {
      <button
        class="pr-btn pr-btn--sm pr-btn--icon"
        [title]="'Tempo up 0.1 % (held, on up)' | t"
        [attr.aria-label]="'Tempo up 0.1 % (held, on up)' | t"
        [disabled]="tempo.held()"
        (press)="stepDown(1)"
        (release)="stepStop()"
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
    // four decks: the fader and its keys alone
    :host(.compact) {
      gap: 2px;
      .pr-label,
      .range {
        display: none;
      }
      .pr-btn--icon {
        height: 20px;
      }
    }
    // the rate to reach: two arrows, one each side of the track, pointing at it
    .notch {
      --cap: 14px;
      position: absolute;
      inset-inline: -7px;
      top: calc((100% - var(--cap)) * var(--at) + var(--cap) / 2 - 4px);
      height: 8px;
      pointer-events: none;
      &::before,
      &::after {
        content: '';
        position: absolute;
        top: 0;
        width: 6px;
        height: 100%;
        background: var(--pr-light);
      }
      &::before {
        left: 0;
        clip-path: polygon(0 0, 100% 50%, 0 100%);
      }
      &::after {
        right: 0;
        clip-path: polygon(100% 0, 0 50%, 100% 100%);
      }
    }
  `,
  host: { '[class.compact]': 'compact()' },
  imports: [Fader, TPipe, Press],
})
export class PitchFader {
  readonly deck = input.required<number>();
  /** Half the height, for four decks. */
  readonly compact = input(false, { transform: booleanAttribute });

  protected readonly engine = inject(EngineService);
  protected readonly tempo = deckTempo(this.deck);

  private stepping: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.stepStop());
  }

  stepDown(direction: -1 | 1) {
    this.stepStop();
    this.step(direction);
    const goOn = (wait: number) => {
      this.stepping = setTimeout(() => {
        // the key disabled under the finger (sync took the tempo): its release may not come
        if (this.tempo.held()) return this.stepStop();
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

  private step(direction: -1 | 1) {
    this.engine.press(this.deck(), direction < 0 ? 'tempo.down' : 'tempo.up');
  }
}

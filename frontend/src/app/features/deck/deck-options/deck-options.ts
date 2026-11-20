import { Component, DestroyRef, computed, inject, input, output } from '@angular/core';
import { PITCH_RANGES } from '../../../core/models/preferences';
import { PHONE } from '../../../core/platform';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { PerformanceService } from '../../../core/services/performance.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { Press } from '../../../shared/press';

/**
 * What a deck does now and then rather than all the time, a row each: its motor (a brake, a
 * soft start, a spinback while held), jumps to the start and the end, the track played
 * backwards, the channel muted, the track repeated, the range of its tempo fader, on the
 * software mixer the channel fader starting and stopping it; and the track taken off.
 */
@Component({
  selector: 'app-deck-options',
  template: `
    <div class="row">
      <span>{{ 'Motor' | t }}</span>
      <div class="keys">
        <button
          class="pr-btn pr-btn--sm"
          [class.pr-reach]="PHONE"
          [disabled]="!movable()"
          [title]="'Brake: the deck slows to a stop; stopped, it starts softly' | t"
          (click)="performance.brake(deck())"
        >
          BRAKE
        </button>
        <button
          class="pr-btn pr-btn--sm"
          [class.pr-reach]="PHONE"
          [disabled]="!movable()"
          [title]="'Spinback: the record spun back while held, then it plays on' | t"
          (press)="spinBack(true)"
          (release)="spinBack(false)"
        >
          SPINBACK
        </button>
      </div>
    </div>
    <div class="row">
      <span>{{ 'Jump to' | t }}</span>
      <div class="keys">
        <button
          class="pr-btn pr-btn--sm"
          [class.pr-reach]="PHONE"
          [disabled]="!placeable()"
          [title]="'To the start of the track, playing or not as it was' | t"
          (click)="engine.press(deck(), 'start.keep')"
        >
          {{ 'START' | t }}
        </button>
        <button
          class="pr-btn pr-btn--sm"
          [class.pr-reach]="PHONE"
          [disabled]="!placeable()"
          [title]="'To the very end of the track' | t"
          (click)="engine.press(deck(), 'end')"
        >
          {{ 'END' | t }}
        </button>
      </div>
    </div>
    <div class="row">
      <span>{{ 'Reverse' | t }}</span>
      <button
        class="pr-switch"
        role="switch"
        [attr.aria-checked]="reversed()"
        [attr.aria-label]="'Reverse' | t"
        [title]="'The track plays backwards; with slip, it comes back where it would be' | t"
        (click)="performance.setReverse(deck(), !reversed())"
      >
        <span></span>
      </button>
    </div>
    <div class="row">
      <span>{{ 'Mute the channel' | t }}</span>
      <button
        class="pr-switch"
        role="switch"
        [attr.aria-checked]="settings().mute"
        [attr.aria-label]="'Mute the channel' | t"
        [title]="'Silent in the master, still heard in the headphones' | t"
        (click)="engine.setMute(deck(), !settings().mute)"
      >
        <span></span>
      </button>
    </div>
    <div class="row">
      <span>{{ 'Repeat the track' | t }}</span>
      <button
        class="pr-switch"
        role="switch"
        [attr.aria-checked]="settings().repeat"
        [attr.aria-label]="'Repeat the track' | t"
        [title]="'At the end, the track plays on from its start' | t"
        (click)="engine.setRepeat(deck(), !settings().repeat)"
      >
        <span></span>
      </button>
    </div>
    <div class="row">
      <span>{{ 'Tempo fader range' | t }}</span>
      <div class="pr-seg" role="radiogroup" [attr.aria-label]="'Tempo fader range' | t">
        @for (r of ranges; track r) {
          <button
            role="radio"
            [class.pr-reach]="PHONE"
            [attr.aria-checked]="range() === r"
            (click)="engine.setTempoRange(deck(), r)"
          >
            ±{{ r * 100 }}
          </button>
        }
      </div>
    </div>
    @if (mixer.internal()) {
      <div class="row">
        <span>{{ 'Fader start' | t }}</span>
        <button
          class="pr-switch"
          role="switch"
          [attr.aria-checked]="settings().faderStart"
          [attr.aria-label]="'Fader start' | t"
          [title]="'The channel fader raised plays the deck; closed, it stops it on its cue' | t"
          (click)="engine.setFaderStart(deck(), !settings().faderStart)"
        >
          <span></span>
        </button>
      </div>
    }
    <div class="row">
      <span>{{ 'Eject the track' | t }}</span>
      <button
        class="pr-btn pr-btn--sm"
        [class.pr-reach]="PHONE"
        [disabled]="!loaded()"
        [title]="'The deck left empty; playing, it asks first' | t"
        (click)="eject()"
      >
        {{ 'EJECT' | t }}
      </button>
    </div>
  `,
  styles: `
    :host {
      display: grid;
    }
    .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pr-space-3);
      min-height: 44px;
      font-size: var(--pr-size-small);
      color: var(--pr-text-2);
      & + & {
        border-top: 1px solid var(--pr-line);
      }
    }
    .row .pr-btn {
      padding: 0 7px;
      font: 600 10.5px / 1 var(--pr-font);
      letter-spacing: 0.04em;
    }
    .keys {
      display: flex;
      align-items: center;
      gap: var(--pr-space-1);
    }
    // a finger's room between the rows
    :host-context(.phone) .row {
      min-height: 60px;
    }
  `,
  imports: [Press, TPipe],
})
export class DeckOptions {
  readonly deck = input.required<number>();
  /** The track was taken off the deck. */
  readonly ejected = output<void>();

  protected readonly engine = inject(EngineService);
  protected readonly performance = inject(PerformanceService);
  protected readonly mixer = inject(MixerService);
  protected readonly PHONE = PHONE;
  protected readonly ranges = PITCH_RANGES;
  protected readonly range = computed(() => this.engine.tempoRange(this.deck()));
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly reversed = computed(
    () => this.engine.status()?.decks[this.deck()]?.reverse ?? false,
  );

  protected readonly loaded = computed(() => !!this.engine.status()?.decks[this.deck()]?.loaded);

  /** The motor moves a track that plays by itself: a record drives its own. */
  protected readonly movable = computed(
    () => this.loaded() && !this.settings().vinylEnabled && !this.settings().passthrough,
  );

  /** A jump moves a track a record in absolute mode does not hold. */
  protected readonly placeable = computed(
    () =>
      this.loaded() && !(this.settings().vinylEnabled && this.settings().vinylMode === 'absolute'),
  );

  /** The deck spun back: the one let go, even if the menu changed decks meanwhile. */
  private spun: number | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.spinBack(false));
  }

  protected async eject() {
    if (await this.engine.press(this.deck(), 'eject')) this.ejected.emit();
  }

  protected spinBack(held: boolean) {
    if (held) {
      this.spun = this.deck();
      this.performance.spinBack(this.spun, true);
    } else if (this.spun !== null) {
      this.performance.spinBack(this.spun, false);
      this.spun = null;
    }
  }
}

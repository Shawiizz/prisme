import { Component, booleanAttribute, computed, inject, input } from '@angular/core';
import { EngineService } from '../../core/services/engine.service';
import { MixerService } from '../../core/services/mixer.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { AssignTarget, deckAction } from '../assign/assign';

/** The headphones of a deck's channel: lit while the deck is heard in them; a double click
 * leaves it alone in them. Sized by its host; `touch` reaches a finger beyond it. */
@Component({
  selector: 'app-cue-button',
  template: `
    <button
      class="pr-btn pr-btn--icon"
      [class.pr-reach]="touch()"
      [class.is-on]="cued()"
      [attr.aria-pressed]="cued()"
      [appAssign]="deckAction(deck(), 'headphones')"
      (click)="mixer.setDeckCue(deck(), !cued())"
      (dblclick)="engine.soloCue(deck())"
      [title]="'Hear this deck in the headphones · double-click: this deck alone' | t"
      [attr.aria-label]="'Hear this deck in the headphones' | t"
    >
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M2.5 10V8a5.5 5.5 0 0 1 11 0v2"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
        />
        <rect x="1.5" y="9" width="3.5" height="5" rx="1" fill="currentColor" />
        <rect x="11" y="9" width="3.5" height="5" rx="1" fill="currentColor" />
      </svg>
    </button>
  `,
  styles: `
    :host {
      display: grid;
      width: var(--pr-h);
      height: var(--pr-h);
    }
    button {
      width: 100%;
      height: 100%;
      padding: 0;
    }
    // half the key, from 14 to 16 px
    svg {
      width: clamp(14px, 50%, 16px);
      height: auto;
    }
  `,
  imports: [TPipe, AssignTarget],
})
export class CueButton {
  readonly deck = input.required<number>();
  protected readonly deckAction = deckAction;
  readonly touch = input(false, { transform: booleanAttribute });

  protected readonly engine = inject(EngineService);
  protected readonly mixer = inject(MixerService);
  protected readonly cued = computed(() => this.engine.decks()[this.deck()].cue);
}

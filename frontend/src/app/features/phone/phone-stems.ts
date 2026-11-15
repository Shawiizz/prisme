import { Component, computed, inject, input, output } from '@angular/core';
import { deckStems } from '../../core/deck-stems';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { Knob } from '../../shared/knob/knob';
import { readouts } from '../../shared/mixer-scales';
import { deckLetter } from '../../shared/time';

/**
 * The stems of a deck, floating over the table of a phone: a knob for the level of each
 * part, its name muting it at that level. A track not separated yet offers its separation;
 * one being separated plays its stems where they are there already, the rest as it is, and
 * says how far it is.
 */
@Component({
  selector: 'app-phone-stems',
  template: `
    <div class="head">
      <span class="pr-tile" [class.pr-tile--outline]="!playing()">{{ letter() }}</span>
      <b>Stems</b>
      <button
        class="pr-btn pr-btn--sm pr-btn--icon pr-btn--ghost"
        (click)="closed.emit()"
        [attr.aria-label]="'Close' | t"
      >
        ×
      </button>
    </div>
    @if (stems.separation() === 'none') {
      <button
        class="pr-btn separate"
        [disabled]="!stems.canSeparate()"
        (click)="stems.separate()"
        [title]="'Separate this track into stems' | t"
      >
        <span>{{ 'Separate' | t }}</span>
        @if (stems.separating(); as done) {
          <span class="pr-num">{{ percent(done) }}</span>
          <i [style.--done]="done"></i>
        }
      </button>
    } @else {
      <div class="parts">
        @for (k of stems.knobs(); track k.part) {
          <app-knob
            [class.waiting]="k.waiting"
            [dimmed]="!k.on"
            [disabled]="!k.ready"
            [value]="k.level"
            [defaultValue]="1"
            [label]="k.label"
            [labelTitle]="k.title"
            [active]="!k.on"
            [format]="percent"
            (valueChange)="engine.setStemLevel(deck(), k.part, $event)"
            (labelClick)="engine.toggleStem(deck(), k.part)"
          />
        }
      </div>
      @if (stems.separating(); as done) {
        <p class="coming">
          <span>{{ 'Separating…' | t }}</span>
          <span class="pr-num">{{ percent(done) }}</span>
          <i [style.--done]="done"></i>
        </p>
      }
    }
  `,
  styleUrls: ['./phone-float.scss', './phone-stems.scss'],
  imports: [Knob, TPipe],
})
export class PhoneStems {
  readonly deck = input.required<number>();
  readonly closed = output<void>();

  protected readonly engine = inject(EngineService);
  protected readonly stems = deckStems(this.deck);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly playing = computed(
    () => this.engine.status()?.decks[this.deck()]?.playing ?? false,
  );
  protected readonly percent = readouts.percent;
}

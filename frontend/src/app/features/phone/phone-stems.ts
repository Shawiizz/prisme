import { Component, inject, input, output } from '@angular/core';
import { deckStems } from '../../core/deck-stems';
import { EngineService } from '../../core/services/engine.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { Knob } from '../../shared/knob/knob';
import { percent } from '../../shared/readout';
import { PhoneFloatHead } from './phone-float-head';

/**
 * The stems of a deck, floating over the table of a phone: a knob for the level of each
 * part, its name muting it at that level. A track not separated yet offers its separation;
 * one being separated plays its stems where they are there already, the rest as it is, and
 * says how far it is; one whose separation stopped short offers it again.
 */
@Component({
  selector: 'app-phone-stems',
  template: `
    <app-phone-float-head [deck]="deck()" name="Stems" (closed)="closed.emit()" />
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
            [readout]="percent(k.level)"
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
      } @else if (stems.resumable()) {
        <button
          class="pr-btn separate"
          (click)="stems.separate()"
          [title]="'Separate this track again: its separation stopped short' | t"
        >
          {{ 'Separate' | t }}
        </button>
      }
    }
  `,
  styleUrls: ['./phone-float.scss', './phone-stems.scss'],
  imports: [Knob, PhoneFloatHead, TPipe],
})
export class PhoneStems {
  readonly deck = input.required<number>();
  readonly closed = output<void>();

  protected readonly engine = inject(EngineService);
  protected readonly stems = deckStems(this.deck);
  protected readonly percent = percent;
}

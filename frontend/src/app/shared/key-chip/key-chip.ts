import { Component, booleanAttribute, computed, input } from '@angular/core';
import { camelotHue, camelotMajor } from '../camelot';

/** A key in its colour of the wheel (`.pr-key`): `fill` for the key a deck plays. */
@Component({
  selector: 'app-key-chip',
  template: `{{ camelot() }}`,
  host: {
    class: 'pr-key',
    '[class.pr-key--fill]': 'fill()',
    '[class.pr-key--maj]': 'major()',
    '[style.--h]': 'hue()',
  },
})
export class KeyChip {
  /** The key in Camelot notation: `8A`. */
  readonly camelot = input.required<string>();
  readonly fill = input(false, { transform: booleanAttribute });

  protected readonly hue = computed(() => camelotHue(this.camelot()));
  protected readonly major = computed(() => camelotMajor(this.camelot()));
}

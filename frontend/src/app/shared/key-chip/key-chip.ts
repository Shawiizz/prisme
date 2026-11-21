import { Component, booleanAttribute, computed, input } from '@angular/core';
import { keyHue } from '../musical-key';
import type { MusicalKey } from '../../core/models/backend';

/** A key in its colour of the wheel (`.pr-key`): `fill` for the key a deck plays. */
@Component({
  selector: 'app-key-chip',
  template: `{{ key().camelot }}`,
  host: {
    class: 'pr-key',
    '[class.pr-key--fill]': 'fill()',
    '[class.pr-key--maj]': '!key().minor',
    '[style.--h]': 'hue()',
  },
})
export class KeyChip {
  readonly key = input.required<MusicalKey>();
  readonly fill = input(false, { transform: booleanAttribute });

  protected readonly hue = computed(() => keyHue(this.key()));
}

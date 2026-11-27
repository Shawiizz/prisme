import { Directive, computed, inject, input } from '@angular/core';
import { AssignService } from '../../core/assign/assign.service';

/** The action `action` of deck `deck` (0 for A): `deck1.play`. */
export function deckAction(deck: number, action: string): string {
  return `deck${deck + 1}.${action}`;
}

/**
 * A button, a knob or a fader that does the action `appAssign`: while assigning, it is outlined
 * if it can take what the mode gives, and pressed, it waits for its key or its control.
 */
@Directive({
  selector: '[appAssign]',
  host: {
    '[attr.data-assign]': 'control()',
    '[class.pr-assignable]': 'open()',
    '[class.pr-assign-chosen]': 'open() && assign.chosen()?.control === control()',
    '[class.pr-assign-pointed]': 'open() && assign.pointed() === control()',
  },
})
export class AssignTarget {
  readonly control = input<string | null>(null, { alias: 'appAssign' });
  protected readonly assign = inject(AssignService);
  protected readonly open = computed(() => {
    const control = this.control();
    return this.assign.on() && control !== null && this.assign.allows(control);
  });
}

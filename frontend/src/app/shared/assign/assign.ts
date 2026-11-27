import { Directive, booleanAttribute, computed, inject, input } from '@angular/core';
import { AssignService } from '../../core/assign/assign.service';

/** The action `action` of deck `deck` (0 for A): `deck1.play`. */
export function deckAction(deck: number, action: string): string {
  return `deck${deck + 1}.${action}`;
}

/**
 * A button, a knob or a fader that does the action `appAssign` pressed, and `appAssignControl`
 * moved, if it differs: a key only presses, so a knob's key switches what its turn sets on a
 * controller. While assigning, it is outlined if it can take what the mode gives, and pressed,
 * it waits for its key or its control.
 */
@Directive({
  selector: '[appAssign],[appAssignControl]',
  host: {
    '[attr.data-assign]': 'control()',
    '[attr.data-assign-opens]': 'opens() || null',
    '[class.pr-assignable]': 'open()',
    '[class.pr-assign-chosen]': 'open() && assign.chosen()?.control === control()',
    '[class.pr-assign-pointed]': 'open() && assign.pointed() === control()',
  },
})
export class AssignTarget {
  /** What a press does: a key's action, and a controller's unless `moved` says otherwise. */
  readonly pressed = input<string | null>(null, { alias: 'appAssign' });
  /** What a controller's control does when it is turned or slid: a knob's, a fader's. */
  readonly moved = input<string | null>(null, { alias: 'appAssignControl' });
  /** A tab, or a page: picked, it shows what it opens too, for its own keys to be given next. */
  readonly opens = input(false, { alias: 'appAssignOpens', transform: booleanAttribute });
  protected readonly assign = inject(AssignService);
  /** The action the mode gives a key or a control now. */
  protected readonly control = computed(() =>
    this.assign.way() === 'controller' ? (this.moved() ?? this.pressed()) : this.pressed(),
  );
  protected readonly open = computed(() => {
    const control = this.control();
    return this.assign.on() && control !== null && this.assign.allows(control);
  });
}

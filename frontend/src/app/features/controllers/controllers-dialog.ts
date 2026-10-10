import { Component, OnDestroy, OnInit, computed, inject, output } from '@angular/core';
import { ControlService } from '../../core/control/control.service';
import { ControllerService } from '../../core/control/controller.service';
import { Control } from '../../core/control/controls';
import { describeControl } from '../../core/models/controllers';

interface Row {
  control: Control;
  /** Where the control is mapped, one line per controller. */
  bindings: string[];
}

/** MIDI controllers in use, and the mapping of the app's controls (learnt by moving them). */
@Component({
  selector: 'app-controllers-dialog',
  templateUrl: './controllers-dialog.html',
  styleUrl: './controllers-dialog.scss',
  host: { '(keydown.escape)': 'service.cancelLearning()' },
})
export class ControllersDialog implements OnInit, OnDestroy {
  protected readonly service = inject(ControllerService);
  private readonly controls = inject(ControlService);
  readonly closed = output();

  /** A controller is open: controls can be learnt. */
  protected readonly canLearn = computed(() =>
    this.service.controllers().some((c) => c.enabled && c.plugged),
  );

  protected readonly groups = computed(() => {
    const groups = new Map<string, Row[]>();
    for (const control of this.controls.controls) {
      const bindings = this.service
        .mappings()
        .filter((m) => m.target === control.id)
        .map((m) => `${m.device} · ${describeControl(m.control)}`);
      groups.set(control.group, [...(groups.get(control.group) ?? []), { control, bindings }]);
    }
    return [...groups.entries()].map(([name, rows]) => ({ name, rows }));
  });

  ngOnInit() {
    this.service.refresh();
  }

  ngOnDestroy() {
    this.service.cancelLearning();
  }

  toggleLearning(id: string) {
    if (this.service.learning() === id) this.service.cancelLearning();
    else this.service.learn(id);
  }
}

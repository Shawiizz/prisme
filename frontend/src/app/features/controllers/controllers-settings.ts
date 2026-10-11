import { Component, OnDestroy, OnInit, computed, inject } from '@angular/core';
import { ControlService } from '../../core/control/control.service';
import { ControllerService } from '../../core/control/controller.service';
import { Control } from '../../core/control/controls';
import { describeControl } from '../../core/models/controllers';
import { TPipe } from '../../core/i18n/t.pipe';

interface Row {
  control: Control;
  /** Where the control is mapped, one line per controller. */
  bindings: string[];
}

/** Settings page: MIDI controllers in use, and the mapping of the app's controls (learnt by moving them). */
@Component({
  selector: 'app-controllers-settings',
  templateUrl: './controllers-settings.html',
  styleUrl: './controllers-settings.scss',
  imports: [TPipe],
  host: { '(keydown.escape)': 'service.cancelLearning()' },
})
export class ControllersSettings implements OnInit, OnDestroy {
  protected readonly service = inject(ControllerService);
  private readonly controls = inject(ControlService);

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

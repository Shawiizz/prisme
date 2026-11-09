import { Component, OnDestroy, OnInit, computed, inject } from '@angular/core';
import { ControlService } from '../../core/control/control.service';
import { ControllerService } from '../../core/control/controller.service';
import { Control } from '../../core/control/controls';
import { ControllerMapping, StepEncoding, describeMapping } from '../../core/models/controllers';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';

interface Binding {
  mapping: ControllerMapping;
  text: string;
}

interface Row {
  control: Control;
  /** Where the control is mapped, one line per controller. */
  bindings: Binding[];
}

/** How an encoder may send its steps, as offered when the guess was wrong. */
const STEP_ENCODINGS: { value: StepEncoding; label: string }[] = [
  { value: 'relative', label: t('Steps 1 and 127') },
  { value: 'relativeOffset', label: t('Steps around 64') },
  { value: 'relativeSigned', label: t('Steps 1 and 65') },
];

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

  protected readonly stepEncodings = STEP_ENCODINGS;

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
        .map((m) => ({ mapping: m, text: `${m.device} · ${describeMapping(m)}` }));
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

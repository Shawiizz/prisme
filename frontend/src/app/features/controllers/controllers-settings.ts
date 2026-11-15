import { Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { ControlService } from '../../core/control/control.service';
import { ControllerService } from '../../core/control/controller.service';
import { Control } from '../../core/control/controls';
import { ControllerMapping, StepEncoding, describeMapping } from '../../core/models/controllers';
import { Text, t } from '../../core/i18n/i18n';
import { errorText } from '../../core/errors';
import { PHONE } from '../../core/platform';
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
const STEP_ENCODINGS: { value: StepEncoding; label: Text }[] = [
  {
    value: 'relative',
    label: () => t('Steps 1 and 127'),
  },
  {
    value: 'relativeOffset',
    label: () => t('Steps around 64'),
  },
  {
    value: 'relativeSigned',
    label: () => t('Steps 1 and 65'),
  },
];

/** A look for the profiles published since. */
type Look =
  { state: 'looking' } | { state: 'done'; came: number } | { state: 'failed'; why: string };

/** What came of an export: where the file went, or why it did not. */
interface Outcome {
  done: boolean;
  text: string;
}

/** The least time "Looking…" shows (ms). */
const LOOKING_MS = 600;

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
  protected readonly refreshing = signal(false);
  private readonly controls = inject(ControlService);
  protected readonly PHONE = PHONE;
  /** How the last look for published profiles went. */
  protected readonly look = signal<Look | null>(null);
  /** Where each controller's profile went, or why it did not, by controller. */
  protected readonly exported = signal<Record<string, Outcome>>({});

  /** What the last look for published profiles found. */
  protected readonly looked = computed(() => {
    const look = this.look();
    switch (look?.state) {
      case 'done': {
        const n = look.came;
        const text =
          n === 0
            ? t('The profiles are up to date.')
            : n === 1
              ? t('1 profile updated.')
              : t('{n} profiles updated.', { n });
        return { failed: false, text };
      }
      case 'failed':
        return { failed: true, text: look.why };
      default:
        return null;
    }
  });

  /** How many models come from where. */
  protected readonly counts = computed(() => {
    const profiles = this.service.profiles()?.profiles ?? [];
    const count = (source: string) => profiles.filter((p) => p.source === source).length;
    return { all: profiles.length, updated: count('updated'), own: count('own') };
  });

  protected readonly stepEncodings = STEP_ENCODINGS;

  /** A controller is open: controls can be learnt. */
  protected readonly canLearn = computed(() =>
    this.service.controllers().some((c) => c.enabled && c.plugged),
  );

  protected readonly groups = computed(() => {
    const groups = new Map<string, Row[]>();
    // a light only shows: nothing is learnt for it
    for (const control of this.controls.controls.filter((c) => c.kind !== 'light')) {
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
    this.service.loadProfiles();
  }

  protected async updateProfiles() {
    this.look.set({ state: 'looking' });
    try {
      this.look.set({ state: 'done', came: await this.service.updateProfiles() });
    } catch (e) {
      this.look.set({ state: 'failed', why: errorText(e) });
    }
  }

  protected async exportProfile(device: string) {
    let outcome: Outcome;
    try {
      outcome = { done: true, text: await this.service.exportProfile(device) };
    } catch (e) {
      outcome = { done: false, text: errorText(e) };
    }
    this.exported.update((all) => ({ ...all, [device]: outcome }));
  }

  /** Looked for long enough to be seen looking, even when nothing new is found. */
  protected async refresh() {
    this.refreshing.set(true);
    await Promise.all([this.service.refresh(), new Promise((r) => setTimeout(r, LOOKING_MS))]);
    this.refreshing.set(false);
  }

  ngOnDestroy() {
    this.service.cancelLearning();
  }

  toggleLearning(id: string) {
    if (this.service.learning() === id) this.service.cancelLearning();
    else this.service.learn(id);
  }
}

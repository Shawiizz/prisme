import { Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { ControlService, named } from '../../core/control/control.service';
import { ControllerService } from '../../core/control/controller.service';
import { Control } from '../../core/control/controls';
import { StepEncoding, describeMapping, shortName } from '../../core/models/controllers';
import { Text, t } from '../../core/i18n/i18n';
import { errorText } from '../../core/errors';
import { PHONE } from '../../core/platform';
import { TPipe } from '../../core/i18n/t.pipe';
import { matches, normalize } from '../settings/settings-catalog';
import type { Controller, ControllerMapping } from '../../core/models/backend';

/** Where a control is mapped on one controller. */
interface Binding {
  device: string;
  text: string;
  /** Its encoder, whose steps can be read another way. */
  encoder?: ControllerMapping;
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
    value: 'relativeOffsetNoZero',
    label: () => t('Steps 64 and 65'),
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

/** Whether `control` has every word looked for (normalized) in its name, its group or its id. */
export function found(words: string[], control: Control): boolean {
  return matches(words, `${control.label} ${control.group} ${control.id}`);
}

/** The least time "Looking…" shows (ms). */
const LOOKING_MS = 600;

/** Settings page: controllers in use, MIDI and HID, and the mapping of the app's controls (learnt
 * by moving them). */
@Component({
  selector: 'app-controllers-settings',
  templateUrl: './controllers-settings.html',
  styleUrl: './controllers-settings.scss',
  imports: [TPipe, NgTemplateOutlet],
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

  /** The DJ's own profiles that replace Prisme's of the same file name. */
  protected readonly replacing = computed(() => this.service.profiles().replacing);

  /** How many models come from where, their variants aside. */
  protected readonly counts = computed(() => this.service.profiles().counts);

  protected readonly stepEncodings = STEP_ENCODINGS;

  /** The controllers listed: the MIDI ports, and the HID devices of a model Prisme knows or in
   * use; the other HID devices (most are no controllers) folded apart. */
  protected readonly listed = computed(() => this.service.controllers().filter((c) => c.listed));
  protected readonly others = computed(() => this.service.controllers().filter((c) => !c.listed));
  protected readonly othersOpen = signal(false);

  /** A controller is open: controls can be learnt. */
  protected readonly canLearn = this.service.canLearn;

  /** Words looked for among every action, to map one. */
  protected readonly query = signal('');
  private readonly words = computed(() => normalize(this.query()).split(/\s+/).filter(Boolean));
  protected readonly searching = computed(() => this.words().length > 0);

  /** The controls by group, those of the decks shown only, then those mapped the list leaves
   * out (a loop of so many beats); a search looks among every action, those of the decks not
   * shown too. A light only shows: nothing is learnt for it. */
  protected readonly groups = computed(() => {
    const mapped = new Map(this.service.mappings().map((m) => [m.target.id, m]));
    const words = this.words();
    const searching = this.searching();
    const listed = searching ? this.controls.controls() : this.controls.inUse();
    const ids = new Set(listed.map((c) => c.id));
    const others = [...mapped.values()]
      .filter((m) => (searching || m.target.inUse) && !ids.has(m.target.id))
      .map((m) => named(m.target));
    const shown = [...listed, ...others].filter(
      (c) => c.kind !== 'light' && (!searching || found(words, c)),
    );
    const groups = new Map<string, Row[]>();
    for (const control of shown) {
      const bindings = (mapped.get(control.id)?.devices ?? []).map(({ device, mappings }) => ({
        device,
        text: `${shortName(device)} · ${mappings.map(describeMapping).join(' / ')}`,
        encoder: mappings.find((m) => m.encoding && m.encoding !== 'absolute'),
      }));
      const rows = groups.get(control.group) ?? [];
      rows.push({ control, bindings });
      groups.set(control.group, rows);
    }
    return [...groups.entries()].map(([name, rows]) => ({
      name,
      rows,
      mapped: rows.filter((row) => row.bindings.length > 0).length,
    }));
  });
  /** The groups unfolded: each folded at first, its actions a long list. */
  private readonly opened = signal<ReadonlySet<string>>(new Set());

  ngOnInit() {
    void this.service.look();
  }

  protected async updateProfiles() {
    this.look.set({ state: 'looking' });
    try {
      this.look.set({ state: 'done', came: await this.service.updateProfiles() });
    } catch (e) {
      this.look.set({ state: 'failed', why: errorText(e) });
    }
  }

  protected async exportProfile(controller: Controller) {
    let outcome: Outcome;
    try {
      const path = await this.service.exportProfile(controller);
      if (path === null) return;
      outcome = { done: true, text: path };
    } catch (e) {
      outcome = { done: false, text: errorText(e) };
    }
    this.exported.update((all) => ({ ...all, [controller.name]: outcome }));
  }

  /** Looked for long enough to be seen looking, even when nothing new is found. */
  protected async refresh() {
    this.refreshing.set(true);
    await Promise.all([this.service.look(), new Promise((r) => setTimeout(r, LOOKING_MS))]);
    this.refreshing.set(false);
  }

  ngOnDestroy() {
    this.service.cancelLearning();
  }

  /** Escape empties the search first; then it gives up learning, or closes the settings. */
  protected clearSearch(event: Event) {
    if (!this.query()) return;
    event.stopPropagation();
    this.query.set('');
  }

  /** Every group found by a search shows unfolded. */
  isOpen(group: string): boolean {
    return this.searching() || this.opened().has(group);
  }

  toggle(group: string) {
    this.opened.update((opened) => {
      const next = new Set(opened);
      if (!next.delete(group)) next.add(group);
      return next;
    });
  }

  toggleLearning(id: string) {
    if (this.service.learning() === id) this.service.cancelLearning();
    else this.service.learn(id);
  }
}

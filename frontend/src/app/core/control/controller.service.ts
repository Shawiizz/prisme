import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { StepEncoding } from '../models/controllers';
import { EngineService } from '../services/engine.service';
import { ControlService } from './control.service';
import { attempt } from '../errors';
import { t } from '../i18n/i18n';
import { Notice, NoticeService } from '../../shared/notice/notice';
import { NavigationService } from '../services/navigation.service';
import type { Controller, ControllerMapping, Opened, PluggedIn, UiAction } from '../models/backend';

/**
 * Controllers, MIDI and HID, as the backend shows them (`controls.controllers`): it performs
 * what their controls drive and lights them; the actions the app performs (its windows, the
 * focus of its search) it is told of (`ui-action`). A control is mapped by moving it (learn).
 */
@Injectable({ providedIn: 'root' })
export class ControllerService {
  private readonly controls = inject(ControlService);
  private readonly engine = inject(EngineService);
  private readonly notices = inject(NoticeService);
  private readonly navigation = inject(NavigationService);

  private readonly told = computed(() => this.engine.told().controllers);
  readonly controllers = computed(() => this.told().controllers);
  /** A controller is open: controls can be learnt. */
  readonly canLearn = computed(() => this.told().canLearn);
  readonly profiles = computed(() => this.told().profiles);
  /** What drives what, by action and by controller. */
  readonly mappings = computed(() => this.told().mapped);
  readonly clockOutput = computed(() => this.told().clockOutput);
  /** The control waiting for a move on the controller. */
  readonly learning = computed(() => this.told().learning);
  /** MIDI outputs, for the clock (`look`). */
  readonly outputs = signal<string[]>([]);

  async start() {
    await listen<UiAction>('ui-action', ({ payload }) =>
      this.controls.act(payload.target, payload.value),
    );
    await listen<unknown>('action-failed', ({ payload }) => void this.engine.failed(payload));
    // plugged back in, or let go by the program that held it: it plays, its lights off
    await listen<Opened>('controller-opened', ({ payload: { model } }) =>
      this.showConnected(model),
    );
    await listen<PluggedIn>('controller-plugged', ({ payload }) => this.pluggedIn(payload));
    const waiting = await invoke<string[]>('waiting_controllers');
    if (waiting.length) this.showWaiting(waiting.join(', '));
  }

  /** A controller plugged in: one Prisme knows plays at once, with its lights; any other is
   * offered, until the DJ chooses not to use it. */
  private pluggedIn({ kind, device, model }: PluggedIn) {
    if (kind === 'waiting') {
      this.showWaiting(model);
      return;
    }
    if (kind === 'opened') {
      this.showConnected(model, [
        { label: t('Do not use it'), run: () => this.setEnabled(device, false) },
      ]);
      return;
    }
    this.notices.show({
      text: t('New controller: {model}', { model }),
      detail: t('Not a model Prisme knows yet: set up its controls by learning them.'),
      actions: [
        {
          label: t('Set up'),
          run: async () => {
            await this.setEnabled(device, true);
            this.navigation.openSettings('controllers');
          },
        },
        {
          label: t('Ignore'),
          run: () => this.run(() => invoke('decline_controller', { name: device })),
        },
      ],
    });
  }

  /** `model` plays, opened; a new one may be let go at once (`actions`). */
  private showConnected(model: string, actions?: Notice['actions']) {
    this.notices.show({
      text: t('{model} connected', { model }),
      detail: t('Ready to play'),
      actions,
    });
  }

  /** Another program holds `model`: it plays here once that program lets go. */
  private showWaiting(model: string) {
    this.notices.show({
      text: t('{model} is used by another program', { model }),
      detail: t('Close that program: Prisme takes the controller at once.'),
    });
  }

  /** Looks for the controllers plugged in, and the MIDI outputs the clock can go to. */
  async look() {
    await this.run(async () => {
      const [, outputs] = await Promise.all([
        invoke('look_for_controllers'),
        invoke<string[]>('midi_outputs'),
      ]);
      this.outputs.set(outputs);
    });
  }

  /** Reads the profiles again (the DJ's own changed). */
  reloadProfiles() {
    return this.run(() => invoke('reload_controller_profiles'));
  }

  /** Fetches the profiles published since: how many came. */
  updateProfiles(): Promise<number> {
    return invoke<number>('update_controller_profiles');
  }

  /** Asks where to write what `controller` drives now as a profile, to share or keep, and
   * writes it: where it went, `null` if cancelled. What drives it does not change. */
  async exportProfile(controller: Controller): Promise<string | null> {
    const path = await save({
      defaultPath: controller.exportFile,
      filters: [{ name: 'JSON', extensions: ['json'] }],
    });
    if (!path) return null;
    await invoke('export_controller_profile', { device: controller.name, path });
    return path;
  }

  openProfilesFolder() {
    return this.run(() => invoke('open_controller_profiles_folder'));
  }

  setEnabled(name: string, enabled: boolean) {
    return this.run(() => invoke('set_controller_enabled', { name, enabled }));
  }

  /** Drives the controller `name` by `file`, another of the profiles offered for it. */
  chooseProfile(name: string, file: string) {
    return this.run(() => invoke('choose_controller_profile', { name, file }));
  }

  /** Sends the MIDI clock, in time with the sync leader, to `name` (`null`: nowhere). */
  setClockOutput(name: string | null) {
    return this.run(() => invoke('set_midi_clock_output', { name }));
  }

  learn(target: string) {
    return this.run(() => invoke('learn_control', { target }));
  }

  cancelLearning() {
    if (this.learning() === null) return;
    return this.run(() => invoke('cancel_learning'));
  }

  /** Reads the steps of `mapping`'s encoder another way. */
  setEncoding(mapping: ControllerMapping, encoding: StepEncoding) {
    const { device, target } = mapping;
    return this.run(() => invoke('set_control_encoding', { device, target, encoding }));
  }

  /** Takes `target` off the controller `device`, its profile's controls included. */
  forget(device: string, target: string) {
    return this.run(() => invoke('forget_control', { device, target }));
  }

  private run(action: () => Promise<unknown>) {
    return attempt(this.engine.error, action);
  }
}

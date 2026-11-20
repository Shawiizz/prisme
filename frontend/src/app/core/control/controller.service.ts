import { Injectable, effect, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import {
  Controller,
  ControllerMapping,
  ControllerProfiles,
  Learnt,
  PluggedIn,
  StepEncoding,
  UiAction,
} from '../models/controllers';
import { EngineService } from '../services/engine.service';
import { ControlService } from './control.service';
import { errorText } from '../errors';
import { t } from '../i18n/i18n';
import { NoticeService } from '../../shared/notice/notice';
import { NavigationService } from '../services/navigation.service';
import { LibraryService } from '../services/library.service';

/**
 * Controllers, MIDI and HID: the backend performs most of what their controls drive and lights
 * them; the actions the app performs (the library's, its windows') it is told of (`ui-action`),
 * and it tells the lights of what it keeps (`set_lights`). A control is mapped by moving it
 * (learn).
 */
@Injectable({ providedIn: 'root' })
export class ControllerService {
  private readonly controls = inject(ControlService);
  private readonly engine = inject(EngineService);
  private readonly notices = inject(NoticeService);
  private readonly navigation = inject(NavigationService);
  private readonly library = inject(LibraryService);

  readonly controllers = signal<Controller[]>([]);
  readonly profiles = signal<ControllerProfiles | null>(null);
  readonly mappings = signal<ControllerMapping[]>([]);
  /** The actions whose lights the app tells: what they show is kept here yet. */
  private readonly lightTargets = signal<string[]>([]);
  /** MIDI outputs, for the clock. */
  readonly outputs = signal<string[]>([]);
  readonly clockOutput = signal<string | null>(null);
  /** The control waiting for a move on the controller. */
  readonly learning = signal<string | null>(null);

  /** Light state last sent, by control. */
  private readonly sent = new Map<string, string>();
  /** Bumped to send every light again (a controller was opened). */
  private readonly relight = signal(0);

  constructor() {
    effect(() => this.updateLights());
  }

  async start() {
    await listen<UiAction>('ui-action', ({ payload }) => {
      this.controls.act(payload.target, payload.value);
      this.library.applied(payload.number);
    });
    await listen<unknown>('action-failed', ({ payload }) => void this.engine.failed(payload));
    await listen<ControllerMapping>('controller-learned', () => {
      this.learning.set(null);
      this.reloadMappings();
    });
    // plugged back in, or let go by the program that held it: it plays, its lights off
    await listen<string>('controller-opened', async ({ payload }) => {
      await this.refresh();
      this.sendAllLights();
      const model = this.controllers().find((c) => c.name === payload)?.profile ?? payload;
      this.notices.show({ text: t('{model} connected', { model }), detail: t('Ready to play') });
    });
    // profiles published since came
    await listen<number>('controller-profiles-changed', () => this.profilesChanged());
    await listen<PluggedIn>('controller-plugged', ({ payload }) => this.pluggedIn(payload));
    await this.reloadMappings();
    const waiting = await invoke<string[]>('waiting_controllers');
    if (waiting.length) this.showWaiting(waiting.join(', '));
  }

  /** A controller plugged in: one Prisme knows plays at once, with its lights; any other is
   * offered, until the DJ chooses not to use it. */
  private async pluggedIn({ kind, device, model }: PluggedIn) {
    if (kind === 'waiting') {
      await this.refresh();
      this.showWaiting(model);
      return;
    }
    if (kind === 'opened') {
      await this.refresh();
      await this.reloadMappings();
      this.sendAllLights();
      this.notices.show({
        text: t('{model} connected', { model }),
        detail: t('Ready to play'),
        actions: [{ label: t('Do not use it'), run: () => this.setEnabled(device, false) }],
      });
      return;
    }
    await this.refresh();
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

  /** Another program holds `model`: it plays here once that program lets go. */
  private showWaiting(model: string) {
    this.notices.show({
      text: t('{model} is used by another program', { model }),
      detail: t('Close that program: Prisme takes the controller at once.'),
    });
  }

  async refresh() {
    await this.run(async () => {
      const [controllers, outputs, clockOutput] = await Promise.all([
        invoke<Controller[]>('list_controllers'),
        invoke<string[]>('midi_outputs'),
        invoke<string | null>('midi_clock_output'),
      ]);
      this.controllers.set(controllers);
      this.outputs.set(outputs);
      this.clockOutput.set(clockOutput);
    });
  }

  async loadProfiles() {
    await this.run(async () =>
      this.profiles.set(await invoke<ControllerProfiles>('controller_profiles')),
    );
  }

  /** Reads the profiles again (the DJ's own changed). */
  async reloadProfiles() {
    await this.run(async () =>
      this.profiles.set(await invoke<ControllerProfiles>('reload_controller_profiles')),
    );
    await this.profilesChanged();
  }

  /** Fetches the profiles published since: how many came. */
  async updateProfiles(): Promise<number> {
    const came = await invoke<number>('update_controller_profiles');
    await this.profilesChanged();
    return came;
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

  /** The profiles drive the controllers otherwise: their list, their mappings, their lights. */
  private async profilesChanged() {
    await Promise.all([this.refresh(), this.loadProfiles(), this.reloadMappings()]);
    this.sendAllLights();
  }

  async setEnabled(name: string, enabled: boolean) {
    await this.run(() => invoke('set_controller_enabled', { name, enabled }));
    await this.refresh();
    await this.reloadMappings();
  }

  /** Drives the controller `name` by `file`, another of the profiles offered for it. */
  async chooseProfile(name: string, file: string) {
    await this.run(() => invoke('choose_controller_profile', { name, file }));
    await this.refresh();
    await this.reloadMappings();
  }

  /** Sends the MIDI clock, in time with the sync leader, to `name` (`null`: nowhere). */
  async setClockOutput(name: string | null) {
    await this.run(() => invoke('set_midi_clock_output', { name }));
    await this.refresh();
  }

  learn(target: string) {
    this.learning.set(target);
    const kind = this.controls.find(target)?.kind;
    const learnt: Learnt = kind === 'encoder' || kind === 'fader' ? kind : 'button';
    return this.run(() => invoke('learn_control', { target, learnt }));
  }

  cancelLearning() {
    if (this.learning() === null) return;
    this.learning.set(null);
    return this.run(() => invoke('cancel_learning'));
  }

  /** Reads the steps of `mapping`'s encoder another way. */
  async setEncoding(mapping: ControllerMapping, encoding: StepEncoding) {
    const { device, target } = mapping;
    await this.run(() => invoke('set_control_encoding', { device, target, encoding }));
    await this.reloadMappings();
  }

  /** Takes `target` off the controller `device`, its profile's controls included. */
  async forget(device: string, target: string) {
    await this.run(() => invoke('forget_control', { device, target }));
    await this.reloadMappings();
  }

  private async reloadMappings() {
    const [mappings, lightTargets] = await Promise.all([
      invoke<ControllerMapping[]>('controller_mappings'),
      invoke<string[]>('controller_light_targets'),
    ]);
    this.mappings.set(mappings);
    this.lightTargets.set(lightTargets);
    this.sendAllLights();
  }

  private sendAllLights() {
    this.sent.clear();
    this.relight.update((n) => n + 1);
  }

  /** Sends the lights the app tells that changed. */
  private updateLights() {
    this.relight();
    const lights: { target: string; state: string }[] = [];
    for (const target of this.lightTargets()) {
      const lit = this.controls.lit(target);
      if (lit === undefined) continue;
      const state = lit ? 'on' : 'off';
      if (this.sent.get(target) === state) continue;
      this.sent.set(target, state);
      lights.push({ target, state });
    }
    if (lights.length > 0) this.run(() => invoke('set_lights', { lights }));
  }

  private async run(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }
}

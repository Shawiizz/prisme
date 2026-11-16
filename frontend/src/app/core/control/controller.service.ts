import { Injectable, effect, inject, signal } from '@angular/core';
import { Pickup } from './pickup';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  Controller,
  ControllerInput,
  ControllerMapping,
  ControllerProfiles,
  StepEncoding,
} from '../models/controllers';
import { EngineService } from '../services/engine.service';
import { EngineStatus } from '../models/engine';
import { ControlService } from './control.service';
import { errorText } from '../errors';

/**
 * MIDI controllers: their mapped controls play the app's controls, the controls that
 * are on light up on the controller, and a control is mapped by moving it (learn).
 */
/** How far a step of an encoder moves a fader's setting (from 0 to 1). */
const ENCODER_STEP = 1 / 64;

/** A value shown on a controller is sent again once it changed this much (of 0 to 1). */
const OUTPUT_CHANGE = 1 / 256;
/** Turns a minute of the record a jog wheel's ring shows. */
const RECORD_RPM = 100 / 3;

@Injectable({ providedIn: 'root' })
export class ControllerService {
  private readonly controls = inject(ControlService);
  private readonly engine = inject(EngineService);

  readonly controllers = signal<Controller[]>([]);
  readonly profiles = signal<ControllerProfiles | null>(null);
  readonly mappings = signal<ControllerMapping[]>([]);
  /** Actions the profiles show on lights of their own, beside the controls that drive them. */
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
  /** The soft takeover of each fader, by control. */
  private readonly pickups = new Map<string, Pickup>();
  /** What is left of each encoder's steps short of a whole one. */
  private readonly partSteps = new Map<string, number>();
  /** The sources of the values the controllers show (`controller_output_targets`). */
  private readonly outputTargets = signal<string[]>([]);
  /** The value of each source last sent. */
  private readonly sentValues = new Map<string, number>();

  constructor() {
    effect(() => this.updateLights());
    effect(() => this.updateOutputs());
  }

  async start() {
    await listen<ControllerInput>('controller-input', ({ payload }) => this.input(payload));
    await listen<ControllerMapping>('controller-learned', () => {
      this.learning.set(null);
      this.reloadMappings();
    });
    // plugged back in, with its lights off
    await listen<string>('controller-opened', async () => {
      await this.refresh();
      this.sendAllLights();
    });
    // profiles published since came
    await listen<number>('controller-profiles-changed', () => this.profilesChanged());
    await this.reloadMappings();
  }

  /** A mapped control moved; a fader acts once it has picked its setting up, an encoder's
   * steps move it from where it is (a profile may scale them below one step: they add up). */
  private input({ target, value, steps, step }: ControllerInput) {
    const control = this.controls.findInUse(target);
    const current = control?.position?.();
    if (steps && control?.kind !== 'encoder') {
      if (current !== undefined)
        control?.apply(Math.min(Math.max(current + value * (step ?? ENCODER_STEP), 0), 1));
      return;
    }
    if (steps) {
      const total = (this.partSteps.get(target) ?? 0) + value;
      const whole = Math.trunc(total);
      this.partSteps.set(target, total - whole);
      if (whole !== 0) control?.apply(whole);
      return;
    }
    if (current !== undefined) {
      const pickup = this.pickups.get(target) ?? new Pickup();
      this.pickups.set(target, pickup);
      if (!pickup.takes(value, current)) return;
    }
    control?.apply(value);
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

  /** Writes what `device` drives now as a profile of the DJ's own: where it went. */
  async exportProfile(device: string): Promise<string> {
    const path = await invoke<string>('export_controller_profile', { device });
    await this.profilesChanged();
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

  /** Sends the MIDI clock, in time with the sync leader, to `name` (`null`: nowhere). */
  async setClockOutput(name: string | null) {
    await this.run(() => invoke('set_midi_clock_output', { name }));
    await this.refresh();
  }

  learn(target: string) {
    this.learning.set(target);
    const encoder = this.controls.find(target)?.kind === 'encoder';
    return this.run(() => invoke('learn_control', { target, encoder }));
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

  async forget(target: string) {
    await this.run(() => invoke('forget_control', { target }));
    await this.reloadMappings();
  }

  private async reloadMappings() {
    const [mappings, lightTargets, outputTargets] = await Promise.all([
      invoke<ControllerMapping[]>('controller_mappings'),
      invoke<string[]>('controller_light_targets'),
      invoke<string[]>('controller_output_targets'),
    ]);
    this.mappings.set(mappings);
    this.lightTargets.set(lightTargets);
    this.outputTargets.set(outputTargets);
    this.sentValues.clear();
    this.sendAllLights();
  }

  private sendAllLights() {
    this.sent.clear();
    this.relight.update((n) => n + 1);
  }

  /** Sends the lights that changed: the state of each mapped control that shows one. */
  private updateLights() {
    this.relight();
    const lights: { target: string; state: string; color: string | null }[] = [];
    for (const target of new Set([
      ...this.mappings().map((m) => m.target),
      ...this.lightTargets(),
    ])) {
      const control = this.controls.findInUse(target);
      const on = control?.lit?.();
      const state = control?.state?.() ?? (on === undefined ? undefined : on ? 'on' : 'off');
      const color = control?.color?.() ?? null;
      const shown = `${state} ${color}`;
      if (state === undefined || this.sent.get(target) === shown) continue;
      this.sent.set(target, shown);
      lights.push({ target, state, color });
    }
    if (lights.length > 0) this.run(() => invoke('set_lights', { lights }));
  }

  /** Sends the values the controllers show that changed: a fader's position, the place in
   * its track or the turn of its record of a deck (`deck1.position`, `deck1.turn`). */
  private updateOutputs() {
    const targets = this.outputTargets();
    if (targets.length === 0) return;
    const status = this.engine.status();
    const values: { target: string; value: number }[] = [];
    for (const target of targets) {
      const value = this.outputValue(target, status);
      if (value === null || Math.abs((this.sentValues.get(target) ?? -1) - value) < OUTPUT_CHANGE)
        continue;
      this.sentValues.set(target, value);
      values.push({ target, value });
    }
    if (values.length > 0) this.run(() => invoke('set_controller_outputs', { values }));
  }

  /** A fader's position, or what a deck's track shows: its place (`position`, 0 to 1), its
   * record's turn, its tempo (`bpm`), its times in seconds, its tempo change and range in
   * percent. */
  private outputValue(target: string, status: EngineStatus | null): number | null {
    const measured =
      /^deck(\d)\.(position|turn|bpm|elapsed|remaining|duration|tempo|tempoRange)$/.exec(target);
    if (!measured) return this.controls.findInUse(target)?.position?.() ?? null;
    const at = Number(measured[1]) - 1;
    const deck = status?.decks[at];
    if (!deck?.loaded) return null;
    switch (measured[2]) {
      case 'position':
        return deck.durationS > 0 ? deck.positionS / deck.durationS : 0;
      case 'turn': {
        const turns = (deck.positionS * RECORD_RPM) / 60;
        return turns - Math.floor(turns);
      }
      case 'bpm':
        return deck.bpm;
      case 'elapsed':
        return deck.positionS;
      case 'remaining':
        return Math.max(deck.durationS - deck.positionS, 0);
      case 'duration':
        return deck.durationS;
      case 'tempo':
        return ((this.engine.tempoRate(at) ?? this.engine.decks()[at].pitch) - 1) * 100;
      default:
        return this.engine.tempoRange(at) * 100;
    }
  }

  private async run(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }
}

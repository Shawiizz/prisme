import { Injectable, effect, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Controller, ControllerInput, ControllerMapping } from '../models/controllers';
import { EngineService } from '../services/engine.service';
import { ControlService } from './control.service';

/**
 * MIDI controllers: their mapped controls play the app's controls, the controls that
 * are on light up on the controller, and a control is mapped by moving it (learn).
 */
@Injectable({ providedIn: 'root' })
export class ControllerService {
  private readonly controls = inject(ControlService);
  private readonly engine = inject(EngineService);

  readonly controllers = signal<Controller[]>([]);
  readonly mappings = signal<ControllerMapping[]>([]);
  /** The control waiting for a move on the controller. */
  readonly learning = signal<string | null>(null);

  /** Light state last sent, by control. */
  private readonly sent = new Map<string, boolean>();
  /** Bumped to send every light again (a controller was opened). */
  private readonly relight = signal(0);

  constructor() {
    effect(() => this.updateLights());
  }

  async start() {
    await listen<ControllerInput>('controller-input', ({ payload }) =>
      this.controls.find(payload.target)?.apply(payload.value),
    );
    await listen<ControllerMapping>('controller-learned', () => {
      this.learning.set(null);
      this.reloadMappings();
    });
    await this.reloadMappings();
  }

  async refresh() {
    await this.run(async () =>
      this.controllers.set(await invoke<Controller[]>('list_controllers')),
    );
  }

  async setEnabled(name: string, enabled: boolean) {
    await this.run(() => invoke('set_controller_enabled', { name, enabled }));
    await this.refresh();
    this.sendAllLights();
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

  async forget(target: string) {
    await this.run(() => invoke('forget_control', { target }));
    await this.reloadMappings();
  }

  private async reloadMappings() {
    this.mappings.set(await invoke<ControllerMapping[]>('controller_mappings'));
    this.sendAllLights();
  }

  private sendAllLights() {
    this.sent.clear();
    this.relight.update((n) => n + 1);
  }

  /** Sends the lights that changed. */
  private updateLights() {
    this.relight();
    const lights: { target: string; on: boolean }[] = [];
    for (const target of new Set(this.mappings().map((m) => m.target))) {
      const on = this.controls.find(target)?.lit?.();
      if (on === undefined || this.sent.get(target) === on) continue;
      this.sent.set(target, on);
      lights.push({ target, on });
    }
    if (lights.length > 0) this.run(() => invoke('set_lights', { lights }));
  }

  private async run(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (e) {
      this.engine.error.set(String(e));
    }
  }
}

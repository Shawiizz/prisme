import { Component, OnInit, computed, inject, output, signal } from '@angular/core';
import { SettingsService } from '../../core/services/settings.service';
import { PreferencesService } from '../../core/services/preferences.service';
import { MixerService } from '../../core/services/mixer.service';
import { AudioBackend, AudioSettings, BUFFER_SIZES } from '../../core/models/audio';
import { MixMode } from '../../core/models/engine';
import { DjPreferences, PITCH_RANGES } from '../../core/models/preferences';

/** Settings: audio backend and devices, mixing, turntables, DJ preferences. */
@Component({
  selector: 'app-settings-dialog',
  templateUrl: './settings-dialog.html',
  styleUrl: './settings-dialog.scss',
})
export class SettingsDialog implements OnInit {
  protected readonly service = inject(SettingsService);
  private readonly preferences = inject(PreferencesService);
  private readonly mixer = inject(MixerService);
  readonly closed = output<void>();

  protected readonly pitchRanges = PITCH_RANGES;
  protected readonly draft = signal<AudioSettings | null>(null);
  protected readonly dj = signal<DjPreferences>(this.preferences.dj());
  protected readonly mixMode = signal<MixMode>(this.mixer.mode());
  protected readonly busy = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly outputs = computed(() => this.service.devices().filter((d) => d.kind === 'output'));
  protected readonly inputs = computed(() => this.service.devices().filter((d) => d.kind === 'input'));
  protected readonly asio = computed(() => this.draft()?.backend === 'asio');

  /** Buffer sizes the chosen output's driver accepts. */
  protected readonly bufferSizes = computed(() => {
    const d = this.draft();
    const device = this.outputs().find((o) => o.name === d?.outputDevice) ?? this.outputs().find((o) => o.isDefault);
    const range = device?.bufferRange;
    return range ? BUFFER_SIZES.filter((n) => n >= range[0] && n <= range[1]) : BUFFER_SIZES;
  });

  async ngOnInit() {
    await this.service.refresh();
    this.draft.set(structuredClone(this.service.settings()));
  }

  update(changes: Partial<AudioSettings>) {
    const d = this.draft();
    if (d) this.draft.set({ ...d, ...changes });
  }

  updateDj(changes: Partial<DjPreferences>) {
    this.dj.set({ ...this.dj(), ...changes });
  }

  /** Another backend has other devices: start again from its defaults. */
  async setBackend(backend: AudioBackend) {
    this.update({ backend, outputDevice: null, inputDevice: null, bufferFrames: null });
    await this.service.loadDevices(backend);
  }

  /** ASIO: one driver for the outputs and, if the turntables are on it, the inputs. */
  setDriver(name: string | null) {
    const d = this.draft();
    this.update({ outputDevice: name, inputDevice: d?.inputDevice ? name : null });
  }

  setTurntablesOnDriver(enabled: boolean) {
    this.update({ inputDevice: enabled ? this.draft()?.outputDevice ?? null : null });
  }

  /** Restarts the audio only if the audio settings changed. */
  async apply() {
    const d = this.draft();
    if (!d) return;
    this.busy.set(true);
    this.error.set(null);
    try {
      await this.preferences.updateDj(this.dj());
      if (JSON.stringify(d) !== JSON.stringify(this.service.settings())) {
        await this.service.apply(d);
      }
      await this.mixer.setMode(this.mixMode());
      this.closed.emit();
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.busy.set(false);
    }
  }

  /** Select value -> setting (empty string = default / none). */
  orNull(v: string): string | null {
    return v === '' ? null : v;
  }

  bufferLabel(frames: number): string {
    const rate = this.service.info()?.sampleRate ?? 48_000;
    return `${frames} samples · ${((frames / rate) * 1000).toFixed(1)} ms`;
  }
}

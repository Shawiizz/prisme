import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import {
  AudioBackend,
  AudioDevice,
  AudioInfo,
  AudioSettings,
  TimecodeFormat,
} from '../models/audio';
import { decimal } from '../../shared/time';
import { EngineService } from './engine.service';

/** Audio settings (backend, devices, buffer, simulation), the timecode records Prisme reads,
 * and diagnostics. */
@Injectable({ providedIn: 'root' })
export class SettingsService {
  readonly backends = signal<AudioBackend[]>([]);
  readonly devices = signal<AudioDevice[]>([]);
  /** The backend `devices` are of. */
  readonly devicesBackend = signal<AudioBackend | null>(null);
  readonly formats = signal<TimecodeFormat[]>([]);
  readonly settings = signal<AudioSettings | null>(null);
  readonly info = signal<AudioInfo | null>(null);
  private readonly engine = inject(EngineService);
  /** The length of an audio period, `5.3 ms`, once the driver has called the engine. */
  readonly period = computed(() => {
    const frames = this.engine.status()?.health?.periodFrames;
    const rate = this.info()?.sampleRate;
    return frames && rate ? `${decimal((frames / rate) * 1000, 1)} ms` : null;
  });
  /** Turntables can drive the decks: the audio reads their timecode from inputs, or
   * simulates it. */
  readonly turntables = computed(() => {
    const info = this.info();
    return !!info && (info.simulated || info.inputChannels >= 2);
  });
  private devicesAsked = 0;

  /** Loads the settings, and again whenever the audio moves to other devices by itself. */
  async init(): Promise<void> {
    await listen('audio-changed', () => this.refresh());
    await this.refresh();
  }

  async refresh(): Promise<void> {
    const [backends, formats, settings, info] = await Promise.all([
      invoke<AudioBackend[]>('audio_backends'),
      invoke<TimecodeFormat[]>('timecode_formats'),
      invoke<AudioSettings>('get_audio_settings'),
      invoke<AudioInfo | null>('audio_info'),
    ]);
    this.backends.set(backends);
    this.formats.set(formats);
    this.settings.set(settings);
    this.info.set(info);
    await this.loadDevices(settings.backend);
  }

  /** The devices of the backend asked for last: a slower answer for another one is dropped. */
  async loadDevices(backend: AudioBackend): Promise<void> {
    const asked = ++this.devicesAsked;
    const devices = await invoke<AudioDevice[]>('list_devices', { backend });
    if (asked === this.devicesAsked) {
      this.devices.set(devices);
      this.devicesBackend.set(backend);
    }
  }

  /** Asks where to save a diagnostic report and writes it; `false` if cancelled. */
  async saveDiagnostics(): Promise<boolean> {
    const path = await save({
      defaultPath: 'prisme-diagnostic.txt',
      filters: [{ name: 'Text', extensions: ['txt'] }],
    });
    if (!path) return false;
    await invoke('save_diagnostics', { path });
    return true;
  }

  /** Restarts the audio with new settings. Throws the error message on failure. */
  async apply(settings: AudioSettings): Promise<AudioInfo> {
    const info = await invoke<AudioInfo>('apply_audio_settings', { settings });
    this.settings.set(settings);
    this.info.set(info);
    return info;
  }
}

import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import {
  AudioBackend,
  AudioDevice,
  AudioInfo,
  AudioSettings,
  TimecodeFormat,
} from '../models/audio';

/** Audio settings: backend, devices, buffer, timecode format, simulation. */
@Injectable({ providedIn: 'root' })
export class SettingsService {
  readonly backends = signal<AudioBackend[]>(['wasapi']);
  readonly devices = signal<AudioDevice[]>([]);
  readonly formats = signal<TimecodeFormat[]>([]);
  readonly settings = signal<AudioSettings | null>(null);
  readonly info = signal<AudioInfo | null>(null);

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

  async loadDevices(backend: AudioBackend): Promise<void> {
    this.devices.set(await invoke<AudioDevice[]>('list_devices', { backend }));
  }

  /** Restarts the audio with new settings. Throws the error message on failure. */
  async apply(settings: AudioSettings): Promise<AudioInfo> {
    const info = await invoke<AudioInfo>('apply_audio_settings', { settings });
    this.settings.set(settings);
    this.info.set(info);
    return info;
  }
}

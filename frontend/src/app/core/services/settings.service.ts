import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { AudioDevice, AudioInfo, AudioSettings, TimecodeFormat } from '../models/audio';

/** Audio settings: devices, timecode format, simulation. */
@Injectable({ providedIn: 'root' })
export class SettingsService {
  readonly devices = signal<AudioDevice[]>([]);
  readonly formats = signal<TimecodeFormat[]>([]);
  readonly settings = signal<AudioSettings | null>(null);
  readonly info = signal<AudioInfo | null>(null);

  async refresh(): Promise<void> {
    const [devices, formats, settings, info] = await Promise.all([
      invoke<AudioDevice[]>('list_devices'),
      invoke<TimecodeFormat[]>('timecode_formats'),
      invoke<AudioSettings>('get_audio_settings'),
      invoke<AudioInfo | null>('audio_info'),
    ]);
    this.devices.set(devices);
    this.formats.set(formats);
    this.settings.set(settings);
    this.info.set(info);
  }

  /** Restarts the audio with new settings. Throws the error message on failure. */
  async apply(settings: AudioSettings): Promise<AudioInfo> {
    const info = await invoke<AudioInfo>('apply_audio_settings', { settings });
    this.settings.set(settings);
    this.info.set(info);
    return info;
  }
}

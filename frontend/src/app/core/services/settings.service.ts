import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import {
  AudioBackend,
  AudioDevice,
  AudioInfo,
  AudioSettings,
  BackendChoice,
  CardOffer,
  TimecodeMaker,
} from '../models/audio';
import { decimal } from '../../shared/time';
import { MixMode } from '../models/engine';
import { EngineService } from './engine.service';
import { NoticeService } from '../../shared/notice/notice';
import { t } from '../i18n/i18n';
import { errorText } from '../errors';

/** Audio settings (backend, devices, buffer, simulation), the timecode records Prisme reads,
 * and diagnostics. */
@Injectable({ providedIn: 'root' })
export class SettingsService {
  readonly backends = signal<BackendChoice[]>([]);
  readonly devices = signal<AudioDevice[]>([]);
  /** The backend `devices` are of. */
  readonly devicesBackend = signal<AudioBackend | null>(null);
  /** The timecode records Prisme reads, by maker. */
  readonly makers = signal<TimecodeMaker[]>([]);
  readonly settings = signal<AudioSettings | null>(null);
  readonly info = signal<AudioInfo | null>(null);
  private readonly engine = inject(EngineService);
  private readonly notices = inject(NoticeService);
  /** The length of an audio period, `5.3 ms`, once the driver has called the engine. */
  readonly period = computed(() => {
    const ms = this.engine.status()?.health?.periodMs;
    return ms ? `${decimal(ms, 1)} ms` : null;
  });
  /** Turntables can drive the decks. */
  readonly turntables = computed(() => this.info()?.turntables ?? false);
  private devicesAsked = 0;

  /** Loads the settings, and again whenever the audio moves to other devices by itself. */
  async init(): Promise<void> {
    await listen('audio-changed', () => this.refresh());
    await listen<CardOffer>('card-plugged', ({ payload }) => this.offer(payload));
    await this.refresh();
  }

  /** A sound card plugged in, offered: used at once, or not offered again. */
  private offer({ name, settings }: CardOffer) {
    this.notices.show({
      text: t('New sound card: {name}', { name }),
      detail: settings.inputDevice
        ? t('Play on it and read the turntables from it?')
        : t('Play on it?'),
      actions: [
        {
          label: t('Use'),
          run: () => this.apply(settings).catch((e) => this.engine.error.set(errorText(e))),
        },
        {
          label: t('Ignore'),
          run: () =>
            invoke('decline_card', { name }).catch((e) => this.engine.error.set(errorText(e))),
        },
      ],
    });
  }

  async refresh(): Promise<void> {
    const [backends, makers, settings, info] = await Promise.all([
      invoke<BackendChoice[]>('audio_backends'),
      invoke<TimecodeMaker[]>('timecode_formats'),
      invoke<AudioSettings>('get_audio_settings'),
      invoke<AudioInfo | null>('audio_info'),
    ]);
    this.backends.set(backends);
    this.makers.set(makers);
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

  /** Restarts the audio with new settings if they changed (the decks and the mixer as it lays
   * them out come with the engine's state), then uses the mixer `mixMode` if given. Throws the
   * error message on failure. */
  async apply(settings: AudioSettings, mixMode: MixMode | null = null): Promise<void> {
    const info = await invoke<AudioInfo | null>('apply_audio_settings', { settings, mixMode });
    this.settings.set(settings);
    if (info) this.info.set(info);
  }
}

import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { decimal } from '../../shared/time';
import { EngineService } from './engine.service';
import { NoticeService } from '../../shared/notice/notice';
import { t } from '../i18n/i18n';
import { attempt, cardAskedText } from '../errors';
import type {
  AudioBackend,
  AudioChange,
  AudioDevice,
  AudioDraft,
  AudioSettings,
  BackendChoice,
  CardOffer,
  Latency,
  LatencyChannels,
  MixMode,
  TimecodeMaker,
} from '../models/backend';

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
  private readonly engine = inject(EngineService);
  /** The settings the DJ chose, as the backend keeps them. */
  readonly settings = computed(() => this.engine.told().audio.settings);
  /** The audio running. */
  readonly info = computed(() => this.engine.told().audio.info);
  /** The USB card the DJ is asked to let Prisme open, or refused it, in words. */
  readonly asked = computed(() => {
    const asked = this.engine.told().audio.asked;
    return asked ? cardAskedText(asked) : null;
  });
  private readonly notices = inject(NoticeService);
  /** The length of an audio period, `5.3 ms`, once the driver has called the engine. */
  readonly period = computed(() => {
    const ms = this.engine.status()?.health?.periodMs;
    return ms ? `${decimal(ms, 1)} ms` : null;
  });
  /** Turntables can drive the decks. */
  readonly turntables = computed(() => this.info()?.turntables ?? false);
  private devicesAsked = 0;

  /** Loads the backends and the records offered, and offers the sound cards plugged in. */
  async init(): Promise<void> {
    await listen<CardOffer>('card-plugged', ({ payload }) => this.offer(payload));
    const [, makers] = await Promise.all([
      this.loadBackends(),
      invoke<TimecodeMaker[]>('timecode_formats'),
    ]);
    this.makers.set(makers);
  }

  /** The backends and their drivers, read again as the settings open: a driver installed
   * meanwhile shows. */
  async loadBackends(): Promise<void> {
    this.backends.set(await invoke<BackendChoice[]>('audio_backends'));
  }

  /** A sound card plugged in, offered: used at once, or not offered again. */
  private offer({ controller, name, settings }: CardOffer) {
    this.notices.show({
      text: controller
        ? t('{controller} plugged in', { controller })
        : t('New sound card: {name}', { name }),
      detail: controller
        ? t('Play on its sound card? Prisme mixes for it: it has no mixer of its own.')
        : settings.inputDevice
          ? t('Play on it and read the turntables from it?')
          : t('Play on it?'),
      stays: controller !== null,
      prominent: controller !== null,
      actions: [
        {
          label: controller ? t('Play on it') : t('Use'),
          run: () => attempt(this.engine.error, () => this.apply(settings)),
        },
        {
          label: t('Ignore'),
          run: () => attempt(this.engine.error, () => invoke('decline_card', { name })),
        },
      ],
    });
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

  /** `draft` with `change` made to it (none: as it is), as the backend shapes it. */
  shapeDraft(draft: AudioSettings, change: AudioChange | null): Promise<AudioDraft> {
    return invoke<AudioDraft>('change_audio_draft', { draft, change });
  }

  /** The channels a latency measurement may join on the running device. */
  latencyChannels(): Promise<LatencyChannels> {
    return invoke<LatencyChannels>('latency_channels');
  }

  /** Clicks played on `output` and their return timed on `input` (from 1): the round trip. */
  async measureLatency(output: number, input: number): Promise<number> {
    const measured = await invoke<Latency>('measure_latency', { output, input });
    return measured.roundTripMs;
  }

  /** Restarts the audio with new settings if they changed (what runs then comes with the
   * engine's state), then uses the mixer `mixMode` if given. Throws the error message on
   * failure. */
  async apply(settings: AudioSettings, mixMode: MixMode | null = null): Promise<void> {
    await invoke('apply_audio_settings', { settings, mixMode });
  }
}

import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { RecordSource, Recording } from '../models/engine';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';
import { SettingsService } from './settings.service';

/** A source to record, as offered to the DJ. */
export interface RecordOption {
  source: RecordSource;
  label: string;
  title: string;
}

/**
 * Recording the mix to a WAV file in Documents/Prisme Recordings, with its tracklist
 * (text and CUE) from the history of the set.
 */
@Injectable({ providedIn: 'root' })
export class RecordingService {
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);
  private readonly settings = inject(SettingsService);

  readonly status = computed(() => this.engine.status()?.recording ?? null);
  /** The last recording finished. */
  readonly last = signal<Recording | null>(null);

  /** The software mixer's master, and each input pair (a hardware mixer's record out). */
  readonly options = computed<RecordOption[]>(() => {
    const options: RecordOption[] = [];
    if (this.mixer.internal()) {
      options.push({
        source: { kind: 'master' },
        label: 'Master',
        title: 'What the software mixer plays',
      });
    }
    const inputs = this.settings.info()?.inputChannels ?? 0;
    for (let first = 0; first + 1 < inputs; first += 2) {
      options.push({
        source: { kind: 'input', first },
        label: `Inputs ${first + 1}-${first + 2}`,
        title: "What comes into these inputs: plug the hardware mixer's record output there",
      });
    }
    return options;
  });

  async start(source: RecordSource): Promise<void> {
    try {
      await invoke<string>('start_recording', { source });
      this.engine.error.set(null);
    } catch (e) {
      this.engine.error.set(String(e));
    }
  }

  async stop(): Promise<void> {
    try {
      this.last.set(await invoke<Recording | null>('stop_recording'));
    } catch (e) {
      this.engine.error.set(String(e));
    }
  }
}

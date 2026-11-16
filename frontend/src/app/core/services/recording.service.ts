import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { RecordSource, Recording } from '../models/engine';
import { t } from '../i18n/i18n';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';
import { SettingsService } from './settings.service';
import { errorText } from '../errors';
import { preferredSource } from '../record-source';

/** A source to record, as offered to the DJ. */
export interface RecordOption {
  source: RecordSource;
  label: string;
  title: string;
}

/**
 * Recording the mix to a WAV file in Documents/Prisme Recordings, with its tracklist
 * (text and CUE) from the history of the set. The Rec menu offers what can be recorded; what
 * is started without asking takes the source recorded last (see `preferredSource`).
 */
@Injectable({ providedIn: 'root' })
export class RecordingService {
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);
  private readonly settings = inject(SettingsService);

  readonly status = computed(() => this.engine.status()?.recording ?? null);
  /** The last recording finished. */
  readonly last = signal<Recording | null>(null);

  /** The software mixer's master, and each input pair (a hardware mixer's record out), those
   * a turntable comes into told apart. */
  readonly options = computed<RecordOption[]>(() => {
    const options: RecordOption[] = [];
    if (this.mixer.internal()) {
      options.push({
        source: { kind: 'master' },
        label: t('Master'),
        title: t('What the software mixer plays'),
      });
    }
    const inputs = this.settings.info()?.inputChannels ?? 0;
    const free = this.mixer.freeInputs();
    for (let first = 0; first + 1 < inputs; first += 2) {
      const pair = { from: first + 1, to: first + 2 };
      options.push(
        free.includes(first)
          ? {
              source: { kind: 'input', first },
              label: t('Inputs {from}-{to}', pair),
              title: t(
                "What comes into these inputs: plug the hardware mixer's record output there",
              ),
            }
          : {
              source: { kind: 'input', first },
              label: t('Inputs {from}-{to}: a turntable', pair),
              title: t('A turntable comes into these inputs: its timecode would be recorded'),
            },
      );
    }
    return options;
  });
  /** What a recording started without asking takes, if anything. */
  readonly preferred = computed(() =>
    preferredSource(
      this.options().map((o) => o.source),
      this.mixer.recordSource(),
      this.mixer.freeInputs(),
    ),
  );

  async start(source: RecordSource): Promise<void> {
    try {
      await invoke<string>('start_recording', { source });
      this.mixer.recordSource.set(source);
      this.engine.error.set(null);
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }

  /** Records what was recorded last, or the software mixer's master; says so when there is
   * nothing it may take without asking. */
  startPreferred(): Promise<void> {
    const source = this.preferred();
    if (source) return this.start(source);
    this.engine.error.set(
      t('Nothing to record without asking: choose the inputs of your mixer in the Rec menu'),
    );
    return Promise.resolve();
  }

  async stop(): Promise<void> {
    try {
      this.last.set(await invoke<Recording | null>('stop_recording'));
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }
}

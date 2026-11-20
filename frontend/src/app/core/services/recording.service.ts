import { Injectable, computed, inject } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { RecordSource, Recording } from '../models/engine';
import { t } from '../i18n/i18n';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';
import { errorText } from '../errors';
import { PHONE } from '../platform';
import { NoticeService } from '../../shared/notice/notice';
import { recordingNotice } from '../recording-notice';

/** A source to record, as offered to the DJ. */
export interface RecordOption {
  source: RecordSource;
  label: string;
  title: string;
}

/**
 * Recording the mix to a WAV file in Documents/Prisme Recordings, with its tracklist
 * (text and CUE) from the history of the set. The Rec menu offers what can be recorded; what
 * is started without asking (`general.record`) takes the source recorded last.
 */
@Injectable({ providedIn: 'root' })
export class RecordingService {
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);
  private readonly notices = inject(NoticeService);

  readonly status = computed(() => this.engine.status()?.recording ?? null);
  /** The last recording finished. */
  readonly last = computed(() => this.engine.controls()?.mixer.lastRecording ?? null);

  /** The software mixer's master, and each input pair (a hardware mixer's record out), those
   * a turntable comes into told apart. */
  readonly options = computed<RecordOption[]>(() =>
    this.mixer.recordOptions().map(({ source, turntable }) => {
      if (source.kind === 'master')
        return { source, label: t('Master'), title: t('What the software mixer plays') };
      const pair = { from: source.first + 1, to: source.first + 2 };
      return turntable
        ? {
            source,
            label: t('Inputs {from}-{to}: a turntable', pair),
            title: t('A turntable comes into these inputs: its timecode would be recorded'),
          }
        : {
            source,
            label: t('Inputs {from}-{to}', pair),
            title: t("What comes into these inputs: plug the hardware mixer's record output there"),
          };
    }),
  );
  start(source: RecordSource) {
    return this.engine.call('start_recording', { source });
  }

  /** Tells where each recording went as it ends, whatever ended it (the Rec key, a
   * controller, the audio lost). */
  async init(): Promise<void> {
    await listen<Recording>('recording-saved', (e) => this.saved(e.payload));
  }

  stop() {
    return this.engine.call('stop_recording', {});
  }

  private saved(recording: Recording) {
    this.notices.show({
      ...recordingNotice(recording, PHONE),
      // a phone shows no folders: the notice says where it went
      actions: PHONE
        ? []
        : [
            {
              label: t('Show in folder'),
              run: () =>
                invoke('show_recording', { path: recording.path }).catch((e) =>
                  this.engine.error.set(errorText(e)),
                ),
            },
          ],
    });
  }
}

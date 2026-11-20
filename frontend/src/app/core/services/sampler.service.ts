import { Injectable, inject } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { t } from '../i18n/i18n';
import { EngineService } from './engine.service';

/** Each deck's sampler: short sounds played from their start, on the beat with quantize. The
 * backend performs it; its slots are shown as it tells them (`EngineService.decks`). */
@Injectable({ providedIn: 'root' })
export class SamplerService {
  private readonly engine = inject(EngineService);

  /** Asks for a sound and puts it in `slot`. */
  async choose(deck: number, slot: number) {
    const extensions = await invoke<string[]>('audio_extensions');
    const path = await open({
      title: t('Sampler sound'),
      filters: [{ name: t('Audio'), extensions }],
    });
    if (path) await this.engine.call('load_sample', { deck, slot, path });
  }

  clear(deck: number, slot: number) {
    return this.engine.perform(sampleId(deck, slot, '.eject'), 1);
  }

  /** Plays `slot` at the sampler's level. */
  trigger(deck: number, slot: number) {
    return this.engine.perform(sampleId(deck, slot), 1);
  }

  stop(deck: number, slot: number) {
    return this.engine.perform(sampleId(deck, slot, '.stop'), 1);
  }

  setGain(deck: number, gain: number) {
    return this.engine.perform(`deck${deck + 1}.sampler.gain`, gain);
  }
}

const sampleId = (deck: number, slot: number, action = '') =>
  `deck${deck + 1}.sample${slot + 1}${action}`;

import { Injectable, inject } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { t } from '../i18n/i18n';
import { EngineService } from './engine.service';
import { errorText } from '../errors';
import { AUDIO_EXTENSIONS } from '../audio-files';

const AUDIO_FILES = { name: () => t('Audio'), extensions: AUDIO_EXTENSIONS };

/** Each deck's sampler: short sounds played from their start, on the beat with quantize. */
@Injectable({ providedIn: 'root' })
export class SamplerService {
  private readonly engine = inject(EngineService);

  /** Asks for a sound and puts it in `slot`. */
  async choose(deck: number, slot: number) {
    const path = await open({
      title: t('Sampler sound'),
      filters: [{ ...AUDIO_FILES, name: AUDIO_FILES.name() }],
    });
    if (path) await this.load(deck, slot, path);
  }

  /** Puts the sound of `path` in `slot`. */
  async load(deck: number, slot: number, path: string) {
    try {
      this.setName(deck, slot, await invoke<string>('load_sample', { deck, slot, path }));
      this.engine.error.set(null);
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }

  clear(deck: number, slot: number) {
    this.setName(deck, slot, null);
    return this.engine.call('clear_sample', { deck, slot });
  }

  /** Plays `slot` at the sampler's level. */
  trigger(deck: number, slot: number) {
    return this.engine.call('trigger_sample', { deck, slot });
  }

  stop(deck: number, slot: number) {
    return this.engine.call('stop_sample', { deck, slot });
  }

  setGain(deck: number, gain: number) {
    this.engine.patchDeck(deck, { samplerGain: gain });
    return this.engine.call('set_sampler_gain', { deck, gain });
  }

  private setName(deck: number, slot: number, name: string | null) {
    const samples = [...this.engine.decks()[deck].samples];
    samples[slot] = name;
    this.engine.patchDeck(deck, { samples });
  }
}

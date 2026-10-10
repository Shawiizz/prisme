import { Injectable, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { MixMode, MixerState } from '../models/engine';
import { EngineService } from './engine.service';

/** Where the decks are heard: a hardware mixer, or the software mixer and its headphones. */
@Injectable({ providedIn: 'root' })
export class MixerService {
  private readonly engine = inject(EngineService);

  readonly crossfader = signal(0);
  /** The mode the user chose. */
  readonly mode = signal<MixMode>('external');
  /** The mode in use: an external mixer needs an output pair per deck. */
  readonly modeInUse = signal<MixMode | null>(null);
  readonly cueMix = signal(0);
  readonly headphoneGain = signal(1);

  apply(state: MixerState) {
    this.crossfader.set(state.crossfader);
    this.mode.set(state.mode);
    this.modeInUse.set(state.modeInUse);
    this.cueMix.set(state.cueMix);
    this.headphoneGain.set(state.headphoneGain);
  }

  async setMode(mode: MixMode) {
    this.mode.set(mode);
    try {
      this.modeInUse.set(await invoke<MixMode | null>('set_mix_mode', { mode }));
    } catch (e) {
      this.engine.error.set(String(e));
    }
  }

  setVolume(deck: number, gain: number) {
    this.engine.patchDeck(deck, { volume: gain });
    return this.engine.call('set_volume', { deck, gain });
  }

  setDeckCue(deck: number, enabled: boolean) {
    this.engine.patchDeck(deck, { cue: enabled });
    return this.engine.call('set_deck_cue', { deck, enabled });
  }

  setCrossfader(position: number) {
    this.crossfader.set(position);
    return this.engine.call('set_crossfader', { position });
  }

  setCueMix(mix: number) {
    this.cueMix.set(mix);
    return this.engine.call('set_cue_mix', { mix });
  }

  setHeadphoneGain(gain: number) {
    this.headphoneGain.set(gain);
    return this.engine.call('set_headphone_gain', { gain });
  }
}

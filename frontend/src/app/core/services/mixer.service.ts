import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { BANDS, Band, ChannelStrip, CrossfaderCurve, MixMode, MixerState } from '../models/engine';
import { EngineService } from './engine.service';

/**
 * Where the decks are heard: a hardware mixer, or the software mixer (channel strips,
 * faders, crossfader, master limiter) and its headphones.
 */
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
  readonly crossfaderCurve = signal<CrossfaderCurve>('smooth');
  readonly limiter = signal(true);
  /** The software mixer is in use. */
  readonly internal = computed(() => (this.modeInUse() ?? this.mode()) === 'internal');
  /** External mode asked for, but the device has too few outputs: the software mixer runs. */
  readonly fallback = computed(() => this.mode() === 'external' && this.modeInUse() === 'internal');

  apply(state: MixerState) {
    this.crossfader.set(state.crossfader);
    this.mode.set(state.mode);
    this.modeInUse.set(state.modeInUse);
    this.cueMix.set(state.cueMix);
    this.headphoneGain.set(state.headphoneGain);
    this.crossfaderCurve.set(state.crossfaderCurve);
    this.limiter.set(state.limiter);
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

  strip(deck: number): ChannelStrip {
    return this.engine.decks()[deck].strip;
  }

  setTrim(deck: number, gain: number) {
    this.patchStrip(deck, { trim: gain });
    return this.engine.call('set_trim', { deck, gain });
  }

  setEq(deck: number, band: Band, gain: number) {
    const eq = [...this.strip(deck).eq];
    eq[BANDS.indexOf(band)] = gain;
    this.patchStrip(deck, { eq });
    return this.engine.call('set_eq', { deck, band, gain });
  }

  setKill(deck: number, band: Band, kill: boolean) {
    const kills = [...this.strip(deck).kills];
    kills[BANDS.indexOf(band)] = kill;
    this.patchStrip(deck, { kills });
    return this.engine.call('set_eq_kill', { deck, band, kill });
  }

  setFilter(deck: number, position: number) {
    this.patchStrip(deck, { filter: position });
    return this.engine.call('set_filter', { deck, position });
  }

  setCrossfaderCurve(curve: CrossfaderCurve) {
    this.crossfaderCurve.set(curve);
    return this.engine.call('set_crossfader_curve', { curve });
  }

  setLimiter(enabled: boolean) {
    this.limiter.set(enabled);
    return this.engine.call('set_limiter', { enabled });
  }

  private patchStrip(deck: number, changes: Partial<ChannelStrip>) {
    this.engine.patchDeck(deck, { strip: { ...this.strip(deck), ...changes } });
  }
}

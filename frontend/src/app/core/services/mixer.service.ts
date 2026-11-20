import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import {
  Band,
  ChannelStrip,
  CrossfaderCurve,
  CrossfaderSide,
  HeadphonesOutput,
  InputChannels,
  LiveSource,
  LiveState,
  MixMode,
  MixerState,
  RecordSource,
} from '../models/engine';
import { EngineService } from './engine.service';
import { SettingsService } from './settings.service';
import { errorText } from '../errors';
import { same } from '../same';

const LIVE_OFF: LiveState = { input: null, inUse: false, level: 1, on: false, cue: false };

/**
 * Where the decks are heard: a hardware mixer, or the software mixer (channel strips,
 * faders, crossfader, the microphone and the aux line, master limiter and balance, booth)
 * and its headphones.
 */
@Injectable({ providedIn: 'root' })
export class MixerService {
  private readonly engine = inject(EngineService);
  private readonly settings = inject(SettingsService);

  readonly crossfader = signal(0);
  /** The mode the user chose. */
  readonly mode = signal<MixMode>('external');
  /** The mode in use: an external mixer needs an output pair per deck. */
  readonly modeInUse = signal<MixMode | null>(null);
  /** Where the user wants the headphones: those of the software mixer, or with an external
   * mixer those of the preview. */
  readonly headphones = signal<HeadphonesOutput>({ kind: 'auto' }, { equal: same });
  /** First output (0-based) of the headphones in use; `null` when they are not heard. With an
   * external mixer, a pair no deck uses, for the preview alone. */
  readonly headphonesInUse = signal<number | null>(null);
  /** Outputs of the running device. */
  readonly outputChannels = signal<number | null>(null);
  readonly cueMix = signal(0);
  /** Headphones: the cue on the left and the master on the right, each in mono. */
  readonly splitCue = signal(false);
  readonly headphoneGain = signal(1);
  readonly crossfaderCurve = signal<CrossfaderCurve>('smooth');
  /** The crossfader's sides swapped: towards the left, the decks on the right come in. */
  readonly crossfaderReverse = signal(false);
  readonly limiter = signal(true);
  readonly mainGain = signal(1);
  /** -1 = the master on the left only, 1 = on the right only. */
  readonly balance = signal(0);
  /** First output (0-based) of the booth the DJ wants, and of the booth in use. */
  readonly booth = signal<number | null>(null);
  readonly boothInUse = signal<number | null>(null);
  readonly boothGain = signal(1);
  readonly mic = signal<LiveState>(LIVE_OFF, { equal: same });
  readonly aux = signal<LiveState>(LIVE_OFF, { equal: same });
  /** While the microphone is on, the music ducks by `talkoverDepthDb`. */
  readonly talkover = signal(false);
  readonly talkoverDepthDb = signal(-18);
  /** The inputs of the running device the turntables leave free (0-based). */
  readonly freeInputs = signal<number[]>([], { equal: same });
  /** What was recorded last: a recording started without asking takes it again. */
  readonly recordSource = signal<RecordSource | null>(null, { equal: same });
  /** The software mixer is in use. */
  readonly internal = computed(() => (this.modeInUse() ?? this.mode()) === 'internal');
  /** External mode asked for, but the device has too few outputs: the software mixer runs. */
  readonly fallback = computed(() => this.mode() === 'external' && this.modeInUse() === 'internal');

  constructor() {
    // the routing depends on the device the audio runs on
    effect(() => {
      if (this.settings.info()) void this.followDevice();
    });
    // what the backend changed
    effect(() => {
      const controls = this.engine.controls();
      if (controls) untracked(() => this.apply(controls.mixer));
    });
  }

  /** The outputs of `deck` to an external mixer, counting from 1 (`3-4`): its own pair, or
   * that of the deck it is layered over when the device has no pair for each deck. */
  outputsOf(deck: number): string {
    const decks = this.engine.deckIndexes().length;
    const pair = (this.outputChannels() ?? 0) >= 2 * decks ? deck : deck % 2;
    return `${pair * 2 + 1}-${pair * 2 + 2}`;
  }

  private async followDevice() {
    try {
      this.applyRouting(await invoke<MixerState>('mixer_state'));
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }

  private applyRouting(state: MixerState) {
    // of two decks layered on a turntable, one follows it on this device, and both read its record
    state.decks.forEach(({ vinylEnabled, timecode }, deck) => {
      const shown = this.engine.decks()[deck];
      if (shown.vinylEnabled !== vinylEnabled || shown.timecode !== timecode)
        this.engine.patchDeck(deck, { vinylEnabled, timecode });
    });
    this.modeInUse.set(state.modeInUse);
    this.headphones.set(state.headphones);
    this.headphonesInUse.set(state.headphonesInUse);
    this.outputChannels.set(state.outputChannels);
    this.booth.set(state.booth);
    this.boothInUse.set(state.boothInUse);
    this.mic.set(state.mic);
    this.aux.set(state.aux);
    this.freeInputs.set(state.freeInputs);
  }

  apply(state: MixerState) {
    this.crossfader.set(state.crossfader);
    this.mode.set(state.mode);
    this.applyRouting(state);
    this.cueMix.set(state.cueMix);
    this.splitCue.set(state.splitCue);
    this.headphoneGain.set(state.headphoneGain);
    this.crossfaderCurve.set(state.crossfaderCurve);
    this.crossfaderReverse.set(state.crossfaderReverse);
    this.limiter.set(state.limiter);
    this.mainGain.set(state.mainGain);
    this.balance.set(state.balance);
    this.boothGain.set(state.boothGain);
    this.talkover.set(state.talkover);
    this.talkoverDepthDb.set(state.talkoverDepthDb);
    this.recordSource.set(state.recordSource);
  }

  async setMode(mode: MixMode) {
    this.mode.set(mode);
    try {
      this.modeInUse.set(await invoke<MixMode | null>('set_mix_mode', { mode }));
      // the headphones too: with an external mixer they need a pair of their own
      await this.followDevice();
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }

  async setHeadphones(output: HeadphonesOutput) {
    this.headphones.set(output);
    try {
      this.headphonesInUse.set(await invoke<number | null>('set_headphones_output', { output }));
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }

  async setBooth(first: number | null) {
    this.booth.set(first);
    try {
      this.boothInUse.set(await invoke<number | null>('set_booth_output', { first }));
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }

  setMainGain(gain: number) {
    return this.engine.perform('mixer.main', gain);
  }

  setCrossfaderSide(deck: number, side: CrossfaderSide) {
    return this.engine.press(deck, `crossfader.${side}`);
  }

  setBoothGain(gain: number) {
    return this.engine.perform('mixer.booth', gain);
  }

  /** The master's balance, where its knob stands (0 to 1). */
  setBalance(knob: number) {
    return this.engine.perform('mixer.balance', knob);
  }

  live(source: LiveSource) {
    return source === 'mic' ? this.mic : this.aux;
  }

  /** The inputs `source` comes in on, `null` for none. */
  async setLiveInput(source: LiveSource, input: InputChannels | null) {
    this.patchLive(source, { input, inUse: false });
    try {
      const inUse = await invoke<boolean>('set_live_input', { source, input });
      this.patchLive(source, { inUse });
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }

  /** The level of `source`, where its knob stands (0 to 1). */
  setLiveLevel(source: LiveSource, knob: number) {
    return this.engine.perform(`mixer.${source}`, knob);
  }

  /** `source` heard in the master, or no longer. */
  toggleLiveOn(source: LiveSource) {
    return this.engine.perform(`mixer.${source}.on`, 1);
  }

  /** `source` heard in the headphones, or no longer. */
  toggleLiveCue(source: LiveSource) {
    return this.engine.perform(`mixer.${source}.cue`, 1);
  }

  setTalkover(enabled: boolean) {
    return this.engine.turn('mixer.talkover', enabled);
  }

  setTalkoverDepth(db: number) {
    return this.engine.call('set_talkover_depth', { db });
  }

  private patchLive(source: LiveSource, changes: Partial<LiveState>) {
    this.live(source).update((state) => ({ ...state, ...changes }));
  }

  setVolume(deck: number, gain: number) {
    return this.engine.perform(`deck${deck + 1}.volume`, gain);
  }

  setDeckCue(deck: number, enabled: boolean) {
    return this.engine.turn(`deck${deck + 1}.headphones`, enabled);
  }

  /** The crossfader from -1 (left) to 1 (right). */
  setCrossfader(position: number) {
    return this.engine.perform('mixer.crossfader', (position + 1) / 2);
  }

  setCueMix(mix: number) {
    return this.engine.perform('mixer.cueMix', mix);
  }

  setSplitCue(enabled: boolean) {
    return this.engine.turn('mixer.headphones.split', enabled);
  }

  /** The headphones' level, where its knob stands (0 to 1, up to +3.5 dB). */
  setHeadphoneLevel(knob: number) {
    return this.engine.perform('mixer.headphones', knob);
  }

  strip(deck: number): ChannelStrip {
    return this.engine.decks()[deck].strip;
  }

  /** The gain of `deck`, where its knob stands (0 to 1). */
  setTrim(deck: number, knob: number) {
    return this.engine.perform(`deck${deck + 1}.trim`, knob);
  }

  /** `band` of `deck`, where its knob stands (0 to 1). */
  setEq(deck: number, band: Band, knob: number) {
    return this.engine.perform(`deck${deck + 1}.eq.${band}`, knob);
  }

  setKill(deck: number, band: Band, kill: boolean) {
    return this.engine.call('set_eq_kill', { deck, band, kill });
  }

  /** The filter of `deck`, where its knob stands (0 to 1). */
  setFilter(deck: number, knob: number) {
    return this.engine.perform(`deck${deck + 1}.filter`, knob);
  }

  setCrossfaderCurve(curve: CrossfaderCurve) {
    return this.engine.perform('mixer.crossfaderCurve', curve === 'sharp' ? 1 : 0);
  }

  setCrossfaderReverse(enabled: boolean) {
    return this.engine.turn('mixer.crossfader.reverse', enabled);
  }

  setLimiter(enabled: boolean) {
    return this.engine.call('set_limiter', { enabled });
  }
}

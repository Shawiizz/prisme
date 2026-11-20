import { Injectable, computed, inject } from '@angular/core';
import {
  Band,
  ChannelStrip,
  CrossfaderCurve,
  MixerKnobs,
  StripKnobs,
  HeadphonesOutput,
  InputChannels,
  LiveSource,
  LiveState,
  MixMode,
  MixerState,
  RecordSource,
} from '../models/engine';
import { EngineService } from './engine.service';
import { same } from '../same';

/**
 * Where the decks are heard: a hardware mixer, or the software mixer (channel strips,
 * faders, crossfader, the microphone and the aux line, master limiter and balance, booth)
 * and its headphones, as the backend tells them.
 */
@Injectable({ providedIn: 'root' })
export class MixerService {
  private readonly engine = inject(EngineService);

  /** The mixer as the backend last told it: the same while it tells other sections. */
  private readonly told = computed(() => this.engine.told().mixer);

  /** The knobs beside the channels, and the crossfader, as the backend shows them. */
  readonly knobs = computed<MixerKnobs>(() => this.told().knobs, { equal: same });
  /** The mode the user chose. */
  readonly mode = computed<MixMode>(() => this.told().mode);
  /** Where the user wants the headphones: those of the software mixer, or with an external
   * mixer those of the preview. */
  readonly headphones = computed<HeadphonesOutput>(() => this.told().headphones, { equal: same });
  /** First output (0-based) of the headphones in use; `null` when they are not heard. With an
   * external mixer, a pair no deck uses, for the preview alone. */
  readonly headphonesInUse = computed(() => this.told().headphonesInUse);
  /** Headphones: the cue on the left and the master on the right, each in mono. */
  readonly splitCue = computed(() => this.told().splitCue);
  readonly crossfaderCurve = computed<CrossfaderCurve>(() => this.told().crossfaderCurve);
  /** The crossfader's sides swapped: towards the left, the decks on the right come in. */
  readonly crossfaderReverse = computed(() => this.told().crossfaderReverse);
  readonly limiter = computed(() => this.told().limiter);
  /** First output (0-based) of the booth the DJ wants, and of the booth in use. */
  readonly booth = computed(() => this.told().booth);
  readonly boothInUse = computed(() => this.told().boothInUse);
  readonly mic = computed<LiveState>(() => this.told().mic, { equal: same });
  readonly aux = computed<LiveState>(() => this.told().aux, { equal: same });
  /** While the microphone is on, the music ducks by `talkoverDepthDb`. */
  readonly talkover = computed(() => this.told().talkover);
  readonly talkoverDepthDb = computed(() => this.told().talkoverDepthDb);
  /** What was recorded last: a recording started without asking takes it again. */
  readonly recordSource = computed<RecordSource | null>(() => this.told().recordSource, {
    equal: same,
  });
  /** The software mixer is in use. */
  readonly internal = computed(() => this.told().internal);
  /** External mode asked for, but the device has too few outputs (it needs this many): the
   * software mixer runs. */
  readonly fallback = computed(() => this.told().fallback);
  /** The routing as the backend tells it: first outputs of the decks, the pairs the
   * headphones and the booth can take and where they are heard. */
  readonly routing = computed<
    Pick<
      MixerState,
      'deckOutputs' | 'headphonesChoices' | 'headphonesNote' | 'boothChoices' | 'boothNote'
    >
  >(
    () => {
      const { deckOutputs, headphonesChoices, headphonesNote, boothChoices, boothNote } =
        this.told();
      return { deckOutputs, headphonesChoices, headphonesNote, boothChoices, boothNote };
    },
    { equal: same },
  );
  /** What the Rec menu offers. */
  readonly recordOptions = computed<MixerState['recordOptions']>(() => this.told().recordOptions, {
    equal: same,
  });

  /** The outputs of `deck` to an external mixer, counting from 1 (`3-4`). */
  outputsOf(deck: number): string {
    const first = this.routing().deckOutputs[deck];
    return `${first + 1}-${first + 2}`;
  }

  setHeadphones(output: HeadphonesOutput) {
    return this.engine.call('set_headphones_output', { output });
  }

  setBooth(first: number | null) {
    return this.engine.call('set_booth_output', { first });
  }

  /** The master's level, where its knob stands (0 to 1). */
  setMainGain(knob: number) {
    return this.engine.perform('mixer.main', knob);
  }

  setBoothGain(knob: number) {
    return this.engine.perform('mixer.booth', knob);
  }

  /** The master's balance, where its knob stands (0 to 1). */
  setBalance(knob: number) {
    return this.engine.perform('mixer.balance', knob);
  }

  live(source: LiveSource) {
    return source === 'mic' ? this.mic : this.aux;
  }

  /** The inputs `source` comes in on, `null` for none. */
  setLiveInput(source: LiveSource, input: InputChannels | null) {
    return this.engine.call('set_live_input', { source, input });
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

  setVolume(deck: number, gain: number) {
    return this.engine.perform(`deck${deck + 1}.volume`, gain);
  }

  setDeckCue(deck: number, enabled: boolean) {
    return this.engine.turn(`deck${deck + 1}.headphones`, enabled);
  }

  /** The crossfader, where it stands (0 left to 1). */
  setCrossfader(position: number) {
    return this.engine.perform('mixer.crossfader', position);
  }

  setCueMix(mix: number) {
    return this.engine.perform('mixer.cueMix', mix);
  }

  setSplitCue(enabled: boolean) {
    return this.engine.turn('mixer.headphones.split', enabled);
  }

  /** The headphones' level, where its knob stands (0 to 1). */
  setHeadphoneLevel(knob: number) {
    return this.engine.perform('mixer.headphones', knob);
  }

  strip(deck: number): ChannelStrip {
    return this.engine.decks()[deck].strip;
  }

  /** The knobs of `deck`'s channel. */
  stripKnobs(deck: number): StripKnobs {
    return this.engine.decks()[deck].knobs;
  }

  /** The gain of `deck`, where its knob stands (0 to 1). */
  setTrim(deck: number, knob: number) {
    return this.engine.perform(`deck${deck + 1}.trim`, knob);
  }

  /** `band` of `deck`, where its knob stands (0 to 1). */
  setEq(deck: number, band: Band, knob: number) {
    return this.engine.perform(`deck${deck + 1}.eq.${band}`, knob);
  }

  /** `band` of `deck` killed, or back. */
  toggleKill(deck: number, band: Band) {
    return this.engine.perform(`deck${deck + 1}.eq.${band}.kill.toggle`, 1);
  }

  /** The filter of `deck`, where its knob stands (0 to 1). */
  setFilter(deck: number, knob: number) {
    return this.engine.perform(`deck${deck + 1}.filter`, knob);
  }

  setCrossfaderCurve(curve: CrossfaderCurve) {
    return this.engine.perform(`mixer.crossfaderCurve.${curve}`, 1);
  }

  setCrossfaderReverse(enabled: boolean) {
    return this.engine.turn('mixer.crossfader.reverse', enabled);
  }

  setLimiter(enabled: boolean) {
    return this.engine.call('set_limiter', { enabled });
  }
}

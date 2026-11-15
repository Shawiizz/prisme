import {
  BANDS,
  Band,
  CrossfaderSide,
  EFFECT_KINDS,
  EffectKind,
  FX_SLOTS,
  MAX_DECKS,
  HOT_CUES,
  LiveSource,
  SAMPLE_SLOTS,
} from '../models/engine';
import { EFFECT_NAMES } from '../effects';
import { Text, t } from '../i18n/i18n';
import { ROLL_BEATS, formatBeats } from '../../shared/beats';
import { deckLetter } from '../../shared/time';
import { StemPart } from '../stem-parts';

/** The physical control an action suits. */
/** A light shows a state only: nothing drives it (the beat, a clip, the end of a track). */
export type ControlKind = 'button' | 'fader' | 'encoder' | 'light';

/** Something a key or a controller can do. */
export interface Control {
  /** Stable: the controller mappings refer to it. */
  id: string;
  group: string;
  label: string;
  kind: ControlKind;
  /** The deck it plays, for the controls of a deck. */
  deck?: number;
  /** Button: 1 pressed, 0 released. Fader: 0 to 1. Encoder: steps, negative to the left. */
  apply(value: number): void;
  /** Whether the control's light is on. */
  lit?(): boolean;
  /** The state its light shows when it has more than on and off (`loaded`, `playing`): the
   * profiles of the controllers give each its value. */
  state?(): string;
  /** The colour its light shows (a hot cue's), `#rrggbb`: the profiles give the nearest
   * value their controller has. */
  color?(): string | null;
  /** A fader's setting now (0 to 1): a controller's fader takes it over from there. */
  position?(): number;
}

/** What the controls do, independent of how it is done. Positions go from 0 to 1. */
export interface ControlActions {
  togglePlay(deck: number): void;
  playing(deck: number): boolean;
  cue(deck: number, pressed: boolean): void;
  /** What the cue key shows, as on a CDJ: `on` on the cue point, `paused` stopped elsewhere
   * (where a press sets it), `off` while playing. */
  cueState(deck: number): 'on' | 'paused' | 'off';
  sync(deck: number): void;
  synced(deck: number): boolean;
  hotCue(deck: number, slot: number): void;
  hotCueSet(deck: number, slot: number): boolean;
  /** The colour of a hot cue set, `#rrggbb`. */
  hotCueColor(deck: number, slot: number): string | null;
  toggleLoop(deck: number): void;
  looping(deck: number): boolean;
  /** The length in beats of the loop the deck plays, if it plays one. */
  loopBeats(deck: number): number | null;
  scaleLoop(deck: number, factor: number): void;
  loopIn(deck: number): void;
  loopInSet(deck: number): boolean;
  loopOut(deck: number): void;
  beatJump(deck: number, beats: number): void;
  /** A loop of `beats` from where the deck plays, or out of the loop it is in. */
  beatLoop(deck: number, beats: number): void;
  bend(deck: number, direction: -1 | 0 | 1): void;
  toggleKeyLock(deck: number): void;
  keyLocked(deck: number): boolean;
  /** The track's key moved by so many semitones. */
  keyShift(deck: number): number;
  setKeyShift(deck: number, semitones: number): void;
  /** Mutes or plays a part of a separated track. */
  toggleStem(deck: number, part: StemPart): void;
  stemPlaying(deck: number, part: StemPart): boolean;
  /** Sets how loud a part of a separated track plays. */
  setStemLevel(deck: number, part: StemPart, position: number): void;
  stemLevel(deck: number, part: StemPart): number;
  /** Plays a sample, as loud as `gain` (0 to 1) when the pad tells how hard it was hit. */
  triggerSample(deck: number, slot: number, gain?: number): void;
  sampleLoaded(deck: number, slot: number): boolean;
  samplePlaying(deck: number, slot: number): boolean;
  /** The effects of a deck are in a chain of slots, played one after the other. */
  toggleFx(deck: number, slot: number): void;
  setFxOn(deck: number, slot: number, on: boolean): void;
  /** Puts `kind` in the slot and switches it on; pressed again, switches it off. */
  pressFx(deck: number, slot: number, kind: EffectKind): void;
  /** Whether the slot holds `kind` and is on. */
  fxHolds(deck: number, slot: number, kind: EffectKind): boolean;
  /** The effect `step` places after the slot's (before it, negative). */
  cycleFx(deck: number, slot: number, step: number): void;
  /** The chain on the next part of the track it can be on (the whole deck after the last). */
  nextFxPart(deck: number): void;
  fxOn(deck: number, slot: number): boolean;
  setFxMix(deck: number, slot: number, position: number): void;
  /** The slot's main knob (feedback, room size, depth: see each effect). */
  setFxAmount(deck: number, slot: number, position: number): void;
  fxAmountPosition(deck: number, slot: number): number;
  /** Multiplies the slot's period by `factor`. */
  scaleFxBeats(deck: number, slot: number, factor: number): void;
  /** Cuts a band of the deck's EQ, or brings it back. */
  setKill(deck: number, band: Band, kill: boolean): void;
  killed(deck: number, band: Band): boolean;
  /** Rolls a loop of `beats` over the track, the track going on beneath; `null` ends it. */
  roll(deck: number, beats: number | null): void;
  setPitch(deck: number, position: number): void;
  setVolume(deck: number, position: number): void;
  setEq(deck: number, band: Band, position: number): void;
  setFilter(deck: number, position: number): void;
  setTrim(deck: number, position: number): void;
  setCrossfader(position: number): void;
  /** What the headphones play: the cued decks (0) to the master (1). */
  setCueMix(position: number): void;
  cueMixPosition(): number;
  /** The master's balance: left only (0), both sides (0.5), right only (1). */
  setBalance(position: number): void;
  balancePosition(): number;
  setBoothLevel(position: number): void;
  boothLevelPosition(): number;
  setLiveLevel(source: LiveSource, position: number): void;
  liveLevelPosition(source: LiveSource): number;
  /** A live source heard in the master, or not. */
  toggleLive(source: LiveSource): void;
  liveOn(source: LiveSource): boolean;
  toggleLiveCue(source: LiveSource): void;
  liveCued(source: LiveSource): boolean;
  /** The music ducks under the microphone, or not. */
  toggleTalkover(): void;
  talkover(): boolean;
  /** The deck plays its input as it comes instead of its track. */
  togglePassthrough(deck: number): void;
  passthrough(deck: number): boolean;
  /** Where the faders' settings stand (0 to 1). */
  fxMixPosition(deck: number, slot: number): number;
  volumePosition(deck: number): number;
  eqPosition(deck: number, band: Band): number;
  filterPosition(deck: number): number;
  trimPosition(deck: number): number;
  crossfaderPosition(): number;
  toggleHeadphones(deck: number): void;
  inHeadphones(deck: number): boolean;
  toggleSlip(deck: number): void;
  slipping(deck: number): boolean;
  censor(deck: number, held: boolean): void;
  reloop(deck: number): void;
  deleteHotCue(deck: number, slot: number): void;
  resetTempo(deck: number): void;
  browse(steps: number): void;
  loadSelected(deck: number | null): void;
  /** Loads the selected track on `deck` and plays it. */
  loadSelectedAndPlay(deck: number): void;
  /** Previews the selected track in the headphones, or stops it if it is the one previewed. */
  togglePreview(): void;
  previewing(): boolean;
  stopPreview(): void;
  /** Moves the preview by so many steps of a few seconds, back if negative. */
  seekPreview(steps: number): void;
  /** Loads on `deck` the track of the other deck of its pair, where it is. */
  double(deck: number): void;
  removeSelected(): void;
  selectAll(): void;
  clearSelection(): void;
  prepareSelected(): void;
  /** The Auto DJ on, off, or on again after the DJ took over. */
  toggleAutoDj(): void;
  /** Whether the Auto DJ plays the prepare list. */
  autoDjOn(): boolean;
  /** The Auto DJ mixes into the next track at once. */
  autoDjFade(): void;
  /** The Auto DJ leaves the next track out: the one after comes next. */
  autoDjSkip(): void;
  focusSearch(): void;
  toggleQuantize(): void;
  quantized(): boolean;
  /** The tempo faders' next range (±8, ±16, ±50 %), back to the first after the last. */
  cycleTempoRange(): void;
  toggleHelp(): void;
  togglePalette(): void;
  openSettings(): void;
  /** To the main cue, then playing (`play`), stopped (`stop`) or as it was (`goto`). */
  toCue(deck: number, then: 'play' | 'stop' | 'goto'): void;
  /** The main cue where the deck plays. */
  setCueHere(deck: number): void;
  /** To the start of the track, playing or stopped. */
  toStart(deck: number, play: boolean): void;
  eject(deck: number): void;
  /** The leader's tempo and beats, once: the deck stays free. */
  syncOnce(deck: number): void;
  /** The deck leads the others, or back to the leader Prisme picks. */
  toggleLeader(deck: number): void;
  leading(deck: number): boolean;
  /** Runs through the track while held, forwards or back. */
  search(deck: number, direction: -1 | 1, held: boolean): void;
  /** The tempo moved by so many tenths of a percent, and kept. */
  stepTempo(deck: number, steps: number): void;
  /** A smaller bend while held. */
  bendSmall(deck: number, direction: -1 | 0 | 1): void;
  /** Where the deck plays in its track (0 to 1). */
  setPosition(deck: number, position: number): void;
  positionInTrack(deck: number): number;
  /** The key of the deck brought to the key of the deck it mixes with. */
  syncKey(deck: number): void;
  /** Takes the stored loop away. */
  clearLoop(deck: number): void;
  /** Back to the start of the stored loop, playing it, stopped. */
  reloopStopped(deck: number): void;
  /** To the start or the end of the stored loop. */
  toLoopEdge(deck: number, edge: 'in' | 'out'): void;
  /** The loop moved by so many beats. */
  moveLoop(deck: number, beats: number): void;
  loopOutSet(deck: number): boolean;
  loopStored(deck: number): boolean;
  /** Whether a roll of `beats` plays. */
  rolling(deck: number, beats: number): boolean;
  /** A correction of the beat grid: a beat here, earlier or later, faster or slower. */
  gridHere(deck: number): void;
  moveGrid(deck: number, steps: number): void;
  stretchGrid(deck: number, steps: number): void;
  /** One tap of the tempo: the track's tempo once there are enough. */
  tapTempo(deck: number): void;
  /** The deck's first effect on while held. */
  holdFx(deck: number, held: boolean): void;
  /** Slip on while held. */
  holdSlip(deck: number, held: boolean): void;
  /** The filter left where it is but heard or not. */
  toggleFilter(deck: number): void;
  filtering(deck: number): boolean;
  setCrossfaderSide(deck: number, side: CrossfaderSide): void;
  crossfaderSide(deck: number): CrossfaderSide;
  /** Swaps the sides of the crossfader of every deck. */
  reverseCrossfader(): void;
  setCrossfaderCurve(position: number): void;
  crossfaderCurvePosition(): number;
  nextCrossfaderCurve(): void;
  setSamplerGain(deck: number | null, position: number): void;
  samplerGainPosition(deck: number | null): number;
  stopSample(deck: number, slot: number): void;
  ejectSample(deck: number, slot: number): void;
  /** The selected track into the slot. */
  loadSample(deck: number, slot: number): void;
  /** Plays the sample while held. */
  holdSample(deck: number, slot: number, held: boolean): void;
  /** Vinyl control of the deck on or off. */
  toggleVinyl(deck: number): void;
  vinylOn(deck: number): boolean;
  /** Whether a touch of its controller's jog wheel holds the record. */
  toggleJogScratch(deck: number): void;
  jogScratching(deck: number): boolean;
  toggleRecording(): void;
  recording(): boolean;
  setHeadphonesLevel(position: number): void;
  headphonesLevelPosition(): number;
  setMainLevel(position: number): void;
  mainLevelPosition(): number;
  /** What a load key shows: `off` empty, `loaded`, `playing`. */
  deckState(deck: number): 'off' | 'loaded' | 'playing';
  /** The browse knob on the lists of the left, or on the tracks. */
  moveFocus(): void;
  /** The pointer on the lists of the left moved, or the list it points at opened. */
  browseLists(steps: number): void;
  openPointedList(): void;
  sortBy(column: 'title' | 'artist' | 'bpm' | 'key'): void;
  /** The library over the whole window, or back. */
  toggleLibraryOnly(): void;
  libraryOnly(): boolean;
  /** States only lights show: on the beat, past full scale, near the end of the track, at
   * the track's tempo or slower or faster than the deck it mixes with, in phase with it. */
  onTheBeat(deck: number): boolean;
  clipping(deck: number | null): boolean;
  ending(deck: number): 'off' | 'on' | 'soon';
  tempoState(deck: number): 'zero' | 'slower' | 'faster' | 'off';
  inPhase(deck: number): boolean;
}

const JUMP_BEATS = 4;
const CROSSFADER_SIDE: Record<CrossfaderSide, Text> = {
  left: () => t('On the left of the crossfader'),
  center: () => t('Heard whatever the crossfader'),
  right: () => t('On the right of the crossfader'),
};
const EQ_LABEL: Record<Band, Text> = {
  low: () => t('EQ low'),
  mid: () => t('EQ mid'),
  high: () => t('EQ high'),
};
/** A kill acts while held, as the kill keys of a controller do. */
const KILL_LABEL: Record<Band, Text> = {
  low: () => t('Kill the bass while held'),
  mid: () => t('Kill the mids while held'),
  high: () => t('Kill the highs while held'),
};

/** Tracks a page of the library moves by. */
const PAGE = 12;
const SORT_LABEL: Record<'title' | 'artist' | 'bpm' | 'key', Text> = {
  title: () => t('Tracks by title'),
  artist: () => t('Tracks by artist'),
  bpm: () => t('Tracks by tempo'),
  key: () => t('Tracks by key'),
};

/** A button is down at any value but zero: a pad hit softly is hit all the same. */
const down = (value: number) => value > 0;

/** A button acting when pressed. */
function button(id: string, group: string, label: string, run: () => void): Control {
  return {
    id,
    group,
    label,
    kind: 'button',
    apply: (v) => {
      if (down(v)) run();
    },
  };
}

/** A button acting while held. */
function hold(id: string, group: string, label: string, run: (held: boolean) => void): Control {
  return { id, group, label, kind: 'button', apply: (v) => run(down(v)) };
}

function fader(
  id: string,
  group: string,
  label: string,
  run: (v: number) => void,
  position?: () => number,
): Control {
  return { id, group, label, kind: 'fader', apply: run, position };
}

/**
 * A control the backend performs itself, straight from the controller (the jog wheels and
 * tempo faders, `controller_direct.rs`): listed to be mapped, never applied here.
 */
function direct(id: string, group: string, label: string, kind: ControlKind): Control {
  return { id, group, label, kind, apply: () => undefined };
}

/** A control with a light, on when `on` says. */
const lit = (control: Control, on: () => boolean): Control => ({ ...control, lit: on });
/** A light that only shows: on, or the state it names. */
function light(id: string, group: string, label: string, state: () => boolean | string): Control {
  const shown = () => {
    const s = state();
    return typeof s === 'string' ? s : s ? 'on' : 'off';
  };
  return {
    id,
    group,
    label,
    kind: 'light',
    apply: () => undefined,
    lit: () => shown() !== 'off',
    state: shown,
  };
}
const coloured = (control: Control, color: () => string | null): Control => ({ ...control, color });

/** The controls of a slot of a deck's effect chain: `deck1.fx…` for the first, `deck1.fx2…`
 * and `deck1.fx3…` for the next. */
function fxControls(a: ControlActions, deck: number, slot: number, group: string): Control[] {
  const n = slot + 1;
  const id = (name: string) => `deck${deck + 1}.${slot === 0 ? 'fx' : `fx${n}`}${name}`;
  return [
    lit(
      button(id(''), group, t('Effect {n} on / off', { n }), () => a.toggleFx(deck, slot)),
      () => a.fxOn(deck, slot),
    ),
    lit(
      button(id('.on'), group, t('Effect {n} on', { n }), () => a.setFxOn(deck, slot, true)),
      () => a.fxOn(deck, slot),
    ),
    lit(
      button(id('.off'), group, t('Effect {n} off', { n }), () => a.setFxOn(deck, slot, false)),
      () => !a.fxOn(deck, slot),
    ),
    button(id('.next'), group, t('Effect {n}: next effect', { n }), () => a.cycleFx(deck, slot, 1)),
    button(id('.previous'), group, t('Effect {n}: previous effect', { n }), () =>
      a.cycleFx(deck, slot, -1),
    ),
    ...EFFECT_KINDS.map((kind) =>
      lit(
        button(
          id(`.${kind}`),
          group,
          t('Effect {n}: {effect} on / off', { n, effect: EFFECT_NAMES[kind]() }),
          () => a.pressFx(deck, slot, kind),
        ),
        () => a.fxHolds(deck, slot, kind),
      ),
    ),
    fader(
      id('.mix'),
      group,
      t('Effect {n} level', { n }),
      (v) => a.setFxMix(deck, slot, v),
      () => a.fxMixPosition(deck, slot),
    ),
    fader(
      id('.amount'),
      group,
      t('Effect {n} amount', { n }),
      (v) => a.setFxAmount(deck, slot, v),
      () => a.fxAmountPosition(deck, slot),
    ),
    {
      id: id('.beats'),
      group,
      label: t('Effect {n} period (a step halves or doubles it)', { n }),
      kind: 'encoder',
      apply: (steps) => a.scaleFxBeats(deck, slot, 2 ** steps),
    },
  ];
}

function deckControls(a: ControlActions, deck: number): Control[] {
  const group = t('Deck {deck}', { deck: deckLetter(deck) });
  const id = (name: string) => `deck${deck + 1}.${name}`;
  // the parts of a separated track: a button muting each, a fader for its level
  const stems: [string, StemPart, string, string][] = [
    ['vocals', 'vocals', t('Vocals'), t('Vocals level')],
    ['instrumental', 'instrumental', t('Instrumental'), t('Instrumental level')],
    ['drums', 'drums', t('Drums'), t('Drums level')],
    ['bass', 'bass', t('Bass'), t('Bass level')],
    ['melody', 'other', t('Melody (the other instruments)'), t('Melody level')],
  ];
  return [
    lit(
      button(id('play'), group, t('Play / pause'), () => a.togglePlay(deck)),
      () => a.playing(deck),
    ),
    {
      ...hold(id('cue'), group, t('Cue (hold to preview)'), (held) => a.cue(deck, held)),
      lit: () => a.cueState(deck) !== 'off',
      state: () => a.cueState(deck),
    },
    lit(
      button(id('sync'), group, t('Sync'), () => a.sync(deck)),
      () => a.synced(deck),
    ),
    ...Array.from({ length: HOT_CUES }, (_, slot) =>
      coloured(
        lit(
          button(id(`hotcue${slot + 1}`), group, t('Hot cue {n}', { n: slot + 1 }), () =>
            a.hotCue(deck, slot),
          ),
          () => a.hotCueSet(deck, slot),
        ),
        () => a.hotCueColor(deck, slot),
      ),
    ),
    ...Array.from({ length: HOT_CUES }, (_, slot) =>
      button(id(`hotcue${slot + 1}.delete`), group, t('Delete hot cue {n}', { n: slot + 1 }), () =>
        a.deleteHotCue(deck, slot),
      ),
    ),
    direct(id('jog'), group, t('Jog wheel (turn)'), 'encoder'),
    direct(id('jog.touch'), group, t('Jog wheel (touch: scratch)'), 'button'),
    direct(id('jog.search'), group, t('Jog wheel (search through the track)'), 'encoder'),
    lit(
      button(id('loop'), group, t('Loop 4 beats / exit'), () => a.toggleLoop(deck)),
      () => a.looping(deck),
    ),
    lit(
      button(id('loop.in'), group, t('Loop in'), () => a.loopIn(deck)),
      () => a.loopInSet(deck),
    ),
    button(id('loop.out'), group, t('Loop out'), () => a.loopOut(deck)),
    button(id('reloop'), group, t('Reloop'), () => a.reloop(deck)),
    button(id('loop.halve'), group, t('Halve the loop'), () => a.scaleLoop(deck, 0.5)),
    button(id('loop.double'), group, t('Double the loop'), () => a.scaleLoop(deck, 2)),
    button(id('jump.back'), group, t('Jump back {n} beats', { n: JUMP_BEATS }), () =>
      a.beatJump(deck, -JUMP_BEATS),
    ),
    button(id('jump.forward'), group, t('Jump {n} beats ahead', { n: JUMP_BEATS }), () =>
      a.beatJump(deck, JUMP_BEATS),
    ),
    hold(id('bend.down'), group, t('Slow down while held'), (held) => a.bend(deck, held ? -1 : 0)),
    hold(id('bend.up'), group, t('Speed up while held'), (held) => a.bend(deck, held ? 1 : 0)),
    button(id('tempo.reset'), group, t('Tempo back to the track'), () => a.resetTempo(deck)),
    button(id('key.up'), group, t('Key up a semitone'), () =>
      a.setKeyShift(deck, a.keyShift(deck) + 1),
    ),
    button(id('key.down'), group, t('Key down a semitone'), () =>
      a.setKeyShift(deck, a.keyShift(deck) - 1),
    ),
    lit(
      button(id('key.reset'), group, t('Key back to the track'), () => a.setKeyShift(deck, 0)),
      () => a.keyShift(deck) !== 0,
    ),
    lit(
      button(id('slip'), group, t('Slip'), () => a.toggleSlip(deck)),
      () => a.slipping(deck),
    ),
    hold(id('censor'), group, t('Censor while held'), (held) => a.censor(deck, held)),
    lit(
      button(id('keylock'), group, t('Key lock'), () => a.toggleKeyLock(deck)),
      () => a.keyLocked(deck),
    ),
    ...stems.map(([name, part, label]) =>
      lit(
        button(id(name), group, label, () => a.toggleStem(deck, part)),
        () => a.stemPlaying(deck, part),
      ),
    ),
    ...stems.map(([name, part, , level]) =>
      fader(
        id(`${name}.level`),
        group,
        level,
        (v) => a.setStemLevel(deck, part, v),
        () => a.stemLevel(deck, part),
      ),
    ),
    ...Array.from({ length: SAMPLE_SLOTS }, (_, slot) => ({
      ...lit(
        button(id(`sample${slot + 1}`), group, t('Sample {n}', { n: slot + 1 }), () =>
          a.triggerSample(deck, slot),
        ),
        () => a.samplePlaying(deck, slot),
      ),
      state: () => {
        if (a.samplePlaying(deck, slot)) return 'playing';
        return a.sampleLoaded(deck, slot) ? 'loaded' : 'off';
      },
    })),
    ...Array.from({ length: FX_SLOTS }, (_, slot) => fxControls(a, deck, slot, group)).flat(),
    button(id('fx.part'), group, t('Effects on the next part'), () => a.nextFxPart(deck)),
    ...ROLL_BEATS.map((beats, i) =>
      hold(
        id(`roll${i + 1}`),
        group,
        t('Roll {beats} while held', { beats: formatBeats(beats) }),
        (held) => a.roll(deck, held ? beats : null),
      ),
    ),
    direct(id('pitch'), group, t('Tempo'), 'fader'),
    fader(
      id('volume'),
      group,
      t('Volume'),
      (v) => a.setVolume(deck, v),
      () => a.volumePosition(deck),
    ),
    fader(
      id('trim'),
      group,
      t('Gain'),
      (v) => a.setTrim(deck, v),
      () => a.trimPosition(deck),
    ),
    ...BANDS.map((band) =>
      fader(
        id(`eq.${band}`),
        group,
        EQ_LABEL[band](),
        (v) => a.setEq(deck, band, v),
        () => a.eqPosition(deck, band),
      ),
    ),
    ...BANDS.map((band) =>
      lit(
        hold(id(`eq.${band}.kill`), group, KILL_LABEL[band](), (held) =>
          a.setKill(deck, band, held),
        ),
        () => a.killed(deck, band),
      ),
    ),
    fader(
      id('filter'),
      group,
      t('Filter'),
      (v) => a.setFilter(deck, v),
      () => a.filterPosition(deck),
    ),
    lit(
      button(id('headphones'), group, t('Headphones (pre-listen)'), () => a.toggleHeadphones(deck)),
      () => a.inHeadphones(deck),
    ),
    lit(
      button(id('passthrough'), group, t('Passthrough: the deck plays its input'), () =>
        a.togglePassthrough(deck),
      ),
      () => a.passthrough(deck),
    ),
    ...moreDeckControls(a, deck, group, id),
  ];
}

/** The deck's other uses: where its controllers have keys for them. */
function moreDeckControls(
  a: ControlActions,
  deck: number,
  group: string,
  id: (name: string) => string,
): Control[] {
  const encoder = (name: string, label: string, apply: (steps: number) => void): Control => ({
    id: id(name),
    group,
    label,
    kind: 'encoder',
    apply,
  });
  return [
    button(id('cue.play'), group, t('To the cue and play'), () => a.toCue(deck, 'play')),
    button(id('cue.stop'), group, t('To the cue, stopped'), () => a.toCue(deck, 'stop')),
    button(id('cue.goto'), group, t('To the cue'), () => a.toCue(deck, 'goto')),
    button(id('cue.set'), group, t('The cue here'), () => a.setCueHere(deck)),
    button(id('start'), group, t('To the start, stopped'), () => a.toStart(deck, false)),
    button(id('start.play'), group, t('To the start and play'), () => a.toStart(deck, true)),
    button(id('eject'), group, t('Eject the track'), () => a.eject(deck)),
    button(id('sync.once'), group, t('Match the tempo and beats once'), () => a.syncOnce(deck)),
    lit(
      button(id('sync.leader'), group, t('Lead the other decks'), () => a.toggleLeader(deck)),
      () => a.leading(deck),
    ),
    hold(id('search.back'), group, t('Run back through the track while held'), (held) =>
      a.search(deck, -1, held),
    ),
    hold(id('search.forward'), group, t('Run through the track while held'), (held) =>
      a.search(deck, 1, held),
    ),
    button(id('tempo.up'), group, t('Tempo up 0.1 %'), () => a.stepTempo(deck, 1)),
    button(id('tempo.down'), group, t('Tempo down 0.1 %'), () => a.stepTempo(deck, -1)),
    encoder('tempo', t('Tempo, 0.1 % a step'), (steps) => a.stepTempo(deck, steps)),
    hold(id('bend.down.small'), group, t('Slow down a little while held'), (held) =>
      a.bendSmall(deck, held ? -1 : 0),
    ),
    hold(id('bend.up.small'), group, t('Speed up a little while held'), (held) =>
      a.bendSmall(deck, held ? 1 : 0),
    ),
    fader(
      id('position'),
      group,
      t('Place in the track (a touch strip)'),
      (v) => a.setPosition(deck, v),
      () => a.positionInTrack(deck),
    ),
    button(id('key.sync'), group, t('Key matched to the other deck'), () => a.syncKey(deck)),
    fader(
      id('key'),
      group,
      t('Key, from 12 semitones down to 12 up'),
      (v) => a.setKeyShift(deck, Math.round(v * 24 - 12)),
      () => (a.keyShift(deck) + 12) / 24,
    ),
    button(id('loop.clear'), group, t('Forget the loop'), () => a.clearLoop(deck)),
    lit(
      button(id('reloop.stop'), group, t('Back to the loop, stopped'), () => a.reloopStopped(deck)),
      () => a.loopStored(deck),
    ),
    button(id('loop.in.goto'), group, t('To the start of the loop'), () =>
      a.toLoopEdge(deck, 'in'),
    ),
    button(id('loop.out.goto'), group, t('To the end of the loop'), () =>
      a.toLoopEdge(deck, 'out'),
    ),
    button(id('loop.move.back'), group, t('Loop a beat back'), () => a.moveLoop(deck, -1)),
    button(id('loop.move.forward'), group, t('Loop a beat forward'), () => a.moveLoop(deck, 1)),
    encoder('loop.move', t('Loop moved, a beat a step'), (steps) => a.moveLoop(deck, steps)),
    button(id('grid.here'), group, t('A beat of the grid here'), () => a.gridHere(deck)),
    button(id('grid.earlier'), group, t('Beat grid earlier'), () => a.moveGrid(deck, -1)),
    button(id('grid.later'), group, t('Beat grid later'), () => a.moveGrid(deck, 1)),
    button(id('grid.faster'), group, t('Beat grid faster'), () => a.stretchGrid(deck, 1)),
    button(id('grid.slower'), group, t('Beat grid slower'), () => a.stretchGrid(deck, -1)),
    encoder('grid.move', t('Beat grid moved, a few milliseconds a step'), (steps) =>
      a.moveGrid(deck, steps),
    ),
    button(id('tap'), group, t('Tap the tempo'), () => a.tapTempo(deck)),
    lit(
      hold(id('fx.held'), group, t('Effect 1 on while held'), (held) => a.holdFx(deck, held)),
      () => a.fxOn(deck, 0),
    ),
    lit(
      hold(id('slip.held'), group, t('Slip while held'), (held) => a.holdSlip(deck, held)),
      () => a.slipping(deck),
    ),
    lit(
      button(id('filter.toggle'), group, t('Filter heard or not'), () => a.toggleFilter(deck)),
      () => a.filtering(deck),
    ),
    ...(['left', 'center', 'right'] as const).map((side) =>
      lit(
        button(id(`crossfader.${side}`), group, CROSSFADER_SIDE[side](), () =>
          a.setCrossfaderSide(deck, side),
        ),
        () => a.crossfaderSide(deck) === side,
      ),
    ),
    fader(
      id('sampler.gain'),
      group,
      t('Sampler level'),
      (v) => a.setSamplerGain(deck, v),
      () => a.samplerGainPosition(deck),
    ),
    ...Array.from({ length: SAMPLE_SLOTS }, (_, slot) => [
      button(id(`sample${slot + 1}.stop`), group, t('Stop sample {n}', { n: slot + 1 }), () =>
        a.stopSample(deck, slot),
      ),
      button(
        id(`sample${slot + 1}.eject`),
        group,
        t('Empty sample slot {n}', { n: slot + 1 }),
        () => a.ejectSample(deck, slot),
      ),
      button(
        id(`sample${slot + 1}.load`),
        group,
        t('The selected track into sample slot {n}', { n: slot + 1 }),
        () => a.loadSample(deck, slot),
      ),
      lit(
        hold(
          id(`sample${slot + 1}.held`),
          group,
          t('Sample {n} while held', { n: slot + 1 }),
          (held) => a.holdSample(deck, slot, held),
        ),
        () => a.samplePlaying(deck, slot),
      ),
    ]).flat(),
    lit(
      button(id('vinyl'), group, t('Vinyl control on / off'), () => a.toggleVinyl(deck)),
      () => a.vinylOn(deck),
    ),
    lit(
      button(id('jog.scratch'), group, t('Jog wheel: a touch scratches, or only nudges'), () =>
        a.toggleJogScratch(deck),
      ),
      () => a.jogScratching(deck),
    ),
    light(id('beat'), group, t('Light: on the beat'), () => a.onTheBeat(deck)),
    light(id('clip'), group, t('Light: the level past full scale'), () => a.clipping(deck)),
    light(id('ending'), group, t('Light: the end of the track near (soon: 10 s)'), () =>
      a.ending(deck),
    ),
    light(
      id('tempo.zero'),
      group,
      t("Light: at the track's tempo"),
      () => a.tempoState(deck) === 'zero',
    ),
    light(
      id('tempo.slower'),
      group,
      t('Light: slower than the deck it mixes with'),
      () => a.tempoState(deck) === 'slower',
    ),
    light(
      id('tempo.faster'),
      group,
      t('Light: faster than the deck it mixes with'),
      () => a.tempoState(deck) === 'faster',
    ),
    light(id('phase'), group, t('Light: in phase with the deck it mixes with'), () =>
      a.inPhase(deck),
    ),
  ];
}

/** The microphone and the aux line: a level each, a key to open it, one to hear it in the
 * headphones. */
function liveControls(a: ControlActions): Control[] {
  const group = t('Mixer');
  const sources: [LiveSource, string, string, string][] = [
    ['mic', t('Microphone level'), t('Microphone on / off'), t('Microphone in the headphones')],
    ['aux', t('Aux line level'), t('Aux line on / off'), t('Aux line in the headphones')],
  ];
  return sources.flatMap(([source, level, on, cue]) => [
    fader(
      `mixer.${source}`,
      group,
      level,
      (v) => a.setLiveLevel(source, v),
      () => a.liveLevelPosition(source),
    ),
    {
      ...button(`mixer.${source}.on`, group, on, () => a.toggleLive(source)),
      lit: () => a.liveOn(source),
    },
    {
      ...button(`mixer.${source}.cue`, group, cue, () => a.toggleLiveCue(source)),
      lit: () => a.liveCued(source),
    },
  ]);
}

/**
 * The controls a controller's profile names by their number rather than one by one: a jump
 * of `deck1.jump.-8` beats (back), a loop of `deck1.loop.0.5` beats, the key moved
 * `deck1.key.+2` semitones, a sample played as hard as its pad is hit
 * (`deck1.sample3.velocity`). `undefined` for an id of another shape.
 */
export function measuredControl(a: ControlActions, id: string): Control | undefined {
  const velocity = /^deck(\d)\.sample(\d+)\.velocity$/.exec(id);
  if (velocity) {
    const [deck, slot] = [Number(velocity[1]) - 1, Number(velocity[2]) - 1];
    if (deck < 0 || deck >= MAX_DECKS || slot < 0 || slot >= SAMPLE_SLOTS) return undefined;
    const group = t('Deck {deck}', { deck: deckLetter(deck) });
    const label = t('Sample {n} (as hard as the pad is hit)', { n: slot + 1 });
    return {
      ...button(id, group, label, () => undefined),
      apply: (v) => {
        if (v > 0) a.triggerSample(deck, slot, v);
      },
      lit: () => a.samplePlaying(deck, slot),
      deck,
    };
  }
  const [, d, kind, amount] =
    /^deck(\d)\.(jump|loop|key|roll)\.([+-]?\d+(?:\.\d+)?)$/.exec(id) ?? [];
  const deck = Number(d) - 1;
  const beats = Number(amount);
  if (kind === undefined || deck < 0 || deck >= MAX_DECKS) return undefined;
  const group = t('Deck {deck}', { deck: deckLetter(deck) });
  if (kind === 'key') {
    if (!Number.isInteger(beats)) return undefined;
    return {
      ...button(id, group, t('Key moved {n} semitones', { n: beats }), () =>
        a.setKeyShift(deck, beats),
      ),
      lit: () => a.keyShift(deck) === beats,
      deck,
    };
  }
  if (beats === 0) return undefined;
  if (kind === 'jump') {
    return {
      ...button(id, group, t('Jump {n} beats', { n: beats }), () => a.beatJump(deck, beats)),
      deck,
    };
  }
  if (beats < 0) return undefined;
  if (kind === 'roll') {
    return {
      ...hold(id, group, t('Roll {n} beats while held', { n: beats }), (held) =>
        a.roll(deck, held ? beats : null),
      ),
      lit: () => a.rolling(deck, beats),
      deck,
    };
  }
  return {
    ...button(id, group, t('Loop {n} beats', { n: beats }), () => a.beatLoop(deck, beats)),
    lit: () => {
      const playing = a.loopBeats(deck);
      return playing !== null && Math.abs(playing - beats) < 0.01;
    },
    deck,
  };
}

export function buildControls(a: ControlActions): Control[] {
  const library = t('Library');
  return [
    ...Array.from({ length: MAX_DECKS }, (_, deck) =>
      deckControls(a, deck).map((control) => ({ ...control, deck })),
    ).flat(),
    fader(
      'mixer.crossfader',
      t('Mixer'),
      t('Crossfader'),
      (v) => a.setCrossfader(v),
      () => a.crossfaderPosition(),
    ),
    fader(
      'mixer.cueMix',
      t('Mixer'),
      t('Headphones: cued decks or master'),
      (v) => a.setCueMix(v),
      () => a.cueMixPosition(),
    ),
    fader(
      'mixer.balance',
      t('Mixer'),
      t('Master balance'),
      (v) => a.setBalance(v),
      () => a.balancePosition(),
    ),
    fader(
      'mixer.booth',
      t('Mixer'),
      t('Booth level'),
      (v) => a.setBoothLevel(v),
      () => a.boothLevelPosition(),
    ),
    ...liveControls(a),
    {
      ...button(
        'mixer.talkover',
        t('Mixer'),
        t('Talkover: the music ducks under the microphone'),
        () => a.toggleTalkover(),
      ),
      lit: () => a.talkover(),
    },
    direct('shift', t('Controller'), t('Shift (second use of the controls)'), 'button'),
    {
      id: 'library.browse',
      group: library,
      label: t('Browse'),
      kind: 'encoder',
      apply: (steps) => a.browse(steps),
    },
    button('library.previous', library, t('Previous track'), () => a.browse(-1)),
    button('library.next', library, t('Next track'), () => a.browse(1)),
    ...Array.from({ length: MAX_DECKS }, (_, deck) => ({
      ...button(
        `library.load${deck + 1}`,
        library,
        t('Load on deck {deck}', { deck: deckLetter(deck) }),
        () => a.loadSelected(deck),
      ),
      lit: () => a.deckState(deck) !== 'off',
      state: () => a.deckState(deck),
      deck,
    })),
    button('library.load', library, t('Load on a free deck'), () => a.loadSelected(null)),
    ...Array.from({ length: MAX_DECKS }, (_, deck) => ({
      ...button(
        `library.load${deck + 1}.play`,
        library,
        t('Load on deck {deck} and play', { deck: deckLetter(deck) }),
        () => a.loadSelectedAndPlay(deck),
      ),
      deck,
    })),
    {
      ...button('library.preview', library, t('Preview in the headphones / stop'), () =>
        a.togglePreview(),
      ),
      lit: () => a.previewing(),
    },
    button('library.preview.stop', library, t('Stop the preview'), () => a.stopPreview()),
    {
      id: 'library.preview.seek',
      group: library,
      label: t('Move through the preview'),
      kind: 'encoder',
      apply: (steps) => a.seekPreview(steps),
    },
    ...Array.from({ length: MAX_DECKS }, (_, deck) => ({
      ...button(
        `library.double${deck + 1}`,
        library,
        t('Instant double on deck {deck}', { deck: deckLetter(deck) }),
        () => a.double(deck),
      ),
      deck,
    })),
    button('library.remove', library, t('Remove from the crate or prepare list'), () =>
      a.removeSelected(),
    ),
    button('library.prepare', library, t('Add to the prepare list'), () => a.prepareSelected()),
    button('library.selectAll', library, t('Select all the tracks'), () => a.selectAll()),
    button('library.clearSelection', library, t('Clear the selection'), () => a.clearSelection()),
    button('library.search', library, t('Search'), () => a.focusSearch()),
    button('library.focus', library, t('Browse the lists or the tracks'), () => a.moveFocus()),
    {
      id: 'library.lists',
      group: library,
      label: t('Browse the lists (prepare, crates)'),
      kind: 'encoder',
      apply: (steps) => a.browseLists(steps),
    },
    button('library.list.open', library, t('Open the list pointed at'), () => a.openPointedList()),
    button('library.page.previous', library, t('A page of tracks up'), () => a.browse(-PAGE)),
    button('library.page.next', library, t('A page of tracks down'), () => a.browse(PAGE)),
    ...(['title', 'artist', 'bpm', 'key'] as const).map((column) =>
      button(`library.sort.${column}`, library, SORT_LABEL[column](), () => a.sortBy(column)),
    ),
    {
      ...button('general.library', t('General'), t('The library over the whole window'), () =>
        a.toggleLibraryOnly(),
      ),
      lit: () => a.libraryOnly(),
    },
    fader(
      'mixer.headphones',
      t('Mixer'),
      t('Headphones level'),
      (v) => a.setHeadphonesLevel(v),
      () => a.headphonesLevelPosition(),
    ),
    fader(
      'mixer.main',
      t('Mixer'),
      t('Master level'),
      (v) => a.setMainLevel(v),
      () => a.mainLevelPosition(),
    ),
    fader(
      'mixer.crossfaderCurve',
      t('Mixer'),
      t('Crossfader curve: smooth, then sharp'),
      (v) => a.setCrossfaderCurve(v),
      () => a.crossfaderCurvePosition(),
    ),
    button('mixer.crossfaderCurve.next', t('Mixer'), t('The other crossfader curve'), () =>
      a.nextCrossfaderCurve(),
    ),
    button('mixer.crossfader.reverse', t('Mixer'), t('Crossfader sides swapped'), () =>
      a.reverseCrossfader(),
    ),
    fader(
      'sampler.gain',
      t('Mixer'),
      t("Level of every deck's sampler"),
      (v) => a.setSamplerGain(null, v),
      () => a.samplerGainPosition(null),
    ),
    light('main.clip', t('Mixer'), t('Light: the master past full scale'), () => a.clipping(null)),
    {
      ...button('general.record', t('General'), t('Record the mix'), () => a.toggleRecording()),
      lit: () => a.recording(),
    },
    {
      ...button('autodj.toggle', t('Auto DJ'), t('Auto DJ on / off'), () => a.toggleAutoDj()),
      lit: () => a.autoDjOn(),
    },
    button('autodj.fade', t('Auto DJ'), t('Mix into the next track now'), () => a.autoDjFade()),
    button('autodj.skip', t('Auto DJ'), t('Skip the next track'), () => a.autoDjSkip()),
    {
      ...button('general.quantize', t('General'), t('Quantize'), () => a.toggleQuantize()),
      lit: () => a.quantized(),
    },
    button('general.tempoRange', t('General'), t('Tempo fader range'), () => a.cycleTempoRange()),
    button('general.help', t('General'), t('Keyboard shortcuts'), () => a.toggleHelp()),
    button('general.palette', t('General'), t('Command palette'), () => a.togglePalette()),
    button('general.settings', t('General'), t('Settings'), () => a.openSettings()),
  ];
}

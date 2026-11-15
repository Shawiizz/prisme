import { BANDS, Band, MAX_DECKS, HOT_CUES, LiveSource, SAMPLE_SLOTS } from '../models/engine';
import { Text, t } from '../i18n/i18n';
import { ROLL_BEATS, formatBeats } from '../../shared/beats';
import { deckLetter } from '../../shared/time';
import { StemPart } from '../stem-parts';

/** The physical control an action suits. */
export type ControlKind = 'button' | 'fader' | 'encoder';

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
  toggleFx(deck: number): void;
  setFxOn(deck: number, on: boolean): void;
  /** The effect `step` places after this one (before it, negative). */
  cycleFx(deck: number, step: number): void;
  /** The effect on the next part of the track it can be on (the whole deck after the last). */
  nextFxPart(deck: number): void;
  fxOn(deck: number): boolean;
  setFxMix(deck: number, position: number): void;
  /** The effect's main knob (feedback, room size, depth: see each effect). */
  setFxAmount(deck: number, position: number): void;
  fxAmountPosition(deck: number): number;
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
  fxMixPosition(deck: number): number;
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
  /** Loads on `deck` the track of the other deck of its pair, where it is. */
  double(deck: number): void;
  removeSelected(): void;
  selectAll(): void;
  clearSelection(): void;
  prepareSelected(): void;
  focusSearch(): void;
  toggleQuantize(): void;
  quantized(): boolean;
  /** The tempo faders' next range (±8, ±16, ±50 %), back to the first after the last. */
  cycleTempoRange(): void;
  toggleHelp(): void;
  togglePalette(): void;
  openSettings(): void;
}

const JUMP_BEATS = 4;
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

function deckControls(a: ControlActions, deck: number): Control[] {
  const group = t('Deck {deck}', { deck: deckLetter(deck) });
  const id = (name: string) => `deck${deck + 1}.${name}`;
  const lit = (control: Control, on: () => boolean): Control => ({ ...control, lit: on });
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
      lit(
        button(id(`hotcue${slot + 1}`), group, t('Hot cue {n}', { n: slot + 1 }), () =>
          a.hotCue(deck, slot),
        ),
        () => a.hotCueSet(deck, slot),
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
    lit(
      button(id('fx'), group, t('Effect on / off'), () => a.toggleFx(deck)),
      () => a.fxOn(deck),
    ),
    lit(
      button(id('fx.on'), group, t('Effect on'), () => a.setFxOn(deck, true)),
      () => a.fxOn(deck),
    ),
    lit(
      button(id('fx.off'), group, t('Effect off'), () => a.setFxOn(deck, false)),
      () => !a.fxOn(deck),
    ),
    button(id('fx.next'), group, t('Next effect'), () => a.cycleFx(deck, 1)),
    button(id('fx.previous'), group, t('Previous effect'), () => a.cycleFx(deck, -1)),
    button(id('fx.part'), group, t('Effect on the next part'), () => a.nextFxPart(deck)),
    fader(
      id('fx.mix'),
      group,
      t('Effect level'),
      (v) => a.setFxMix(deck, v),
      () => a.fxMixPosition(deck),
    ),
    fader(
      id('fx.amount'),
      group,
      t('Effect amount'),
      (v) => a.setFxAmount(deck, v),
      () => a.fxAmountPosition(deck),
    ),
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
  const [, d, kind, amount] = /^deck(\d)\.(jump|loop|key)\.([+-]?\d+(?:\.\d+)?)$/.exec(id) ?? [];
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
      deck,
    })),
    button('library.load', library, t('Load on a free deck'), () => a.loadSelected(null)),
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

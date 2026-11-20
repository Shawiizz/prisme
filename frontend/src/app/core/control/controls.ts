import {
  BANDS,
  Band,
  CrossfaderSide,
  EFFECT_KINDS,
  FX_SLOTS,
  MAX_DECKS,
  HOT_CUES,
  LiveSource,
  SAMPLE_SLOTS,
  SLICES,
} from '../models/engine';
import { EFFECT_NAMES } from '../effects';
import { Text, t } from '../i18n/i18n';
import { CUT_BEATS, ROLL_BEATS, formatBeats } from '../../shared/beats';
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
  /** Whether the control's light is on, for the few the app tells the controllers: the
   * backend lights the others. */
  lit?(): boolean;
  /** A fader's setting now (0 to 1): a controller's fader takes it over from there. */
  position?(): number;
}

/** What the controls do, independent of how it is done. Positions go from 0 to 1. */
export interface ControlActions {
  /** Has the backend perform action `id` given `value`: a key's 1 pressed and 0 let go, a
   * fader's position, or an encoder's steps (`steps`). */
  perform(id: string, value: number, steps: boolean): void;
  /** Mutes or plays a part of a separated track. */
  toggleStem(deck: number, part: StemPart): void;
  sampleLoaded(deck: number, slot: number): boolean;
  samplePlaying(deck: number, slot: number): boolean;
  browse(steps: number): void;
  loadSelected(deck: number | null): void;
  /** Loads the selected track on `deck` and plays it. */
  loadSelectedAndPlay(deck: number): void;
  /** Previews the selected track in the headphones, or stops it if it is the one previewed. */
  togglePreview(): void;
  stopPreview(): void;
  /** Moves the preview by so many steps of a few seconds, back if negative. */
  seekPreview(steps: number): void;
  /** Loads on `deck` the track of the other deck of its pair, where it is. */
  double(deck: number): void;
  removeSelected(): void;
  selectAll(): void;
  clearSelection(): void;
  prepareSelected(): void;
  focusSearch(): void;
  toggleHelp(): void;
  togglePalette(): void;
  openSettings(): void;
  eject(deck: number): void;
  /** Runs through the track while held, forwards or back. */
  search(deck: number, direction: -1 | 1, held: boolean): void;
  /** The key of the deck brought to the key of the deck it mixes with. */
  syncKey(deck: number): void;
  /** Back to the start of the stored loop, playing it, stopped. */
  reloopStopped(deck: number): void;
  /** To the start or the end of the stored loop. */
  toLoopEdge(deck: number, edge: 'in' | 'out'): void;
  /** The loop moved by so many beats. */
  moveLoop(deck: number, beats: number): void;
  /** One tap of the tempo: the track's tempo once there are enough. */
  tapTempo(deck: number): void;
  /** The filter left where it is but heard or not. */
  toggleFilter(deck: number): void;
  ejectSample(deck: number, slot: number): void;
  /** The selected track into the slot. */
  loadSample(deck: number, slot: number): void;
  toggleRecording(): void;
  /** The browse knob on the lists of the left, or on the tracks. */
  moveFocus(): void;
  /** The pointer on the lists of the left moved, or the list it points at opened. */
  browseLists(steps: number): void;
  openPointedList(): void;
  /** The list `steps` after the one open on the left opened (before it, negative). */
  openNextList(steps: number): void;
  /** On the lists, the one pointed at opened; on the tracks, the selected one loaded on the
   * first deck that is not playing. */
  go(): void;
  /** The browse knob on the lists of the left, or on the tracks. */
  setFocus(where: 'lists' | 'tracks'): void;
  sortBy(column: 'title' | 'artist' | 'bpm' | 'key'): void;
  /** The library over the whole window, or back. */
  toggleLibraryOnly(): void;
  libraryOnly(): boolean;
}

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

/** A control the controller reads itself (its shift): listed to be mapped, never applied here. */
function direct(id: string, group: string, label: string): Control {
  return { id, group, label, kind: 'button', apply: () => undefined };
}

/** A control the backend performs, whoever moves it: a controller (straight to the backend),
 * a key, the command palette. */
function performed(
  a: ControlActions,
  id: string,
  group: string,
  label: string,
  kind: ControlKind = 'button',
): Control {
  return { id, group, label, kind, apply: (v) => a.perform(id, v, kind === 'encoder') };
}

/** A light that only shows a state (the backend's): nothing drives it. */
function light(id: string, group: string, label: string): Control {
  return { id, group, label, kind: 'light', apply: () => undefined };
}

/** A switch the backend performs: a key flipping it, and keys putting it on (`<id>.on`) and
 * off (`<id>.off`) whatever it is, for a controller's switch sending its position: never out
 * of step with the app. */
function switched(a: ControlActions, id: string, group: string, label: string): Control[] {
  return [
    performed(a, id, group, label),
    performed(a, `${id}.on`, group, t('{control}: on', { control: label })),
    performed(a, `${id}.off`, group, t('{control}: off', { control: label })),
  ];
}

/** The id of a control of a slot of a deck's effect chain: `deck1.fx…` for the first,
 * `deck1.fx2…` and `deck1.fx3…` for the next. */
export const fxId = (deck: number, slot: number, name = '') =>
  `deck${deck + 1}.${slot === 0 ? 'fx' : `fx${slot + 1}`}${name}`;

/** The controls of a slot of a deck's effect chain. */
function fxControls(a: ControlActions, deck: number, slot: number, group: string): Control[] {
  const n = slot + 1;
  const id = (name: string) => fxId(deck, slot, name);
  return [
    performed(a, id(''), group, t('Effect {n} on / off', { n })),
    performed(a, id('.on'), group, t('Effect {n} on', { n })),
    performed(a, id('.off'), group, t('Effect {n} off', { n })),
    performed(a, id('.next'), group, t('Effect {n}: next effect', { n })),
    performed(a, id('.previous'), group, t('Effect {n}: previous effect', { n })),
    ...EFFECT_KINDS.map((kind) =>
      performed(
        a,
        id(`.${kind}`),
        group,
        t('Effect {n}: {effect} on / off', { n, effect: EFFECT_NAMES[kind]() }),
      ),
    ),
    performed(a, id('.mix'), group, t('Effect {n} level', { n }), 'fader'),
    performed(a, id('.amount'), group, t('Effect {n} amount', { n }), 'fader'),
    performed(
      a,
      id('.param2'),
      group,
      t('Effect {n} second knob (tone, resonance… as the effect has)', { n }),
      'fader',
    ),
    performed(
      a,
      id('.beats'),
      group,
      t('Effect {n} period (a step halves or doubles it)', { n }),
      'encoder',
    ),
    performed(a, id('.period'), group, t('Effect {n} period, along the knob', { n }), 'fader'),
    performed(a, id('.kind'), group, t('Effect {n}: the effect, along the knob', { n }), 'fader'),
    performed(a, id('.focus'), group, t('Effect {n} focused on the effect knobs / let go', { n })),
    performed(a, id('.held'), group, t('Effect {n} on while held', { n })),
  ];
}

const FX_KNOB_LABEL: Text[] = [
  () => t("Effect knob 1: effect 1's amount, or the focused effect's"),
  () => t("Effect knob 2: effect 2's amount, or the focused effect's second knob"),
  () => t("Effect knob 3: effect 3's amount, or the focused effect's period"),
];

/** The controls of a deck's whole effect chain: its slots', the chain on or off, the three
 * amounts together, and three knobs as Mixxx's effect unit has them: with no effect focused,
 * knob K is effect K's amount; with one focused, they are its amount, second knob and period. */
function fxChainControls(a: ControlActions, deck: number, group: string): Control[] {
  const slots = Array.from({ length: FX_SLOTS }, (_, slot) => fxControls(a, deck, slot, group));
  const knob = (k: number) =>
    performed(a, `deck${deck + 1}.fx.knob${k + 1}`, group, FX_KNOB_LABEL[k](), 'fader');
  return [
    ...slots.flat(),
    ...switched(a, `deck${deck + 1}.fx.chain`, group, t('Effects (the whole chain)')),
    ...Array.from({ length: FX_SLOTS }, (_, k) => knob(k)),
    performed(
      a,
      `deck${deck + 1}.fx.super`,
      group,
      t('Effect amounts together (super knob)'),
      'fader',
    ),
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
    performed(a, id('play'), group, t('Play / pause')),
    performed(a, id('cue'), group, t('Cue (hold to preview)')),
    ...switched(a, id('sync'), group, t('Sync')),
    ...Array.from({ length: HOT_CUES }, (_, slot) =>
      performed(a, id(`hotcue${slot + 1}`), group, t('Hot cue {n}', { n: slot + 1 })),
    ),
    ...Array.from({ length: HOT_CUES }, (_, slot) =>
      performed(a, id(`hotcue${slot + 1}.delete`), group, t('Delete hot cue {n}', { n: slot + 1 })),
    ),
    performed(a, id('jog'), group, t('Jog wheel (turn)'), 'encoder'),
    performed(a, id('jog.touch'), group, t('Jog wheel (touch: scratch)')),
    performed(a, id('jog.search'), group, t('Jog wheel (search through the track)'), 'encoder'),
    performed(a, id('loop'), group, t('Loop 4 beats / exit')),
    performed(a, id('loop.in'), group, t('Loop in')),
    performed(a, id('loop.out'), group, t('Loop out')),
    performed(a, id('reloop'), group, t('Reloop')),
    performed(a, id('loop.halve'), group, t('Halve the loop')),
    performed(a, id('loop.double'), group, t('Double the loop')),
    performed(a, id('jump.back'), group, t('Jump back by the size')),
    performed(a, id('jump.forward'), group, t('Jump ahead by the size')),
    performed(a, id('loop.size'), group, t('Loop of the size / exit')),
    performed(a, id('size.halve'), group, t('Halve the size')),
    performed(a, id('size.double'), group, t('Double the size')),
    performed(
      a,
      id('size'),
      group,
      t('Size of the jumps and the loop (a step halves or doubles it)'),
      'encoder',
    ),
    performed(a, id('bend.down'), group, t('Slow down while held')),
    performed(a, id('bend.up'), group, t('Speed up while held')),
    performed(a, id('tempo.reset'), group, t('Tempo back to the track')),
    performed(a, id('tempoRange'), group, t('Tempo fader range: next')),
    performed(a, id('tempoRange.previous'), group, t('Tempo fader range: previous')),
    performed(a, id('key.up'), group, t('Key up a semitone')),
    performed(a, id('key.down'), group, t('Key down a semitone')),
    performed(a, id('key.reset'), group, t('Key back to the track')),
    ...switched(a, id('slip'), group, t('Slip')),
    performed(a, id('censor'), group, t('Censor while held')),
    performed(a, id('brake'), group, t('Brake (stopped: soft start)')),
    performed(a, id('spinback'), group, t('Spinback while held')),
    ...switched(a, id('reverse'), group, t('Reverse')),
    performed(a, id('reverse.held'), group, t('Reverse while held')),
    ...switched(a, id('repeat'), group, t('Repeat the track')),
    ...switched(a, id('faderStart'), group, t('Fader start')),
    ...switched(a, id('keylock'), group, t('Key lock')),
    ...stems.map(([name, part, label]) =>
      button(id(name), group, label, () => a.toggleStem(deck, part)),
    ),
    ...stems.map(([name, part, , level]) =>
      performed(a, id(`${name}.level`), group, level, 'fader'),
    ),
    ...Array.from({ length: SAMPLE_SLOTS }, (_, slot) =>
      performed(a, id(`sample${slot + 1}`), group, t('Sample {n}', { n: slot + 1 })),
    ),
    ...fxChainControls(a, deck, group),
    performed(a, id('fx.part'), group, t('Effects on the next part')),
    ...ROLL_BEATS.map((beats, i) =>
      performed(
        a,
        id(`roll${i + 1}`),
        group,
        t('Roll {beats} while held', { beats: formatBeats(beats) }),
      ),
    ),
    ...Array.from({ length: SLICES }, (_, slice) =>
      performed(a, id(`slice${slice + 1}`), group, t('Slice {n} while held', { n: slice + 1 })),
    ),
    ...CUT_BEATS.map((beats, i) =>
      performed(
        a,
        id(`cut${i + 1}`),
        group,
        t('Fader cut on {beats} while held', { beats: formatBeats(beats) }),
      ),
    ),
    performed(a, id('pitch'), group, t('Tempo'), 'fader'),
    performed(a, id('volume'), group, t('Volume'), 'fader'),
    performed(a, id('trim'), group, t('Gain'), 'fader'),
    ...BANDS.map((band) => performed(a, id(`eq.${band}`), group, EQ_LABEL[band](), 'fader')),
    ...BANDS.map((band) => performed(a, id(`eq.${band}.kill`), group, KILL_LABEL[band]())),
    performed(a, id('filter'), group, t('Filter'), 'fader'),
    ...switched(a, id('headphones'), group, t('Headphones (pre-listen)')),
    ...switched(a, id('mute'), group, t('Mute the channel')),
    performed(a, id('headphones.solo'), group, t('This deck alone in the headphones')),
    ...switched(a, id('passthrough'), group, t('Passthrough: the deck plays its input')),
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
    performed(a, id('cue.play'), group, t('To the cue and play')),
    performed(a, id('cue.stop'), group, t('To the cue, stopped')),
    performed(a, id('cue.goto'), group, t('To the cue')),
    performed(a, id('cue.set'), group, t('The cue here')),
    performed(a, id('start'), group, t('To the start, stopped')),
    performed(a, id('start.play'), group, t('To the start and play')),
    performed(a, id('start.keep'), group, t('To the start')),
    performed(a, id('end'), group, t('To the end')),
    button(id('eject'), group, t('Eject the track'), () => a.eject(deck)),
    performed(a, id('sync.once'), group, t('Match the tempo and beats once')),
    performed(a, id('sync.leader'), group, t('Lead the other decks')),
    hold(id('search.back'), group, t('Run back through the track while held'), (held) =>
      a.search(deck, -1, held),
    ),
    hold(id('search.forward'), group, t('Run through the track while held'), (held) =>
      a.search(deck, 1, held),
    ),
    performed(a, id('tempo.up'), group, t('Tempo up 0.1 %')),
    performed(a, id('tempo.down'), group, t('Tempo down 0.1 %')),
    performed(a, id('tempo'), group, t('Tempo, 0.1 % a step'), 'encoder'),
    performed(a, id('bend.down.small'), group, t('Slow down a little while held')),
    performed(a, id('bend.up.small'), group, t('Speed up a little while held')),
    performed(a, id('position'), group, t('Place in the track (a touch strip)'), 'fader'),
    button(id('key.sync'), group, t('Key matched to the other deck'), () => a.syncKey(deck)),
    performed(a, id('key'), group, t('Key, from 12 semitones down to 12 up'), 'fader'),
    performed(a, id('loop.clear'), group, t('Forget the loop')),
    button(id('reloop.stop'), group, t('Back to the loop, stopped'), () => a.reloopStopped(deck)),
    button(id('loop.in.goto'), group, t('To the start of the loop'), () =>
      a.toLoopEdge(deck, 'in'),
    ),
    button(id('loop.out.goto'), group, t('To the end of the loop'), () =>
      a.toLoopEdge(deck, 'out'),
    ),
    button(id('loop.move.back'), group, t('Loop a beat back'), () => a.moveLoop(deck, -1)),
    button(id('loop.move.forward'), group, t('Loop a beat forward'), () => a.moveLoop(deck, 1)),
    encoder('loop.move', t('Loop moved, a beat a step'), (steps) => a.moveLoop(deck, steps)),
    performed(a, id('grid.here'), group, t('A beat of the grid here')),
    performed(a, id('grid.earlier'), group, t('Beat grid earlier')),
    performed(a, id('grid.later'), group, t('Beat grid later')),
    performed(a, id('grid.faster'), group, t('Beat grid faster')),
    performed(a, id('grid.slower'), group, t('Beat grid slower')),
    performed(
      a,
      id('grid.move'),
      group,
      t('Beat grid moved, a few milliseconds a step'),
      'encoder',
    ),
    button(id('tap'), group, t('Tap the tempo'), () => a.tapTempo(deck)),
    performed(a, id('slip.held'), group, t('Slip while held')),
    button(id('filter.toggle'), group, t('Filter heard or not'), () => a.toggleFilter(deck)),
    ...(['left', 'center', 'right'] as const).map((side) =>
      performed(a, id(`crossfader.${side}`), group, CROSSFADER_SIDE[side]()),
    ),
    performed(a, id('sampler.gain'), group, t('Sampler level'), 'fader'),
    ...Array.from({ length: SAMPLE_SLOTS }, (_, slot) => [
      performed(a, id(`sample${slot + 1}.stop`), group, t('Stop sample {n}', { n: slot + 1 })),
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
      performed(
        a,
        id(`sample${slot + 1}.held`),
        group,
        t('Sample {n} while held', { n: slot + 1 }),
      ),
      button(
        id(`sample${slot + 1}.smart`),
        group,
        t('Sample {n}: load the selected track, play, or stop', { n: slot + 1 }),
        () => {
          if (a.samplePlaying(deck, slot)) a.perform(id(`sample${slot + 1}.stop`), 1, false);
          else if (a.sampleLoaded(deck, slot)) a.perform(id(`sample${slot + 1}`), 1, false);
          else a.loadSample(deck, slot);
        },
      ),
    ]).flat(),
    ...switched(a, id('vinyl'), group, t('Vinyl control')),
    ...switched(a, id('jog.scratch'), group, t('Jog wheel: a touch scratches, or only nudges')),
    light(id('beat'), group, t('Light: on the beat')),
    light(id('clip'), group, t('Light: the level past full scale')),
    light(id('ending'), group, t('Light: the end of the track near (soon: 10 s)')),
    light(id('tempo.zero'), group, t("Light: at the track's tempo")),
    light(id('tempo.slower'), group, t('Light: slower than the deck it mixes with')),
    light(id('tempo.faster'), group, t('Light: faster than the deck it mixes with')),
    light(id('phase'), group, t('Light: in phase with the leader')),
    light(id('phase.ahead'), group, t('Light: ahead of the leader')),
    light(id('phase.behind'), group, t('Light: behind the leader')),
    light(id('loop.any'), group, t('Light: a loop playing')),
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
    performed(a, `mixer.${source}`, group, level, 'fader'),
    performed(a, `mixer.${source}.on`, group, on),
    performed(a, `mixer.${source}.cue`, group, cue),
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
    return { ...performed(a, id, group, label), deck };
  }
  const [, d, kind, amount] =
    /^deck(\d)\.(jump|loop|key|roll)\.([+-]?\d+(?:\.\d+)?)$/.exec(id) ?? [];
  const deck = Number(d) - 1;
  const beats = Number(amount);
  if (kind === undefined || deck < 0 || deck >= MAX_DECKS) return undefined;
  const group = t('Deck {deck}', { deck: deckLetter(deck) });
  if (kind === 'key') {
    if (!Number.isInteger(beats)) return undefined;
    return { ...performed(a, id, group, t('Key moved {n} semitones', { n: beats })), deck };
  }
  if (beats === 0) return undefined;
  if (kind === 'jump')
    return { ...performed(a, id, group, t('Jump {n} beats', { n: beats })), deck };
  if (beats < 0) return undefined;
  const label =
    kind === 'roll'
      ? t('Roll {n} beats while held', { n: beats })
      : t('Loop {n} beats', { n: beats });
  return { ...performed(a, id, group, label), deck };
}

export function buildControls(a: ControlActions): Control[] {
  const library = t('Library');
  return [
    ...Array.from({ length: MAX_DECKS }, (_, deck) =>
      deckControls(a, deck).map((control) => ({ ...control, deck })),
    ).flat(),
    performed(a, 'mixer.crossfader', t('Mixer'), t('Crossfader'), 'fader'),
    performed(a, 'mixer.cueMix', t('Mixer'), t('Headphones: cued decks or master'), 'fader'),
    ...switched(
      a,
      'mixer.headphones.split',
      t('Mixer'),
      t('Split headphones: cue left, master right'),
    ),
    performed(a, 'mixer.balance', t('Mixer'), t('Master balance'), 'fader'),
    performed(a, 'mixer.booth', t('Mixer'), t('Booth level'), 'fader'),
    ...liveControls(a),
    ...switched(
      a,
      'mixer.talkover',
      t('Mixer'),
      t('Talkover: the music ducks under the microphone'),
    ),
    direct('shift', t('Controller'), t('Shift (second use of the controls)')),
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
        `library.load${deck + 1}.play`,
        library,
        t('Load on deck {deck} and play', { deck: deckLetter(deck) }),
        () => a.loadSelectedAndPlay(deck),
      ),
      deck,
    })),
    button('library.preview', library, t('Preview in the headphones / stop'), () =>
      a.togglePreview(),
    ),
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
    button('library.list.next', library, t('Open the next list'), () => a.openNextList(1)),
    button('library.list.previous', library, t('Open the previous list'), () => a.openNextList(-1)),
    button('library.go', library, t('Open the list, or load the track on a stopped deck'), () =>
      a.go(),
    ),
    button('library.left', library, t('To the lists'), () => a.setFocus('lists')),
    button('library.right', library, t('To the tracks'), () => a.setFocus('tracks')),
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
    performed(a, 'mixer.headphones', t('Mixer'), t('Headphones level'), 'fader'),
    performed(a, 'mixer.main', t('Mixer'), t('Master level'), 'fader'),
    performed(
      a,
      'mixer.crossfaderCurve',
      t('Mixer'),
      t('Crossfader curve: smooth, then sharp'),
      'fader',
    ),
    performed(a, 'mixer.crossfaderCurve.next', t('Mixer'), t('The other crossfader curve')),
    ...switched(a, 'mixer.crossfader.reverse', t('Mixer'), t('Crossfader sides swapped')),
    performed(a, 'sampler.gain', t('Mixer'), t("Level of every deck's sampler"), 'fader'),
    light('main.clip', t('Mixer'), t('Light: the master past full scale')),
    button('general.record', t('General'), t('Record the mix'), () => a.toggleRecording()),
    performed(a, 'autodj.toggle', t('Auto DJ'), t('Auto DJ on / off')),
    performed(a, 'autodj.fade', t('Auto DJ'), t('Mix into the next track now')),
    performed(a, 'autodj.skip', t('Auto DJ'), t('Skip the next track')),
    ...switched(a, 'general.quantize', t('General'), t('Quantize')),
    performed(a, 'general.tempoRange', t('General'), t('Range of every tempo fader: next')),
    performed(
      a,
      'general.tempoRange.previous',
      t('General'),
      t('Range of every tempo fader: previous'),
    ),
    button('general.help', t('General'), t('Keyboard shortcuts'), () => a.toggleHelp()),
    button('general.palette', t('General'), t('Command palette'), () => a.togglePalette()),
    button('general.settings', t('General'), t('Settings'), () => a.openSettings()),
  ];
}

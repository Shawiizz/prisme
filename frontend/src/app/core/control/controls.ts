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

/** The physical control an action suits. A light shows a state only: nothing drives it (the
 * beat, a clip, the end of a track). */
export type ControlKind = 'button' | 'fader' | 'encoder' | 'light';

/** Something a key or a controller can do: what names it, for the mappings, the keys, the
 * command palette. The backend performs it, but the few the app performs itself (the library's
 * browsing, its windows: see `ControlService`). */
export interface Control {
  /** Stable: the controller mappings refer to it. */
  id: string;
  group: string;
  label: string;
  /** Button: 1 pressed, 0 released. Fader: 0 to 1. Encoder: steps, negative to the left. */
  kind: ControlKind;
  /** The deck it plays, for the controls of a deck. */
  deck?: number;
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

const SORT_LABEL: Record<'title' | 'artist' | 'bpm' | 'key', Text> = {
  title: () => t('Tracks by title'),
  artist: () => t('Tracks by artist'),
  bpm: () => t('Tracks by tempo'),
  key: () => t('Tracks by key'),
};

function control(id: string, group: string, label: string, kind: ControlKind = 'button'): Control {
  return { id, group, label, kind };
}

/** A switch: a key flipping it, and keys putting it on (`<id>.on`) and off (`<id>.off`)
 * whatever it is, for a controller's switch sending its position: never out of step with the
 * app. */
function switched(id: string, group: string, label: string): Control[] {
  return [
    control(id, group, label),
    control(`${id}.on`, group, t('{control}: on', { control: label })),
    control(`${id}.off`, group, t('{control}: off', { control: label })),
  ];
}

/** The id of a control of a slot of a deck's effect chain: `deck1.fx…` for the first,
 * `deck1.fx2…` and `deck1.fx3…` for the next. */
export const fxId = (deck: number, slot: number, name = '') =>
  `deck${deck + 1}.${slot === 0 ? 'fx' : `fx${slot + 1}`}${name}`;

/** The controls of a slot of a deck's effect chain. */
function fxControls(deck: number, slot: number, group: string): Control[] {
  const n = slot + 1;
  const id = (name: string) => fxId(deck, slot, name);
  return [
    control(id(''), group, t('Effect {n} on / off', { n })),
    control(id('.on'), group, t('Effect {n} on', { n })),
    control(id('.off'), group, t('Effect {n} off', { n })),
    control(id('.next'), group, t('Effect {n}: next effect', { n })),
    control(id('.previous'), group, t('Effect {n}: previous effect', { n })),
    ...EFFECT_KINDS.map((kind) =>
      control(
        id(`.${kind}`),
        group,
        t('Effect {n}: {effect} on / off', { n, effect: EFFECT_NAMES[kind]() }),
      ),
    ),
    control(id('.mix'), group, t('Effect {n} level', { n }), 'fader'),
    control(id('.amount'), group, t('Effect {n} amount', { n }), 'fader'),
    control(
      id('.param2'),
      group,
      t('Effect {n} second knob (tone, resonance… as the effect has)', { n }),
      'fader',
    ),
    control(
      id('.beats'),
      group,
      t('Effect {n} period (a step halves or doubles it)', { n }),
      'encoder',
    ),
    control(id('.period'), group, t('Effect {n} period, along the knob', { n }), 'fader'),
    control(id('.kind'), group, t('Effect {n}: the effect, along the knob', { n }), 'fader'),
    control(id('.focus'), group, t('Effect {n} focused on the effect knobs / let go', { n })),
    control(id('.held'), group, t('Effect {n} on while held', { n })),
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
function fxChainControls(deck: number, group: string): Control[] {
  const slots = Array.from({ length: FX_SLOTS }, (_, slot) => fxControls(deck, slot, group));
  const knob = (k: number) =>
    control(`deck${deck + 1}.fx.knob${k + 1}`, group, FX_KNOB_LABEL[k](), 'fader');
  return [
    ...slots.flat(),
    ...switched(`deck${deck + 1}.fx.chain`, group, t('Effects (the whole chain)')),
    ...Array.from({ length: FX_SLOTS }, (_, k) => knob(k)),
    control(`deck${deck + 1}.fx.super`, group, t('Effect amounts together (super knob)'), 'fader'),
  ];
}

function deckControls(deck: number): Control[] {
  const group = t('Deck {deck}', { deck: deckLetter(deck) });
  const id = (name: string) => `deck${deck + 1}.${name}`;
  // the parts of a separated track: a button muting each, a fader for its level
  const stems: [string, string, string][] = [
    ['vocals', t('Vocals'), t('Vocals level')],
    ['instrumental', t('Instrumental'), t('Instrumental level')],
    ['drums', t('Drums'), t('Drums level')],
    ['bass', t('Bass'), t('Bass level')],
    ['melody', t('Melody (the other instruments)'), t('Melody level')],
  ];
  return [
    control(id('play'), group, t('Play / pause')),
    control(id('cue'), group, t('Cue (hold to preview)')),
    ...switched(id('sync'), group, t('Sync')),
    ...Array.from({ length: HOT_CUES }, (_, slot) =>
      control(id(`hotcue${slot + 1}`), group, t('Hot cue {n}', { n: slot + 1 })),
    ),
    ...Array.from({ length: HOT_CUES }, (_, slot) =>
      control(id(`hotcue${slot + 1}.delete`), group, t('Delete hot cue {n}', { n: slot + 1 })),
    ),
    control(id('jog'), group, t('Jog wheel (turn)'), 'encoder'),
    control(id('jog.touch'), group, t('Jog wheel (touch: scratch)')),
    control(id('jog.search'), group, t('Jog wheel (search through the track)'), 'encoder'),
    control(id('loop'), group, t('Loop 4 beats / exit')),
    control(id('loop.in'), group, t('Loop in')),
    control(id('loop.out'), group, t('Loop out')),
    control(id('reloop'), group, t('Reloop')),
    control(id('loop.halve'), group, t('Halve the loop')),
    control(id('loop.double'), group, t('Double the loop')),
    control(id('jump.back'), group, t('Jump back by the size')),
    control(id('jump.forward'), group, t('Jump ahead by the size')),
    control(id('loop.size'), group, t('Loop of the size / exit')),
    control(id('size.halve'), group, t('Halve the size')),
    control(id('size.double'), group, t('Double the size')),
    control(
      id('size'),
      group,
      t('Size of the jumps and the loop (a step halves or doubles it)'),
      'encoder',
    ),
    control(id('bend.down'), group, t('Slow down while held')),
    control(id('bend.up'), group, t('Speed up while held')),
    control(id('tempo.reset'), group, t('Tempo back to the track')),
    control(id('tempoRange'), group, t('Tempo fader range: next')),
    control(id('tempoRange.previous'), group, t('Tempo fader range: previous')),
    control(id('key.up'), group, t('Key up a semitone')),
    control(id('key.down'), group, t('Key down a semitone')),
    control(id('key.reset'), group, t('Key back to the track')),
    ...switched(id('slip'), group, t('Slip')),
    control(id('censor'), group, t('Censor while held')),
    control(id('brake'), group, t('Brake (stopped: soft start)')),
    control(id('spinback'), group, t('Spinback while held')),
    ...switched(id('reverse'), group, t('Reverse')),
    control(id('reverse.held'), group, t('Reverse while held')),
    ...switched(id('repeat'), group, t('Repeat the track')),
    ...switched(id('faderStart'), group, t('Fader start')),
    ...switched(id('keylock'), group, t('Key lock')),
    ...stems.map(([name, label]) => control(id(name), group, label)),
    ...stems.map(([name, , level]) => control(id(`${name}.level`), group, level, 'fader')),
    ...Array.from({ length: SAMPLE_SLOTS }, (_, slot) =>
      control(id(`sample${slot + 1}`), group, t('Sample {n}', { n: slot + 1 })),
    ),
    ...fxChainControls(deck, group),
    control(id('fx.part'), group, t('Effects on the next part')),
    ...ROLL_BEATS.map((beats, i) =>
      control(
        id(`roll${i + 1}`),
        group,
        t('Roll {beats} while held', { beats: formatBeats(beats) }),
      ),
    ),
    ...Array.from({ length: SLICES }, (_, slice) =>
      control(id(`slice${slice + 1}`), group, t('Slice {n} while held', { n: slice + 1 })),
    ),
    ...CUT_BEATS.map((beats, i) =>
      control(
        id(`cut${i + 1}`),
        group,
        t('Fader cut on {beats} while held', { beats: formatBeats(beats) }),
      ),
    ),
    control(id('pitch'), group, t('Tempo'), 'fader'),
    control(id('volume'), group, t('Volume'), 'fader'),
    control(id('trim'), group, t('Gain'), 'fader'),
    ...BANDS.map((band) => control(id(`eq.${band}`), group, EQ_LABEL[band](), 'fader')),
    ...BANDS.map((band) => control(id(`eq.${band}.kill`), group, KILL_LABEL[band]())),
    control(id('filter'), group, t('Filter'), 'fader'),
    ...switched(id('headphones'), group, t('Headphones (pre-listen)')),
    ...switched(id('mute'), group, t('Mute the channel')),
    control(id('headphones.solo'), group, t('This deck alone in the headphones')),
    ...switched(id('passthrough'), group, t('Passthrough: the deck plays its input')),
    ...moreDeckControls(group, id),
  ];
}

/** The deck's other uses: where its controllers have keys for them. */
function moreDeckControls(group: string, id: (name: string) => string): Control[] {
  return [
    control(id('cue.play'), group, t('To the cue and play')),
    control(id('cue.stop'), group, t('To the cue, stopped')),
    control(id('cue.goto'), group, t('To the cue')),
    control(id('cue.set'), group, t('The cue here')),
    control(id('start'), group, t('To the start, stopped')),
    control(id('start.play'), group, t('To the start and play')),
    control(id('start.keep'), group, t('To the start')),
    control(id('end'), group, t('To the end')),
    control(id('eject'), group, t('Eject the track')),
    control(id('sync.once'), group, t('Match the tempo and beats once')),
    control(id('sync.leader'), group, t('Lead the other decks')),
    control(id('search.back'), group, t('Run back through the track while held')),
    control(id('search.forward'), group, t('Run through the track while held')),
    control(id('tempo.up'), group, t('Tempo up 0.1 %')),
    control(id('tempo.down'), group, t('Tempo down 0.1 %')),
    control(id('tempo'), group, t('Tempo, 0.1 % a step'), 'encoder'),
    control(id('bend.down.small'), group, t('Slow down a little while held')),
    control(id('bend.up.small'), group, t('Speed up a little while held')),
    control(id('position'), group, t('Place in the track (a touch strip)'), 'fader'),
    control(id('key.sync'), group, t('Key matched to the other deck')),
    control(id('key'), group, t('Key, from 12 semitones down to 12 up'), 'fader'),
    control(id('loop.clear'), group, t('Forget the loop')),
    control(id('reloop.stop'), group, t('Back to the loop, stopped')),
    control(id('loop.in.goto'), group, t('To the start of the loop')),
    control(id('loop.out.goto'), group, t('To the end of the loop')),
    control(id('loop.move.back'), group, t('Loop a beat back')),
    control(id('loop.move.forward'), group, t('Loop a beat forward')),
    control(id('loop.move'), group, t('Loop moved, a beat a step'), 'encoder'),
    control(id('grid.here'), group, t('A beat of the grid here')),
    control(id('grid.earlier'), group, t('Beat grid earlier')),
    control(id('grid.later'), group, t('Beat grid later')),
    control(id('grid.faster'), group, t('Beat grid faster')),
    control(id('grid.slower'), group, t('Beat grid slower')),
    control(id('grid.move'), group, t('Beat grid moved, a few milliseconds a step'), 'encoder'),
    control(id('tap'), group, t('Tap the tempo')),
    control(id('slip.held'), group, t('Slip while held')),
    control(id('filter.toggle'), group, t('Filter heard or not')),
    ...(['left', 'center', 'right'] as const).map((side) =>
      control(id(`crossfader.${side}`), group, CROSSFADER_SIDE[side]()),
    ),
    control(id('sampler.gain'), group, t('Sampler level'), 'fader'),
    ...Array.from({ length: SAMPLE_SLOTS }, (_, slot) => [
      control(id(`sample${slot + 1}.stop`), group, t('Stop sample {n}', { n: slot + 1 })),
      control(id(`sample${slot + 1}.eject`), group, t('Empty sample slot {n}', { n: slot + 1 })),
      control(
        id(`sample${slot + 1}.load`),
        group,
        t('The selected track into sample slot {n}', { n: slot + 1 }),
      ),
      control(id(`sample${slot + 1}.held`), group, t('Sample {n} while held', { n: slot + 1 })),
      control(
        id(`sample${slot + 1}.smart`),
        group,
        t('Sample {n}: load the selected track, play, or stop', { n: slot + 1 }),
      ),
    ]).flat(),
    ...switched(id('vinyl'), group, t('Vinyl control')),
    ...switched(id('jog.scratch'), group, t('Jog wheel: a touch scratches, or only nudges')),
    control(id('beat'), group, t('Light: on the beat'), 'light'),
    control(id('clip'), group, t('Light: the level past full scale'), 'light'),
    control(id('ending'), group, t('Light: the end of the track near (soon: 10 s)'), 'light'),
    control(id('tempo.zero'), group, t("Light: at the track's tempo"), 'light'),
    control(id('tempo.slower'), group, t('Light: slower than the deck it mixes with'), 'light'),
    control(id('tempo.faster'), group, t('Light: faster than the deck it mixes with'), 'light'),
    control(id('phase'), group, t('Light: in phase with the leader'), 'light'),
    control(id('phase.ahead'), group, t('Light: ahead of the leader'), 'light'),
    control(id('phase.behind'), group, t('Light: behind the leader'), 'light'),
    control(id('loop.any'), group, t('Light: a loop playing'), 'light'),
  ];
}

/** The microphone and the aux line: a level each, a key to open it, one to hear it in the
 * headphones. */
function liveControls(): Control[] {
  const group = t('Mixer');
  const sources: [LiveSource, string, string, string][] = [
    ['mic', t('Microphone level'), t('Microphone on / off'), t('Microphone in the headphones')],
    ['aux', t('Aux line level'), t('Aux line on / off'), t('Aux line in the headphones')],
  ];
  return sources.flatMap(([source, level, on, cue]) => [
    control(`mixer.${source}`, group, level, 'fader'),
    control(`mixer.${source}.on`, group, on),
    control(`mixer.${source}.cue`, group, cue),
  ]);
}

/**
 * The controls a controller's profile names by their number rather than one by one: a jump
 * of `deck1.jump.-8` beats (back), a loop of `deck1.loop.0.5` beats, the key moved
 * `deck1.key.+2` semitones, a sample played as hard as its pad is hit
 * (`deck1.sample3.velocity`). `undefined` for an id of another shape.
 */
export function measuredControl(id: string): Control | undefined {
  const velocity = /^deck(\d)\.sample(\d+)\.velocity$/.exec(id);
  if (velocity) {
    const [deck, slot] = [Number(velocity[1]) - 1, Number(velocity[2]) - 1];
    if (deck < 0 || deck >= MAX_DECKS || slot < 0 || slot >= SAMPLE_SLOTS) return undefined;
    const group = t('Deck {deck}', { deck: deckLetter(deck) });
    const label = t('Sample {n} (as hard as the pad is hit)', { n: slot + 1 });
    return { ...control(id, group, label), deck };
  }
  const [, d, kind, amount] =
    /^deck(\d)\.(jump|loop|key|roll)\.([+-]?\d+(?:\.\d+)?)$/.exec(id) ?? [];
  const deck = Number(d) - 1;
  const beats = Number(amount);
  if (kind === undefined || deck < 0 || deck >= MAX_DECKS) return undefined;
  const group = t('Deck {deck}', { deck: deckLetter(deck) });
  if (kind === 'key') {
    if (!Number.isInteger(beats)) return undefined;
    return { ...control(id, group, t('Key moved {n} semitones', { n: beats })), deck };
  }
  if (beats === 0) return undefined;
  if (kind === 'jump') return { ...control(id, group, t('Jump {n} beats', { n: beats })), deck };
  if (beats < 0) return undefined;
  const label =
    kind === 'roll'
      ? t('Roll {n} beats while held', { n: beats })
      : t('Loop {n} beats', { n: beats });
  return { ...control(id, group, label), deck };
}

export function buildControls(): Control[] {
  const library = t('Library');
  return [
    ...Array.from({ length: MAX_DECKS }, (_, deck) =>
      deckControls(deck).map((control) => ({ ...control, deck })),
    ).flat(),
    control('mixer.crossfader', t('Mixer'), t('Crossfader'), 'fader'),
    control('mixer.cueMix', t('Mixer'), t('Headphones: cued decks or master'), 'fader'),
    ...switched(
      'mixer.headphones.split',
      t('Mixer'),
      t('Split headphones: cue left, master right'),
    ),
    control('mixer.balance', t('Mixer'), t('Master balance'), 'fader'),
    control('mixer.booth', t('Mixer'), t('Booth level'), 'fader'),
    ...liveControls(),
    ...switched('mixer.talkover', t('Mixer'), t('Talkover: the music ducks under the microphone')),
    control('shift', t('Controller'), t('Shift (second use of the controls)')),
    control('library.browse', library, t('Browse'), 'encoder'),
    control('library.previous', library, t('Previous track')),
    control('library.next', library, t('Next track')),
    ...Array.from({ length: MAX_DECKS }, (_, deck) => ({
      ...control(
        `library.load${deck + 1}`,
        library,
        t('Load on deck {deck}', { deck: deckLetter(deck) }),
      ),
      deck,
    })),
    control('library.load', library, t('Load on a free deck')),
    ...Array.from({ length: MAX_DECKS }, (_, deck) => ({
      ...control(
        `library.load${deck + 1}.play`,
        library,
        t('Load on deck {deck} and play', { deck: deckLetter(deck) }),
      ),
      deck,
    })),
    control('library.preview', library, t('Preview in the headphones / stop')),
    control('library.preview.stop', library, t('Stop the preview')),
    control('library.preview.seek', library, t('Move through the preview'), 'encoder'),
    ...Array.from({ length: MAX_DECKS }, (_, deck) => ({
      ...control(
        `library.double${deck + 1}`,
        library,
        t('Instant double on deck {deck}', { deck: deckLetter(deck) }),
      ),
      deck,
    })),
    control('library.remove', library, t('Remove from the crate or prepare list')),
    control('library.prepare', library, t('Add to the prepare list')),
    control('library.selectAll', library, t('Select all the tracks')),
    control('library.clearSelection', library, t('Clear the selection')),
    control('library.search', library, t('Search')),
    control('library.focus', library, t('Browse the lists or the tracks')),
    control('library.lists', library, t('Browse the lists (prepare, crates)'), 'encoder'),
    control('library.list.open', library, t('Open the list pointed at')),
    control('library.list.next', library, t('Open the next list')),
    control('library.list.previous', library, t('Open the previous list')),
    control('library.go', library, t('Open the list, or load the track on a stopped deck')),
    control('library.left', library, t('To the lists')),
    control('library.right', library, t('To the tracks')),
    control('library.page.previous', library, t('A page of tracks up')),
    control('library.page.next', library, t('A page of tracks down')),
    ...(['title', 'artist', 'bpm', 'key'] as const).map((column) =>
      control(`library.sort.${column}`, library, SORT_LABEL[column]()),
    ),
    control('general.library', t('General'), t('The library over the whole window')),
    control('mixer.headphones', t('Mixer'), t('Headphones level'), 'fader'),
    control('mixer.main', t('Mixer'), t('Master level'), 'fader'),
    control(
      'mixer.crossfaderCurve',
      t('Mixer'),
      t('Crossfader curve: smooth, then sharp'),
      'fader',
    ),
    control('mixer.crossfaderCurve.next', t('Mixer'), t('The other crossfader curve')),
    ...switched('mixer.crossfader.reverse', t('Mixer'), t('Crossfader sides swapped')),
    control('sampler.gain', t('Mixer'), t("Level of every deck's sampler"), 'fader'),
    control('main.clip', t('Mixer'), t('Light: the master past full scale'), 'light'),
    control('general.record', t('General'), t('Record the mix')),
    control('autodj.toggle', t('Auto DJ'), t('Auto DJ on / off')),
    control('autodj.fade', t('Auto DJ'), t('Mix into the next track now')),
    control('autodj.skip', t('Auto DJ'), t('Skip the next track')),
    ...switched('general.quantize', t('General'), t('Quantize')),
    control('general.tempoRange', t('General'), t('Range of every tempo fader: next')),
    control('general.tempoRange.previous', t('General'), t('Range of every tempo fader: previous')),
    control('general.help', t('General'), t('Keyboard shortcuts')),
    control('general.palette', t('General'), t('Command palette')),
    control('general.settings', t('General'), t('Settings')),
  ];
}

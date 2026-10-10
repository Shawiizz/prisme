import { BANDS, Band, DECK_COUNT, HOT_CUES, SAMPLE_SLOTS } from '../models/engine';

/** The physical control an action suits. */
export type ControlKind = 'button' | 'fader' | 'encoder';

/** Something a key or a controller can do. */
export interface Control {
  /** Stable: the controller mappings refer to it. */
  id: string;
  group: string;
  label: string;
  kind: ControlKind;
  /** Button: 1 pressed, 0 released. Fader: 0 to 1. Encoder: steps, negative to the left. */
  apply(value: number): void;
  /** Whether the control's light is on. */
  lit?(): boolean;
}

/** What the controls do, independent of how it is done. Positions go from 0 to 1. */
export interface ControlActions {
  togglePlay(deck: number): void;
  playing(deck: number): boolean;
  cue(deck: number, pressed: boolean): void;
  sync(deck: number): void;
  synced(deck: number): boolean;
  hotCue(deck: number, slot: number): void;
  hotCueSet(deck: number, slot: number): boolean;
  toggleLoop(deck: number): void;
  looping(deck: number): boolean;
  scaleLoop(deck: number, factor: number): void;
  beatJump(deck: number, beats: number): void;
  bend(deck: number, direction: -1 | 0 | 1): void;
  toggleKeyLock(deck: number): void;
  keyLocked(deck: number): boolean;
  triggerSample(deck: number, slot: number): void;
  samplePlaying(deck: number, slot: number): boolean;
  toggleFx(deck: number): void;
  fxOn(deck: number): boolean;
  setFxMix(deck: number, position: number): void;
  setPitch(deck: number, position: number): void;
  setVolume(deck: number, position: number): void;
  setEq(deck: number, band: Band, position: number): void;
  setFilter(deck: number, position: number): void;
  setCrossfader(position: number): void;
  browse(steps: number): void;
  loadSelected(deck: number | null): void;
  removeSelected(): void;
  prepareSelected(): void;
  focusSearch(): void;
  toggleHelp(): void;
}

const JUMP_BEATS = 4;

/** A button acting when pressed. */
function button(id: string, group: string, label: string, run: () => void): Control {
  return {
    id,
    group,
    label,
    kind: 'button',
    apply: (v) => {
      if (v >= 0.5) run();
    },
  };
}

/** A button acting while held. */
function hold(id: string, group: string, label: string, run: (held: boolean) => void): Control {
  return { id, group, label, kind: 'button', apply: (v) => run(v >= 0.5) };
}

function fader(id: string, group: string, label: string, run: (v: number) => void): Control {
  return { id, group, label, kind: 'fader', apply: run };
}

function deckControls(a: ControlActions, deck: number): Control[] {
  const group = `Deck ${deck + 1}`;
  const id = (name: string) => `deck${deck + 1}.${name}`;
  const lit = (control: Control, on: () => boolean): Control => ({ ...control, lit: on });
  return [
    lit(
      button(id('play'), group, 'Play / pause', () => a.togglePlay(deck)),
      () => a.playing(deck),
    ),
    hold(id('cue'), group, 'Cue (hold to preview)', (held) => a.cue(deck, held)),
    lit(
      button(id('sync'), group, 'Sync', () => a.sync(deck)),
      () => a.synced(deck),
    ),
    ...Array.from({ length: HOT_CUES }, (_, slot) =>
      lit(
        button(id(`hotcue${slot + 1}`), group, `Hot cue ${slot + 1}`, () => a.hotCue(deck, slot)),
        () => a.hotCueSet(deck, slot),
      ),
    ),
    lit(
      button(id('loop'), group, 'Loop 4 beats / exit', () => a.toggleLoop(deck)),
      () => a.looping(deck),
    ),
    button(id('loop.halve'), group, 'Halve the loop', () => a.scaleLoop(deck, 0.5)),
    button(id('loop.double'), group, 'Double the loop', () => a.scaleLoop(deck, 2)),
    button(id('jump.back'), group, `Jump back ${JUMP_BEATS} beats`, () =>
      a.beatJump(deck, -JUMP_BEATS),
    ),
    button(id('jump.forward'), group, `Jump ${JUMP_BEATS} beats ahead`, () =>
      a.beatJump(deck, JUMP_BEATS),
    ),
    hold(id('bend.down'), group, 'Slow down while held', (held) => a.bend(deck, held ? -1 : 0)),
    hold(id('bend.up'), group, 'Speed up while held', (held) => a.bend(deck, held ? 1 : 0)),
    lit(
      button(id('keylock'), group, 'Key lock', () => a.toggleKeyLock(deck)),
      () => a.keyLocked(deck),
    ),
    ...Array.from({ length: SAMPLE_SLOTS }, (_, slot) =>
      lit(
        button(id(`sample${slot + 1}`), group, `Sample ${slot + 1}`, () =>
          a.triggerSample(deck, slot),
        ),
        () => a.samplePlaying(deck, slot),
      ),
    ),
    lit(
      button(id('fx'), group, 'Effect on / off', () => a.toggleFx(deck)),
      () => a.fxOn(deck),
    ),
    fader(id('fx.mix'), group, 'Effect level', (v) => a.setFxMix(deck, v)),
    fader(id('pitch'), group, 'Tempo', (v) => a.setPitch(deck, v)),
    fader(id('volume'), group, 'Volume', (v) => a.setVolume(deck, v)),
    ...BANDS.map((band) =>
      fader(id(`eq.${band}`), group, `EQ ${band}`, (v) => a.setEq(deck, band, v)),
    ),
    fader(id('filter'), group, 'Filter', (v) => a.setFilter(deck, v)),
  ];
}

export function buildControls(a: ControlActions): Control[] {
  const library = 'Library';
  return [
    ...Array.from({ length: DECK_COUNT }, (_, deck) => deckControls(a, deck)).flat(),
    fader('mixer.crossfader', 'Mixer', 'Crossfader', (v) => a.setCrossfader(v)),
    {
      id: 'library.browse',
      group: library,
      label: 'Browse',
      kind: 'encoder',
      apply: (steps) => a.browse(steps),
    },
    button('library.previous', library, 'Previous track', () => a.browse(-1)),
    button('library.next', library, 'Next track', () => a.browse(1)),
    ...Array.from({ length: DECK_COUNT }, (_, deck) =>
      button(`library.load${deck + 1}`, library, `Load on deck ${deck + 1}`, () =>
        a.loadSelected(deck),
      ),
    ),
    button('library.load', library, 'Load on a free deck', () => a.loadSelected(null)),
    button('library.remove', library, 'Remove from the crate or prepare list', () =>
      a.removeSelected(),
    ),
    button('library.prepare', library, 'Add to the prepare list', () => a.prepareSelected()),
    button('library.search', library, 'Search', () => a.focusSearch()),
    button('general.help', 'General', 'Keyboard shortcuts', () => a.toggleHelp()),
  ];
}

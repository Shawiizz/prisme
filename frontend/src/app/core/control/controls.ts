import { BANDS, Band, DECK_COUNT, HOT_CUES, SAMPLE_SLOTS } from '../models/engine';
import { t } from '../i18n/i18n';
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
  /** Button: 1 pressed, 0 released. Fader: 0 to 1. Encoder: steps, negative to the left. */
  apply(value: number): void;
  /** Whether the control's light is on. */
  lit?(): boolean;
  /** A fader's setting now (0 to 1): a controller's fader takes it over from there. */
  position?(): number;
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
  loopIn(deck: number): void;
  loopInSet(deck: number): boolean;
  loopOut(deck: number): void;
  beatJump(deck: number, beats: number): void;
  bend(deck: number, direction: -1 | 0 | 1): void;
  toggleKeyLock(deck: number): void;
  keyLocked(deck: number): boolean;
  /** Mutes or plays a part of a separated track. */
  toggleStem(deck: number, part: StemPart): void;
  stemPlaying(deck: number, part: StemPart): boolean;
  /** Sets how loud a part of a separated track plays. */
  setStemLevel(deck: number, part: StemPart, position: number): void;
  stemLevel(deck: number, part: StemPart): number;
  triggerSample(deck: number, slot: number): void;
  samplePlaying(deck: number, slot: number): boolean;
  toggleFx(deck: number): void;
  fxOn(deck: number): boolean;
  setFxMix(deck: number, position: number): void;
  setPitch(deck: number, position: number): void;
  setVolume(deck: number, position: number): void;
  setEq(deck: number, band: Band, position: number): void;
  setFilter(deck: number, position: number): void;
  setTrim(deck: number, position: number): void;
  setCrossfader(position: number): void;
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
  toggleHelp(): void;
  togglePalette(): void;
  openSettings(): void;
}

const JUMP_BEATS = 4;
const EQ_LABEL: Record<Band, string> = {
  low: t('EQ low'),
  mid: t('EQ mid'),
  high: t('EQ high'),
};

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
  return [
    lit(
      button(id('play'), group, t('Play / pause'), () => a.togglePlay(deck)),
      () => a.playing(deck),
    ),
    hold(id('cue'), group, t('Cue (hold to preview)'), (held) => a.cue(deck, held)),
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
    lit(
      button(id('slip'), group, t('Slip'), () => a.toggleSlip(deck)),
      () => a.slipping(deck),
    ),
    hold(id('censor'), group, t('Censor while held'), (held) => a.censor(deck, held)),
    lit(
      button(id('keylock'), group, t('Key lock'), () => a.toggleKeyLock(deck)),
      () => a.keyLocked(deck),
    ),
    lit(
      button(id('vocals'), group, t('Vocals'), () => a.toggleStem(deck, 'vocals')),
      () => a.stemPlaying(deck, 'vocals'),
    ),
    lit(
      button(id('instrumental'), group, t('Instrumental'), () =>
        a.toggleStem(deck, 'instrumental'),
      ),
      () => a.stemPlaying(deck, 'instrumental'),
    ),
    lit(
      button(id('drums'), group, t('Drums'), () => a.toggleStem(deck, 'drums')),
      () => a.stemPlaying(deck, 'drums'),
    ),
    lit(
      button(id('bass'), group, t('Bass'), () => a.toggleStem(deck, 'bass')),
      () => a.stemPlaying(deck, 'bass'),
    ),
    lit(
      button(id('melody'), group, t('Melody (the other instruments)'), () =>
        a.toggleStem(deck, 'other'),
      ),
      () => a.stemPlaying(deck, 'other'),
    ),
    fader(
      id('vocals.level'),
      group,
      t('Vocals level'),
      (v) => a.setStemLevel(deck, 'vocals', v),
      () => a.stemLevel(deck, 'vocals'),
    ),
    fader(
      id('instrumental.level'),
      group,
      t('Instrumental level'),
      (v) => a.setStemLevel(deck, 'instrumental', v),
      () => a.stemLevel(deck, 'instrumental'),
    ),
    fader(
      id('drums.level'),
      group,
      t('Drums level'),
      (v) => a.setStemLevel(deck, 'drums', v),
      () => a.stemLevel(deck, 'drums'),
    ),
    fader(
      id('bass.level'),
      group,
      t('Bass level'),
      (v) => a.setStemLevel(deck, 'bass', v),
      () => a.stemLevel(deck, 'bass'),
    ),
    fader(
      id('melody.level'),
      group,
      t('Melody level'),
      (v) => a.setStemLevel(deck, 'other', v),
      () => a.stemLevel(deck, 'other'),
    ),
    ...Array.from({ length: SAMPLE_SLOTS }, (_, slot) =>
      lit(
        button(id(`sample${slot + 1}`), group, t('Sample {n}', { n: slot + 1 }), () =>
          a.triggerSample(deck, slot),
        ),
        () => a.samplePlaying(deck, slot),
      ),
    ),
    lit(
      button(id('fx'), group, t('Effect on / off'), () => a.toggleFx(deck)),
      () => a.fxOn(deck),
    ),
    fader(
      id('fx.mix'),
      group,
      t('Effect level'),
      (v) => a.setFxMix(deck, v),
      () => a.fxMixPosition(deck),
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
        EQ_LABEL[band],
        (v) => a.setEq(deck, band, v),
        () => a.eqPosition(deck, band),
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
  ];
}

export function buildControls(a: ControlActions): Control[] {
  const library = t('Library');
  return [
    ...Array.from({ length: DECK_COUNT }, (_, deck) => deckControls(a, deck)).flat(),
    fader(
      'mixer.crossfader',
      t('Mixer'),
      t('Crossfader'),
      (v) => a.setCrossfader(v),
      () => a.crossfaderPosition(),
    ),
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
    ...Array.from({ length: DECK_COUNT }, (_, deck) =>
      button(
        `library.load${deck + 1}`,
        library,
        t('Load on deck {deck}', { deck: deckLetter(deck) }),
        () => a.loadSelected(deck),
      ),
    ),
    button('library.load', library, t('Load on a free deck'), () => a.loadSelected(null)),
    ...Array.from({ length: DECK_COUNT }, (_, deck) =>
      button(
        `library.double${deck + 1}`,
        library,
        t('Instant double on deck {deck}', { deck: deckLetter(deck) }),
        () => a.double(deck),
      ),
    ),
    button('library.remove', library, t('Remove from the crate or prepare list'), () =>
      a.removeSelected(),
    ),
    button('library.prepare', library, t('Add to the prepare list'), () => a.prepareSelected()),
    button('library.selectAll', library, t('Select all the tracks'), () => a.selectAll()),
    button('library.clearSelection', library, t('Clear the selection'), () => a.clearSelection()),
    button('library.search', library, t('Search'), () => a.focusSearch()),
    button('general.help', t('General'), t('Keyboard shortcuts'), () => a.toggleHelp()),
    button('general.palette', t('General'), t('Command palette'), () => a.togglePalette()),
    button('general.settings', t('General'), t('Settings'), () => a.openSettings()),
  ];
}

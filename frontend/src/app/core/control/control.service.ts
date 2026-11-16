import { nextTempoRange } from '../models/preferences';
import { computed, Injectable, inject } from '@angular/core';
import { AutoDjService } from '../services/auto-dj.service';
import { EngineService } from '../services/engine.service';
import { FxService } from '../services/fx.service';
import { LibraryService } from '../services/library.service';
import { MixerService } from '../services/mixer.service';
import { PerformanceService } from '../services/performance.service';
import { PreferencesService } from '../services/preferences.service';
import { SamplerService } from '../services/sampler.service';
import { NavigationService } from '../services/navigation.service';
import { RecordingService } from '../services/recording.service';
import { TrackEditService } from '../services/track-edit.service';
import { TapTempo } from '../../shared/tap-tempo';
import { PREVIEW_STEP_S, PreviewService } from '../services/preview.service';
import {
  balanceAmount,
  balanceKnob,
  bandKnob,
  eqGain,
  filterAmount,
  filterKnob,
  liveGain,
  liveKnob,
  MAX_HEADPHONE_GAIN,
  trimGain,
  trimKnob,
} from '../../shared/mixer-scales';
import { BANDS } from '../models/engine';
import { hasPart, partLevel, partPlaying } from '../stem-parts';
import { phaseState } from '../sync';
import { Control, ControlActions, buildControls, measuredControl } from './controls';

const LOOP_BEATS = 4;
/** A search runs this many times as fast as play, in jumps of this often (ms). */
const SEARCH_SPEED = 8;
const SEARCH_EVERY_MS = 50;
/** A tempo step (ratio) and a small bend. */
const TEMPO_STEP = 0.001;
const SMALL_BEND = 0.01;
/** A step of a beat grid moved (s) and stretched (BPM). */
const GRID_STEP_S = 0.005;
const GRID_STEP_BPM = 0.01;
/** A light shows a clip this long after it (ms). */
const CLIP_SHOWN_MS = 1000;
/** The end of a track near (s), and soon (s). */
const ENDING_S = 30;
const ENDING_SOON_S = 10;
/** Two decks this close in their beat (of a beat) are in phase, else one is ahead or behind;
 * a beat lights its first share. */
const IN_PHASE = 0.03;
const BEAT_LIT = 0.25;

/** Every action a key or a controller can do (see `buildControls`). */
@Injectable({ providedIn: 'root' })
export class ControlService {
  private readonly engine = inject(EngineService);
  private readonly performance = inject(PerformanceService);
  private readonly mixer = inject(MixerService);
  private readonly fx = inject(FxService);
  private readonly sampler = inject(SamplerService);
  private readonly library = inject(LibraryService);
  private readonly preferences = inject(PreferencesService);
  private readonly navigation = inject(NavigationService);
  private readonly preview = inject(PreviewService);
  private readonly recordings = inject(RecordingService);
  private readonly trackEdit = inject(TrackEditService);
  /** The searches running, by deck. */
  private readonly searches = new Map<number, ReturnType<typeof setInterval>>();
  /** The roll playing on each deck, in beats. */
  private readonly rolls = new Map<number, number>();
  private readonly taps = new Map<number, TapTempo>();
  /** The filter of each deck set aside, where it stood. */
  private readonly filtersAside = new Map<number, number>();
  /** When each deck's level, and the master's (`-1`), last went past full scale. */
  private readonly clippedAt = new Map<number, number>();
  private readonly autoDj = inject(AutoDjService);

  private readonly actions: ControlActions = {
    togglePlay: (deck) => this.engine.togglePlay(deck),
    playing: (deck) => this.deck(deck)?.playing ?? false,
    cue: (deck, pressed) => (pressed ? this.engine.cueDown(deck) : this.engine.cueUp(deck)),
    cueState: (deck) => {
      if (!this.deck(deck)?.loaded || this.engine.atCue(deck))
        return this.deck(deck)?.loaded ? 'on' : 'off';
      return this.deck(deck)?.playing ? 'off' : 'paused';
    },
    setSync: (deck, on) => this.engine.setSync(deck, on),
    synced: (deck) => this.deck(deck)?.sync.locked ?? false,
    hotCue: (deck, slot) => this.performance.pressHotCue(deck, slot),
    hotCueSet: (deck, slot) => this.performance.hotCue(deck, slot) !== null,
    hotCueColor: (deck, slot) => {
      const cue = this.performance.hotCue(deck, slot);
      return cue ? `#${cue.color.toString(16).padStart(6, '0')}` : null;
    },
    toggleLoop: (deck) =>
      this.deck(deck)?.looping.active
        ? this.performance.exitLoop(deck)
        : this.performance.beatLoop(deck, LOOP_BEATS),
    looping: (deck) => this.deck(deck)?.looping.active ?? false,
    loopBeats: (deck) => {
      const loop = this.deck(deck)?.looping;
      const bpm = this.engine.tracks()[deck]?.tempo?.bpm;
      if (!loop?.active || !bpm || loop.startS === null || loop.endS === null) return null;
      return ((loop.endS - loop.startS) * bpm) / 60;
    },
    scaleLoop: (deck, factor) => this.performance.scaleLoop(deck, factor),
    loopIn: (deck) => this.performance.loopIn(deck),
    loopInSet: (deck) => this.deck(deck)?.looping.loopInS != null,
    loopOut: (deck) => this.performance.loopOut(deck),
    beatJump: (deck, beats) => this.performance.beatJump(deck, beats),
    size: (deck) => this.performance.size(deck),
    scaleSize: (deck, factor) => this.performance.scaleSize(deck, factor),
    sizedLoop: (deck) => this.performance.toggleSizedLoop(deck),
    slice: (deck, index, held) => this.performance.slice(deck, index, held),
    slicePlaying: (deck) => this.performance.slicePlaying(deck),
    cut: (deck, index, held) => this.performance.cut(deck, index, held),
    cutPlaying: (deck) => this.performance.cuts()[deck],
    beatLoop: (deck, beats) => this.performance.beatLoop(deck, beats),
    bend: (deck, direction) => this.engine.pitchBend(deck, direction),
    setKeyLock: (deck, on) => this.engine.setKeyLock(deck, on),
    keyLocked: (deck) => this.engine.decks()[deck].keyLock,
    toggleStem: (deck, stem) => this.engine.toggleStem(deck, stem),
    stemPlaying: (deck, part) => {
      const track = this.engine.tracks()[deck];
      return !!track && hasPart(track.separation, part) && partPlaying(track.stemMix, part);
    },
    setStemLevel: (deck, part, position) => this.engine.setStemLevel(deck, part, position),
    stemLevel: (deck, part) => {
      const track = this.engine.tracks()[deck];
      return track ? partLevel(track.stemMix, part) : 1;
    },
    triggerSample: (deck, slot, velocity) => this.sampler.trigger(deck, slot, velocity),
    sampleLoaded: (deck, slot) => !!this.engine.decks()[deck].samples[slot],
    samplePlaying: (deck, slot) => ((this.deck(deck)?.samplesPlaying ?? 0) & (1 << slot)) !== 0,
    toggleFx: (deck, slot) => this.fx.setEnabled(deck, slot, !this.fx.settings(deck, slot).enabled),
    setFxOn: (deck, slot, on) => this.fx.setEnabled(deck, slot, on),
    pressFx: (deck, slot, kind) => this.fx.press(deck, slot, kind),
    fxHolds: (deck, slot, kind) => {
      const fx = this.fx.settings(deck, slot);
      return fx.enabled && fx.kind === kind;
    },
    cycleFx: (deck, slot, step) => this.fx.cycle(deck, slot, step),
    nextFxPart: (deck) => this.fx.nextPart(deck),
    fxOn: (deck, slot) => this.fx.settings(deck, slot).enabled,
    setFxMix: (deck, slot, position) => this.fx.setMix(deck, slot, position),
    setFxAmount: (deck, slot, position) => this.fx.setAmount(deck, slot, position),
    fxAmountPosition: (deck, slot) => this.fx.settings(deck, slot).amount,
    setFxAmounts: (deck, position) => this.fx.setAmounts(deck, position),
    setFxParam2: (deck, slot, position) => this.fx.setParam2(deck, slot, position),
    fxParam2Position: (deck, slot) => this.fx.settings(deck, slot).param2,
    scaleFxBeats: (deck, slot, factor) => this.fx.scaleBeats(deck, slot, factor),
    setFxBeats: (deck, slot, beats) => this.fx.setBeats(deck, slot, beats),
    fxBeats: (deck, slot) => this.fx.settings(deck, slot).beats,
    setFxKind: (deck, slot, kind) => this.fx.setKind(deck, slot, kind),
    fxKind: (deck, slot) => this.fx.settings(deck, slot).kind,
    fxFocused: (deck) => this.fx.focused(deck),
    toggleFxFocus: (deck, slot) => this.fx.toggleFocus(deck, slot),
    setFxChainOn: (deck, on) => this.fx.setChainEnabled(deck, on),
    fxChainOn: (deck) => this.fx.chain(deck).enabled,
    setKill: (deck, band, kill) => this.mixer.setKill(deck, band, kill),
    killed: (deck, band) => this.mixer.strip(deck).kills[BANDS.indexOf(band)],
    roll: (deck, beats) => {
      if (beats === null) this.rolls.delete(deck);
      else this.rolls.set(deck, beats);
      this.performance.loopRoll(deck, beats);
    },
    setPitch: (deck, position) => {
      const range = this.engine.tempoRange(deck);
      this.engine.setPitch(deck, 1 + (position * 2 - 1) * range);
    },
    setVolume: (deck, position) => this.mixer.setVolume(deck, position),
    setEq: (deck, band, position) => this.mixer.setEq(deck, band, eqGain(position)),
    setFilter: (deck, position) => this.mixer.setFilter(deck, filterAmount(position)),
    setTrim: (deck, position) => this.mixer.setTrim(deck, trimGain(position)),
    setCrossfader: (position) => this.mixer.setCrossfader(position * 2 - 1),
    setCueMix: (position) => this.mixer.setCueMix(position),
    cueMixPosition: () => this.mixer.cueMix(),
    setSplitCue: (on) => this.mixer.setSplitCue(on),
    splitCue: () => this.mixer.splitCue(),
    setBalance: (position) => this.mixer.setBalance(balanceAmount(position)),
    balancePosition: () => balanceKnob(this.mixer.balance()),
    setBoothLevel: (position) => this.mixer.setBoothGain(position),
    boothLevelPosition: () => this.mixer.boothGain(),
    setLiveLevel: (source, position) => this.mixer.setLiveLevel(source, liveGain(position)),
    liveLevelPosition: (source) => liveKnob(this.mixer.live(source)().level),
    toggleLive: (source) => this.mixer.setLiveOn(source, !this.mixer.live(source)().on),
    liveOn: (source) => this.mixer.live(source)().on,
    toggleLiveCue: (source) => this.mixer.setLiveCue(source, !this.mixer.live(source)().cue),
    liveCued: (source) => this.mixer.live(source)().cue,
    setTalkover: (on) => this.mixer.setTalkover(on),
    talkover: () => this.mixer.talkover(),
    setPassthrough: (deck, on) => this.engine.setPassthrough(deck, on),
    passthrough: (deck) => this.engine.decks()[deck].passthrough,
    fxMixPosition: (deck, slot) => this.fx.settings(deck, slot).mix,
    volumePosition: (deck) => this.engine.decks()[deck].volume,
    eqPosition: (deck, band) => bandKnob(this.mixer.strip(deck), band),
    filterPosition: (deck) => filterKnob(this.mixer.strip(deck).filter),
    trimPosition: (deck) => trimKnob(this.mixer.strip(deck).trim),
    crossfaderPosition: () => (this.mixer.crossfader() + 1) / 2,
    setHeadphones: (deck, on) => this.mixer.setDeckCue(deck, on),
    inHeadphones: (deck) => this.engine.decks()[deck].cue,
    soloHeadphones: (deck) => this.engine.soloCue(deck),
    aloneInHeadphones: (deck) =>
      this.engine.deckIndexes().every((d) => this.engine.decks()[d].cue === (d === deck)),
    setSlip: (deck, on) => this.performance.setSlip(deck, on),
    slipping: (deck) => this.deck(deck)?.slipMode ?? false,
    censor: (deck, held) => this.performance.censor(deck, held),
    brake: (deck) => this.performance.brake(deck),
    spinBack: (deck, held) => this.performance.spinBack(deck, held),
    setReverse: (deck, on) => this.performance.setReverse(deck, on),
    reversing: (deck) => this.deck(deck)?.reverse ?? false,
    holdReverse: (deck, held) => this.performance.holdReverse(deck, held),
    backwards: (deck) => (this.deck(deck)?.speed ?? 0) < 0,
    setMute: (deck, on) => this.engine.setMute(deck, on),
    muted: (deck) => this.engine.decks()[deck].mute,
    setRepeat: (deck, on) => this.engine.setRepeat(deck, on),
    repeating: (deck) => this.engine.decks()[deck].repeat,
    setFaderStart: (deck, on) => this.engine.setFaderStart(deck, on),
    faderStart: (deck) => this.engine.decks()[deck].faderStart,
    reloop: (deck) => this.performance.reloop(deck),
    deleteHotCue: (deck, slot) => this.performance.deleteHotCue(deck, slot),
    resetTempo: (deck) => this.engine.setPitch(deck, 1),
    browse: (steps) =>
      this.library.focus() === 'lists'
        ? this.library.movePointedList(steps)
        : this.library.moveSelection(steps),
    loadSelected: (deck) => this.library.loadSelected(deck),
    loadSelectedAndPlay: (deck) => this.library.loadSelectedAndPlay(deck),
    togglePreview: () => {
      const path = this.library.selected();
      if (path) this.preview.toggle(path);
    },
    previewing: () => this.preview.active(),
    stopPreview: () => this.preview.stop(),
    seekPreview: (steps) => this.preview.skip(steps * PREVIEW_STEP_S),
    double: (deck) => this.engine.double(deck ^ 1, deck),
    removeSelected: () => this.library.removeSelected(),
    selectAll: () => this.library.selectAll(),
    clearSelection: () => this.library.clearSelection(),
    prepareSelected: () => this.library.addSelectedToPrepare(),
    toggleAutoDj: () => this.autoDj.toggle(),
    autoDjOn: () => this.autoDj.running(),
    autoDjFade: () => this.autoDj.fadeNow(),
    autoDjSkip: () => this.autoDj.skip(),
    focusSearch: () => this.library.searchRequests.update((n) => n + 1),
    keyShift: (deck) => this.engine.tracks()[deck]?.keyShift ?? 0,
    setKeyShift: (deck, semitones) => this.engine.setKeyShift(deck, semitones),
    setQuantize: (on) => this.preferences.updateDj({ quantize: on }),
    quantized: () => this.preferences.dj().quantize,
    cycleTempoRange: (step) =>
      this.engine.setTempoRange(null, nextTempoRange(this.preferences.dj().pitchRange, step)),
    cycleDeckTempoRange: (deck, step) =>
      this.engine.setTempoRange(deck, nextTempoRange(this.engine.tempoRange(deck), step)),
    toggleHelp: () => this.navigation.toggleSettings('shortcuts'),
    togglePalette: () => this.navigation.togglePalette(),
    openSettings: () => this.navigation.openSettings(),
    toCue: (deck, then) => {
      this.engine.seek(deck, this.engine.mainCue(deck)?.positionS ?? 0);
      if (then !== 'goto') this.engine.setPlaying(deck, then === 'play');
    },
    setCueHere: (deck) => this.engine.setMainCue(deck, this.engine.livePosition(deck) ?? 0),
    toStart: (deck, then) => {
      this.engine.seek(deck, 0);
      if (then !== 'goto') this.engine.setPlaying(deck, then === 'play');
    },
    toEnd: (deck) => this.performance.toEnd(deck),
    eject: (deck) => this.engine.eject(deck),
    syncOnce: (deck) => this.engine.syncOnce(deck),
    toggleLeader: (deck) =>
      this.engine.call('set_sync_leader', { deck: this.deck(deck)?.sync.leader ? null : deck }),
    leading: (deck) => this.deck(deck)?.sync.leader ?? false,
    search: (deck, direction, held) => {
      clearInterval(this.searches.get(deck));
      this.searches.delete(deck);
      if (!held) return;
      const run = () => {
        const at = this.engine.livePosition(deck);
        if (at !== null)
          this.engine.seek(deck, at + (direction * SEARCH_SPEED * SEARCH_EVERY_MS) / 1000);
      };
      this.searches.set(deck, setInterval(run, SEARCH_EVERY_MS));
    },
    stepTempo: (deck, steps) =>
      this.engine.setPitch(deck, this.engine.decks()[deck].pitch + steps * TEMPO_STEP),
    bendSmall: (deck, direction) =>
      this.engine.call('set_pitch_bend', { deck, amount: direction * SMALL_BEND }),
    setPosition: (deck, position) => {
      const duration = this.deck(deck)?.durationS ?? 0;
      if (duration > 0) this.engine.seek(deck, position * duration);
    },
    positionInTrack: (deck) => {
      const d = this.deck(deck);
      return d && d.durationS > 0 ? d.positionS / d.durationS : 0;
    },
    syncKey: (deck) => {
      const shift = this.engine.keyMatch(deck);
      if (shift !== null) this.engine.setKeyShift(deck, shift);
    },
    clearLoop: (deck) => this.engine.setLoop(deck, null, null, false),
    reloopStopped: (deck) => {
      const loop = this.deck(deck)?.looping;
      if (loop?.startS == null || loop.endS == null) return;
      this.engine.setLoop(deck, loop.startS, loop.endS, true);
      this.engine.seek(deck, loop.startS);
      this.engine.setPlaying(deck, false);
    },
    toLoopEdge: (deck, edge) => {
      const loop = this.deck(deck)?.looping;
      const at = edge === 'in' ? loop?.startS : loop?.endS;
      if (at != null) this.engine.seek(deck, at);
    },
    moveLoop: (deck, beats) => {
      const loop = this.deck(deck)?.looping;
      const beat = this.beatSeconds(deck);
      if (loop?.startS == null || loop.endS == null || beat === null) return;
      const by = beats * beat;
      this.engine.setLoop(deck, loop.startS + by, loop.endS + by, loop.active);
    },
    loopOutSet: (deck) => this.deck(deck)?.looping.endS != null,
    loopStored: (deck) => {
      const loop = this.deck(deck)?.looping;
      return loop?.startS != null && loop.endS != null;
    },
    rolling: (deck, beats) => this.rolls.get(deck) === beats,
    gridHere: (deck) => this.trackEdit.editGrid(deck, { kind: 'beatHere' }),
    moveGrid: (deck, steps) =>
      this.trackEdit.editGrid(deck, { kind: 'shift', seconds: steps * GRID_STEP_S }),
    stretchGrid: (deck, steps) => {
      const bpm = this.engine.tracks()[deck]?.tempo?.bpm;
      if (bpm) this.trackEdit.editGrid(deck, { kind: 'bpm', bpm: bpm + steps * GRID_STEP_BPM });
    },
    tapTempo: (deck) => {
      const taps = this.taps.get(deck) ?? new TapTempo();
      this.taps.set(deck, taps);
      const bpm = taps.tap(performance.now());
      if (bpm !== null)
        this.trackEdit.editGrid(deck, { kind: 'bpm', bpm: Math.round(bpm * 100) / 100 });
    },
    holdFx: (deck, slot, held) => this.fx.setEnabled(deck, slot, held),
    holdSlip: (deck, held) => this.performance.setSlip(deck, held),
    toggleFilter: (deck) => {
      const aside = this.filtersAside.get(deck);
      if (aside !== undefined) {
        this.filtersAside.delete(deck);
        this.mixer.setFilter(deck, aside);
        return;
      }
      const position = this.mixer.strip(deck).filter;
      if (position === 0) return;
      this.filtersAside.set(deck, position);
      this.mixer.setFilter(deck, 0);
    },
    filtering: (deck) => this.mixer.strip(deck).filter !== 0,
    setCrossfaderSide: (deck, side) => this.mixer.setCrossfaderSide(deck, side),
    crossfaderSide: (deck) => this.engine.decks()[deck].side,
    setCrossfaderReverse: (on) => this.mixer.setCrossfaderReverse(on),
    crossfaderReversed: () => this.mixer.crossfaderReverse(),
    setCrossfaderCurve: (position) =>
      this.mixer.setCrossfaderCurve(position < 0.5 ? 'smooth' : 'sharp'),
    crossfaderCurvePosition: () => (this.mixer.crossfaderCurve() === 'sharp' ? 1 : 0),
    nextCrossfaderCurve: () =>
      this.mixer.setCrossfaderCurve(this.mixer.crossfaderCurve() === 'sharp' ? 'smooth' : 'sharp'),
    setSamplerGain: (deck, position) => {
      for (const d of deck === null ? this.engine.deckIndexes() : [deck])
        this.sampler.setGain(d, position);
    },
    samplerGainPosition: (deck) => this.engine.decks()[deck ?? 0].samplerGain,
    stopSample: (deck, slot) => this.sampler.stop(deck, slot),
    ejectSample: (deck, slot) => this.sampler.clear(deck, slot),
    loadSample: (deck, slot) => {
      const path = this.library.selected();
      if (path) this.sampler.load(deck, slot, path);
    },
    holdSample: (deck, slot, held) =>
      held ? this.sampler.trigger(deck, slot) : this.sampler.stop(deck, slot),
    setVinyl: (deck, on) => this.engine.setVinyl(deck, { enabled: on }),
    vinylOn: (deck) => this.engine.decks()[deck].vinylEnabled,
    setJogScratch: (deck, on) => this.engine.setJogScratch(deck, on),
    jogScratching: (deck) => this.engine.decks()[deck].jogScratch,
    toggleRecording: () =>
      this.recordings.status() ? this.recordings.stop() : this.recordings.startPreferred(),
    recording: () => this.recordings.status() !== null,
    setHeadphonesLevel: (position) => this.mixer.setHeadphoneGain(position * MAX_HEADPHONE_GAIN),
    headphonesLevelPosition: () => this.mixer.headphoneGain() / MAX_HEADPHONE_GAIN,
    setMainLevel: (position) => this.mixer.setMainGain(position),
    mainLevelPosition: () => this.mixer.mainGain(),
    deckState: (deck) => {
      const d = this.deck(deck);
      return !d?.loaded ? 'off' : d.playing ? 'playing' : 'loaded';
    },
    moveFocus: () => this.library.focus.update((f) => (f === 'tracks' ? 'lists' : 'tracks')),
    browseLists: (steps) => this.library.movePointedList(steps),
    openPointedList: () => this.library.openPointedList(),
    openNextList: (steps) => this.library.openNextList(steps),
    go: () =>
      this.library.focus() === 'lists'
        ? this.library.openPointedList()
        : this.library.loadSelectedOnStoppedDeck(),
    setFocus: (where) => this.library.focus.set(where),
    sortBy: (column) => this.library.sortBy(column),
    toggleLibraryOnly: () => this.navigation.libraryOnly.update((only) => !only),
    libraryOnly: () => this.navigation.libraryOnly(),
    onTheBeat: (deck) => {
      const d = this.deck(deck);
      return !!d?.playing && d.beatPhase !== null && d.beatPhase < BEAT_LIT;
    },
    clipping: (deck) => {
      const key = deck ?? -1;
      const peak = deck === null ? this.engine.status()?.mainPeak : this.deck(deck)?.peak;
      const now = performance.now();
      if ((peak ?? 0) >= 1) this.clippedAt.set(key, now);
      return now - (this.clippedAt.get(key) ?? -Infinity) < CLIP_SHOWN_MS;
    },
    ending: (deck) => {
      const d = this.deck(deck);
      if (!d?.playing || d.durationS <= 0) return 'off';
      const left = d.durationS - d.positionS;
      return left < ENDING_SOON_S ? 'soon' : left < ENDING_S ? 'on' : 'off';
    },
    tempoState: (deck) => {
      const own = this.deck(deck)?.bpm;
      const other = this.deck(this.engine.mixedWith(deck))?.bpm;
      if (Math.abs(this.engine.decks()[deck].pitch - 1) < TEMPO_STEP / 2) return 'zero';
      if (!own || !other || Math.abs(own - other) < GRID_STEP_BPM) return 'off';
      return own < other ? 'slower' : 'faster';
    },
    phase: (deck) => {
      const own = this.deck(deck);
      const other = this.deck(this.phaseReference(deck));
      return own && other ? phaseState(own, other, IN_PHASE) : 'off';
    },
  };

  /** Built again when the language changes: their names are in it. */
  private readonly built = computed(() => {
    const controls = buildControls(this.actions);
    return { controls, byId: new Map(controls.map((c) => [c.id, c])) };
  });

  /** Every control, named in the UI's language. */
  get controls(): Control[] {
    return this.built().controls;
  }

  /** The controls that act now: those of the decks shown, and those of no deck. */
  readonly inUse = computed(() => {
    const decks = this.engine.deckIndexes().length;
    return this.controls.filter((c) => c.deck === undefined || c.deck < decks);
  });
  /** `inUse` as a set: looked into at every message of a controller. */
  private readonly inUseSet = computed(() => new Set(this.inUse()));

  /** The control `id`, whatever its deck (to name it, to map it). */
  find(id: string): Control | undefined {
    return this.built().byId.get(id);
  }

  /** The control `id` if it acts now: a control of a deck not shown does nothing. */
  findInUse(id: string): Control | undefined {
    const control = this.built().byId.get(id);
    if (control) return this.inUseSet().has(control) ? control : undefined;
    const measured = this.measured(id);
    return measured && measured.deck! < this.engine.deckIndexes().length ? measured : undefined;
  }

  /** The controls named by their measure (`deck1.jump.-8`), made once each. */
  private readonly measuredById = new Map<string, Control | undefined>();

  private measured(id: string): Control | undefined {
    if (!this.measuredById.has(id)) this.measuredById.set(id, measuredControl(this.actions, id));
    return this.measuredById.get(id);
  }

  private deck(deck: number) {
    return this.engine.status()?.decks[deck];
  }

  /** The deck whose beats `deck` is compared with: the leader, else the deck it mixes with. */
  private phaseReference(deck: number): number {
    const decks = this.engine.status()?.decks ?? [];
    const leader = this.engine.deckIndexes().find((d) => d !== deck && decks[d]?.sync.leader);
    return leader ?? this.engine.mixedWith(deck);
  }

  /** The length of a beat of `deck` (s), if its track has a grid. */
  private beatSeconds(deck: number): number | null {
    const bpm = this.deck(deck)?.bpm ?? this.engine.tracks()[deck]?.tempo?.bpm;
    return bpm ? 60 / bpm : null;
  }
}

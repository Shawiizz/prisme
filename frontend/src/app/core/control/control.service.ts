import { nextTempoRange } from '../models/preferences';
import { computed, Injectable, inject } from '@angular/core';
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
import { Control, ControlActions, buildControls, measuredControl } from './controls';

const LOOP_BEATS = 4;
/** A search runs this many times as fast as play, in jumps of this often (ms). */
const SEARCH_SPEED = 8;
const SEARCH_EVERY_MS = 50;
/** A tempo step (ratio). */
const TEMPO_STEP = 0.001;

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

  private readonly actions: ControlActions = {
    perform: (id, value, steps) => this.engine.perform(id, value, steps),
    togglePlay: (deck) => this.engine.togglePlay(deck),
    cue: (deck, pressed) => (pressed ? this.engine.cueDown(deck) : this.engine.cueUp(deck)),
    setSync: (deck, on) => this.engine.setSync(deck, on),
    synced: (deck) => this.deck(deck)?.sync.locked ?? false,
    hotCue: (deck, slot) => this.performance.pressHotCue(deck, slot),
    toggleLoop: (deck) =>
      this.deck(deck)?.looping.active
        ? this.performance.exitLoop(deck)
        : this.performance.beatLoop(deck, LOOP_BEATS),
    beatJump: (deck, beats) => this.performance.beatJump(deck, beats),
    size: (deck) => this.performance.size(deck),
    scaleSize: (deck, factor) => this.performance.scaleSize(deck, factor),
    sizedLoop: (deck) => this.performance.toggleSizedLoop(deck),
    slice: (deck, index, held) => this.performance.slice(deck, index, held),
    slicePlaying: (deck) => this.performance.slicePlaying(deck),
    cut: (deck, index, held) => this.performance.cut(deck, index, held),
    cutPlaying: (deck) => this.performance.cuts()[deck],
    setKeyLock: (deck, on) => this.engine.setKeyLock(deck, on),
    keyLocked: (deck) => this.engine.decks()[deck].keyLock,
    toggleStem: (deck, stem) => this.engine.toggleStem(deck, stem),
    sampleLoaded: (deck, slot) => !!this.engine.decks()[deck].samples[slot],
    samplePlaying: (deck, slot) => ((this.deck(deck)?.samplesPlaying ?? 0) & (1 << slot)) !== 0,
    toggleFx: (deck, slot) => this.fx.setEnabled(deck, slot, !this.fx.settings(deck, slot).enabled),
    setFxOn: (deck, slot, on) => this.fx.setEnabled(deck, slot, on),
    pressFx: (deck, slot, kind) => this.fx.press(deck, slot, kind),
    cycleFx: (deck, slot, step) => this.fx.cycle(deck, slot, step),
    nextFxPart: (deck) => this.fx.nextPart(deck),
    fxAmountPosition: (deck, slot) => this.fx.settings(deck, slot).amount,
    fxParam2Position: (deck, slot) => this.fx.settings(deck, slot).param2,
    scaleFxBeats: (deck, slot, factor) => this.fx.scaleBeats(deck, slot, factor),
    fxBeats: (deck, slot) => this.fx.settings(deck, slot).beats,
    fxFocused: (deck) => this.fx.focused(deck),
    toggleFxFocus: (deck, slot) => this.fx.toggleFocus(deck, slot),
    setFxChainOn: (deck, on) => this.fx.setChainEnabled(deck, on),
    fxChainOn: (deck) => this.fx.chain(deck).enabled,
    roll: (deck, beats) => {
      if (beats === null) this.rolls.delete(deck);
      else this.rolls.set(deck, beats);
      this.performance.loopRoll(deck, beats);
    },
    setSplitCue: (on) => this.mixer.setSplitCue(on),
    splitCue: () => this.mixer.splitCue(),
    toggleLive: (source) => this.mixer.setLiveOn(source, !this.mixer.live(source)().on),
    toggleLiveCue: (source) => this.mixer.setLiveCue(source, !this.mixer.live(source)().cue),
    setTalkover: (on) => this.mixer.setTalkover(on),
    talkover: () => this.mixer.talkover(),
    setPassthrough: (deck, on) => this.engine.setPassthrough(deck, on),
    passthrough: (deck) => this.engine.decks()[deck].passthrough,
    setHeadphones: (deck, on) => this.mixer.setDeckCue(deck, on),
    inHeadphones: (deck) => this.engine.decks()[deck].cue,
    soloHeadphones: (deck) => this.engine.soloCue(deck),
    setSlip: (deck, on) => this.performance.setSlip(deck, on),
    slipping: (deck) => this.deck(deck)?.slipMode ?? false,
    setReverse: (deck, on) => this.performance.setReverse(deck, on),
    reversing: (deck) => this.deck(deck)?.reverse ?? false,
    setMute: (deck, on) => this.engine.setMute(deck, on),
    muted: (deck) => this.engine.decks()[deck].mute,
    setRepeat: (deck, on) => this.engine.setRepeat(deck, on),
    repeating: (deck) => this.engine.decks()[deck].repeat,
    setFaderStart: (deck, on) => this.engine.setFaderStart(deck, on),
    faderStart: (deck) => this.engine.decks()[deck].faderStart,
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
    stopPreview: () => this.preview.stop(),
    seekPreview: (steps) => this.preview.skip(steps * PREVIEW_STEP_S),
    double: (deck) => this.engine.double(deck ^ 1, deck),
    removeSelected: () => this.library.removeSelected(),
    selectAll: () => this.library.selectAll(),
    clearSelection: () => this.library.clearSelection(),
    prepareSelected: () => this.library.addSelectedToPrepare(),
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
    syncKey: (deck) => {
      const shift = this.engine.keyMatch(deck);
      if (shift !== null) this.engine.setKeyShift(deck, shift);
    },
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
    rolling: (deck, beats) => this.rolls.get(deck) === beats,
    tapTempo: (deck) => {
      const taps = this.taps.get(deck) ?? new TapTempo();
      this.taps.set(deck, taps);
      const bpm = taps.tap(performance.now());
      if (bpm !== null)
        this.trackEdit.editGrid(deck, { kind: 'bpm', bpm: Math.round(bpm * 100) / 100 });
    },
    // a filter heard is set aside, whatever was before; a silent one gets back the one set aside
    toggleFilter: (deck) => {
      const position = this.mixer.strip(deck).filter;
      if (position !== 0) {
        this.filtersAside.set(deck, position);
        this.mixer.setFilter(deck, 0);
        return;
      }
      const aside = this.filtersAside.get(deck);
      this.filtersAside.delete(deck);
      if (aside !== undefined) this.mixer.setFilter(deck, aside);
    },
    setCrossfaderSide: (deck, side) => this.mixer.setCrossfaderSide(deck, side),
    setCrossfaderReverse: (on) => this.mixer.setCrossfaderReverse(on),
    crossfaderReversed: () => this.mixer.crossfaderReverse(),
    nextCrossfaderCurve: () =>
      this.mixer.setCrossfaderCurve(this.mixer.crossfaderCurve() === 'sharp' ? 'smooth' : 'sharp'),
    ejectSample: (deck, slot) => this.sampler.clear(deck, slot),
    loadSample: (deck, slot) => {
      const path = this.library.selected();
      if (path) this.sampler.load(deck, slot, path);
    },
    setVinyl: (deck, on) => this.engine.setVinyl(deck, { enabled: on }),
    vinylOn: (deck) => this.engine.decks()[deck].vinylEnabled,
    setJogScratch: (deck, on) => this.engine.setJogScratch(deck, on),
    jogScratching: (deck) => this.engine.decks()[deck].jogScratch,
    toggleRecording: () =>
      this.recordings.status() ? this.recordings.stop() : this.recordings.startPreferred(),
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

  /** The control `id`, whatever its deck (to name it, to map it, to put its light out). */
  find(id: string): Control | undefined {
    return this.built().byId.get(id) ?? this.measured(id);
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

  /** The length of a beat of `deck` (s), if its track has a grid. */
  private beatSeconds(deck: number): number | null {
    const bpm = this.deck(deck)?.bpm ?? this.engine.tracks()[deck]?.tempo?.bpm;
    return bpm ? 60 / bpm : null;
  }
}

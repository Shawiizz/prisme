import { PITCH_RANGES } from '../models/preferences';
import { computed, Injectable, inject } from '@angular/core';
import { EngineService } from '../services/engine.service';
import { FxService } from '../services/fx.service';
import { LibraryService } from '../services/library.service';
import { MixerService } from '../services/mixer.service';
import { PerformanceService } from '../services/performance.service';
import { PreferencesService } from '../services/preferences.service';
import { SamplerService } from '../services/sampler.service';
import { NavigationService } from '../services/navigation.service';
import { PREVIEW_STEP_S, PreviewService } from '../services/preview.service';
import {
  bandKnob,
  eqGain,
  filterAmount,
  filterKnob,
  trimGain,
  trimKnob,
} from '../../shared/mixer-scales';
import { BANDS } from '../models/engine';
import { hasPart, partLevel, partPlaying } from '../stem-parts';
import { Control, ControlActions, buildControls, measuredControl } from './controls';

const LOOP_BEATS = 4;

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

  private readonly actions: ControlActions = {
    togglePlay: (deck) => this.engine.togglePlay(deck),
    playing: (deck) => this.deck(deck)?.playing ?? false,
    cue: (deck, pressed) => (pressed ? this.engine.cueDown(deck) : this.engine.cueUp(deck)),
    cueState: (deck) => {
      if (!this.deck(deck)?.loaded || this.engine.atCue(deck))
        return this.deck(deck)?.loaded ? 'on' : 'off';
      return this.deck(deck)?.playing ? 'off' : 'paused';
    },
    sync: (deck) => this.engine.toggleSync(deck),
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
    beatLoop: (deck, beats) => this.performance.beatLoop(deck, beats),
    bend: (deck, direction) => this.engine.pitchBend(deck, direction),
    toggleKeyLock: (deck) => this.engine.setKeyLock(deck, !this.engine.decks()[deck].keyLock),
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
    triggerSample: (deck, slot, gain) => {
      if (gain !== undefined) this.sampler.setGain(deck, gain);
      this.sampler.trigger(deck, slot);
    },
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
    scaleFxBeats: (deck, slot, factor) => this.fx.scaleBeats(deck, slot, factor),
    setKill: (deck, band, kill) => this.mixer.setKill(deck, band, kill),
    killed: (deck, band) => this.mixer.strip(deck).kills[BANDS.indexOf(band)],
    roll: (deck, beats) => this.performance.loopRoll(deck, beats),
    setPitch: (deck, position) => {
      const range = this.preferences.dj().pitchRange;
      this.engine.setPitch(deck, 1 + (position * 2 - 1) * range);
    },
    setVolume: (deck, position) => this.mixer.setVolume(deck, position),
    setEq: (deck, band, position) => this.mixer.setEq(deck, band, eqGain(position)),
    setFilter: (deck, position) => this.mixer.setFilter(deck, filterAmount(position)),
    setTrim: (deck, position) => this.mixer.setTrim(deck, trimGain(position)),
    setCrossfader: (position) => this.mixer.setCrossfader(position * 2 - 1),
    setCueMix: (position) => this.mixer.setCueMix(position),
    cueMixPosition: () => this.mixer.cueMix(),
    fxMixPosition: (deck, slot) => this.fx.settings(deck, slot).mix,
    volumePosition: (deck) => this.engine.decks()[deck].volume,
    eqPosition: (deck, band) => bandKnob(this.mixer.strip(deck), band),
    filterPosition: (deck) => filterKnob(this.mixer.strip(deck).filter),
    trimPosition: (deck) => trimKnob(this.mixer.strip(deck).trim),
    crossfaderPosition: () => (this.mixer.crossfader() + 1) / 2,
    toggleHeadphones: (deck) => this.mixer.setDeckCue(deck, !this.engine.decks()[deck].cue),
    inHeadphones: (deck) => this.engine.decks()[deck].cue,
    toggleSlip: (deck) => this.performance.setSlip(deck, !(this.deck(deck)?.slipMode ?? false)),
    slipping: (deck) => this.deck(deck)?.slipMode ?? false,
    censor: (deck, held) => this.performance.censor(deck, held),
    reloop: (deck) => this.performance.reloop(deck),
    deleteHotCue: (deck, slot) => this.performance.deleteHotCue(deck, slot),
    resetTempo: (deck) => this.engine.setPitch(deck, 1),
    browse: (steps) => this.library.moveSelection(steps),
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
    focusSearch: () => this.library.searchRequests.update((n) => n + 1),
    keyShift: (deck) => this.engine.tracks()[deck]?.keyShift ?? 0,
    setKeyShift: (deck, semitones) => this.engine.setKeyShift(deck, semitones),
    toggleQuantize: () => this.preferences.updateDj({ quantize: !this.preferences.dj().quantize }),
    quantized: () => this.preferences.dj().quantize,
    cycleTempoRange: () => {
      const at = PITCH_RANGES.indexOf(
        this.preferences.dj().pitchRange as (typeof PITCH_RANGES)[number],
      );
      this.preferences.updateDj({ pitchRange: PITCH_RANGES[(at + 1) % PITCH_RANGES.length] });
    },
    toggleHelp: () => this.navigation.toggleSettings('shortcuts'),
    togglePalette: () => this.navigation.togglePalette(),
    openSettings: () => this.navigation.openSettings(),
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
}

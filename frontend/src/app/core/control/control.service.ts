import { Injectable, inject } from '@angular/core';
import { EngineService } from '../services/engine.service';
import { FxService } from '../services/fx.service';
import { LibraryService } from '../services/library.service';
import { MixerService } from '../services/mixer.service';
import { PerformanceService } from '../services/performance.service';
import { PreferencesService } from '../services/preferences.service';
import { SamplerService } from '../services/sampler.service';
import { NavigationService } from '../services/navigation.service';
import { eqGain, eqKnob, trimGain, trimKnob } from '../../shared/mixer-scales';
import { BANDS } from '../models/engine';
import { hasPart, partLevel, partPlaying } from '../stem-parts';
import { Control, buildControls } from './controls';

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

  readonly controls: Control[] = buildControls({
    togglePlay: (deck) => this.engine.togglePlay(deck),
    playing: (deck) => this.deck(deck)?.playing ?? false,
    cue: (deck, pressed) => (pressed ? this.engine.cueDown(deck) : this.engine.cueUp(deck)),
    sync: (deck) => this.engine.toggleSync(deck),
    synced: (deck) => this.deck(deck)?.sync.locked ?? false,
    hotCue: (deck, slot) => this.performance.pressHotCue(deck, slot),
    hotCueSet: (deck, slot) => this.performance.hotCue(deck, slot) !== null,
    toggleLoop: (deck) =>
      this.deck(deck)?.looping.active
        ? this.performance.exitLoop(deck)
        : this.performance.beatLoop(deck, LOOP_BEATS),
    looping: (deck) => this.deck(deck)?.looping.active ?? false,
    scaleLoop: (deck, factor) => this.performance.scaleLoop(deck, factor),
    loopIn: (deck) => this.performance.loopIn(deck),
    loopInSet: (deck) => this.deck(deck)?.looping.loopInS != null,
    loopOut: (deck) => this.performance.loopOut(deck),
    beatJump: (deck, beats) => this.performance.beatJump(deck, beats),
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
    triggerSample: (deck, slot) => this.sampler.trigger(deck, slot),
    samplePlaying: (deck, slot) => ((this.deck(deck)?.samplesPlaying ?? 0) & (1 << slot)) !== 0,
    toggleFx: (deck) => this.fx.setEnabled(deck, !this.fx.settings(deck).enabled),
    fxOn: (deck) => this.fx.settings(deck).enabled,
    setFxMix: (deck, position) => this.fx.setMix(deck, position),
    setPitch: (deck, position) => {
      const range = this.preferences.dj().pitchRange;
      this.engine.setPitch(deck, 1 + (position * 2 - 1) * range);
    },
    setVolume: (deck, position) => this.mixer.setVolume(deck, position),
    setEq: (deck, band, position) => this.mixer.setEq(deck, band, eqGain(position)),
    setFilter: (deck, position) => this.mixer.setFilter(deck, position * 2 - 1),
    setTrim: (deck, position) => this.mixer.setTrim(deck, trimGain(position)),
    setCrossfader: (position) => this.mixer.setCrossfader(position * 2 - 1),
    fxMixPosition: (deck) => this.fx.settings(deck).mix,
    volumePosition: (deck) => this.engine.decks()[deck].volume,
    eqPosition: (deck, band) => eqKnob(this.mixer.strip(deck).eq[BANDS.indexOf(band)]),
    filterPosition: (deck) => (this.mixer.strip(deck).filter + 1) / 2,
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
    double: (deck) => this.engine.double(deck ^ 1, deck),
    removeSelected: () => this.library.removeSelected(),
    selectAll: () => this.library.selectAll(),
    clearSelection: () => this.library.clearSelection(),
    prepareSelected: () => this.library.addSelectedToPrepare(),
    focusSearch: () => this.library.searchRequests.update((n) => n + 1),
    toggleHelp: () => this.navigation.toggleSettings('shortcuts'),
    togglePalette: () => this.navigation.togglePalette(),
    openSettings: () => this.navigation.openSettings(),
  });

  private readonly byId = new Map(this.controls.map((c) => [c.id, c]));

  find(id: string): Control | undefined {
    return this.byId.get(id);
  }

  private deck(deck: number) {
    return this.engine.status()?.decks[deck];
  }
}

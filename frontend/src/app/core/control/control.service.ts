import { computed, Injectable, inject } from '@angular/core';
import { EngineService } from '../services/engine.service';
import { LibraryService } from '../services/library.service';
import { MixerService } from '../services/mixer.service';
import { PerformanceService } from '../services/performance.service';
import { SamplerService } from '../services/sampler.service';
import { NavigationService } from '../services/navigation.service';
import { RecordingService } from '../services/recording.service';
import { TrackEditService } from '../services/track-edit.service';
import { TapTempo } from '../../shared/tap-tempo';
import { PREVIEW_STEP_S, PreviewService } from '../services/preview.service';
import { Control, ControlActions, buildControls, measuredControl } from './controls';

/** A search runs this many times as fast as play, in jumps of this often (ms). */
const SEARCH_SPEED = 8;
const SEARCH_EVERY_MS = 50;

/** Every action a key or a controller can do (see `buildControls`). */
@Injectable({ providedIn: 'root' })
export class ControlService {
  private readonly engine = inject(EngineService);
  private readonly performance = inject(PerformanceService);
  private readonly mixer = inject(MixerService);
  private readonly sampler = inject(SamplerService);
  private readonly library = inject(LibraryService);
  private readonly navigation = inject(NavigationService);
  private readonly preview = inject(PreviewService);
  private readonly recordings = inject(RecordingService);
  private readonly trackEdit = inject(TrackEditService);
  /** The searches running, by deck. */
  private readonly searches = new Map<number, ReturnType<typeof setInterval>>();
  private readonly taps = new Map<number, TapTempo>();
  /** The filter of each deck set aside, where it stood. */
  private readonly filtersAside = new Map<number, number>();

  private readonly actions: ControlActions = {
    perform: (id, value, steps) => this.engine.perform(id, value, steps),
    toggleStem: (deck, stem) => this.engine.toggleStem(deck, stem),
    sampleLoaded: (deck, slot) => !!this.engine.decks()[deck].samples[slot],
    samplePlaying: (deck, slot) => ((this.deck(deck)?.samplesPlaying ?? 0) & (1 << slot)) !== 0,
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
    toggleHelp: () => this.navigation.toggleSettings('shortcuts'),
    togglePalette: () => this.navigation.togglePalette(),
    openSettings: () => this.navigation.openSettings(),
    eject: (deck) => this.engine.eject(deck),
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
    syncKey: (deck) => {
      const shift = this.engine.keyMatch(deck);
      if (shift !== null) this.engine.setKeyShift(deck, shift);
    },
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
    ejectSample: (deck, slot) => this.sampler.clear(deck, slot),
    loadSample: (deck, slot) => {
      const path = this.library.selected();
      if (path) this.sampler.load(deck, slot, path);
    },
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
}

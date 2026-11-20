import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import {
  ControlState,
  Cue,
  DeckStatus,
  DeckTrackUpdate,
  EngineStatus,
  TrackInfo,
  Waveform,
} from '../models/engine';
import { EngineService } from './engine.service';
import { PreferencesService } from './preferences.service';

function deck(changes: Partial<DeckStatus> = {}): DeckStatus {
  return {
    loaded: true,
    playing: false,
    positionS: 0,
    durationS: 300,
    speed: 0,
    bpm: null,
    beatPhase: null,
    sync: { locked: false, leader: false, rateOffset: 1 },
    looping: { startS: null, endS: null, active: false, loopInS: null },
    peak: 0,
    cue: false,
    slipMode: false,
    reverse: false,
    slipPositionS: null,
    vinyl: {
      enabled: false,
      mode: 'relative',
      runOut: false,
      crossed: false,
      signal: 'none',
      level: 0,
      recordSpeed: 0,
      recordPositionS: null,
    },
    samplesPlaying: 0,
    stemsWaiting: 0,
    stemsHere: 0,
    ...changes,
  };
}

function status(decks: DeckStatus[]): EngineStatus {
  return {
    audioRunning: true,
    decks,
    mainPeak: 0,
    micPeak: 0,
    auxPeak: 0,
    health: null,
    recording: null,
    preview: null,
  };
}

describe('EngineService', () => {
  let engine: EngineService;
  let calls: [string, Record<string, unknown>][];

  beforeEach(() => {
    engine = TestBed.inject(EngineService);
    calls = [];
    vi.spyOn(engine, 'call').mockImplementation(async (command, args) => {
      calls.push([command, args]);
    });
  });

  afterEach(() => vi.restoreAllMocks());

  /** The deck as the engine tells it after what was sent. */
  function playing(on: boolean) {
    engine.status.set(status([deck({ playing: on }), deck()]));
  }

  it('has the backend perform the keys of a deck, pressed and let go', () => {
    engine.press(1, 'cue');
    engine.press(1, 'cue', false);
    engine.togglePlay(0);
    const performed = { steps: false, step: null };
    expect(calls).toEqual([
      ['perform_action', { target: 'deck2.cue', value: 1, ...performed }],
      ['perform_action', { target: 'deck2.cue', value: 0, ...performed }],
      ['perform_action', { target: 'deck1.play', value: 1, ...performed }],
    ]);
  });

  describe('loading', () => {
    afterEach(() => clearMocks());

    it('shows nothing, and no error, when another load asked for on the deck goes there', async () => {
      mockIPC((command) => (command === 'load_track' ? null : undefined));
      expect(await engine.load(0, 'C:/Music/late.mp3')).toBe(false);
      expect(engine.tracks()[0]).toBeNull();
      expect(engine.error()).toBeNull();
      expect(engine.loading()[0]).toBe(false);
    });

    it('says for a few seconds, and not as an error, that a playing deck keeps its track', async () => {
      vi.useFakeTimers();
      TestBed.inject(PreferencesService).dj.update((p) => ({ ...p, protectPlaying: true }));
      playing(true);
      expect(await engine.load(0, 'C:/Music/next.mp3')).toBe(false);
      expect(engine.refusal()).toBe('Deck A is playing: nothing is loaded on it');
      expect(engine.error()).toBeNull();
      vi.advanceTimersByTime(5000);
      expect(engine.refusal()).toBeNull();
      vi.useRealTimers();
    });

    it('takes a refusal back at the next thing asked', async () => {
      TestBed.inject(PreferencesService).dj.update((p) => ({ ...p, protectPlaying: true }));
      playing(true);
      await engine.eject(0);
      expect(engine.refusal()).toBe('Deck A is playing: its track stays on it');
      vi.mocked(engine.call).mockRestore();
      mockIPC(() => undefined);
      await engine.setPlaying(0, false);
      expect(engine.refusal()).toBeNull();
    });
  });

  describe('fewer decks', () => {
    it('leaves C and D empty, as the backend leaves them', () => {
      const dj = TestBed.inject(PreferencesService).dj;
      const decks = (deckCount: number) => {
        dj.update((p) => ({ ...p, deckCount }));
        TestBed.tick();
      };
      const track = (loadId: number) => ({ loadId }) as TrackInfo;
      decks(4);
      engine.tracks.set([track(1), track(2), track(3), track(4)]);
      engine.waveforms.set(Array(4).fill({} as Waveform));
      decks(2);
      expect(engine.tracks()).toEqual([track(1), track(2), null, null]);
      expect(engine.waveforms().map((w) => w !== null)).toEqual([true, true, false, false]);
      decks(4);
      expect(engine.tracks()[2]).toBeNull();
    });
  });

  describe('the audio started again', () => {
    afterEach(() => clearMocks());

    it('shows the decks as the backend left them', async () => {
      const [a, b] = engine.decks();
      mockIPC((command) =>
        command === 'mixer_state' ? { decks: [{ ...a, vinylEnabled: true }, b] } : undefined,
      );
      await engine.reloadDecks();
      expect(engine.decks().map((d) => d.vinylEnabled)).toEqual([true, false]);
    });
  });

  describe('a track separated once on its deck', () => {
    const stems = { separation: 'vocals', partial: false } as const;
    const update = (changes: Partial<DeckTrackUpdate>): DeckTrackUpdate => ({
      deck: 1,
      loadId: 7,
      analysis: null,
      stems: null,
      ...changes,
    });

    beforeEach(() =>
      engine.tracks.set([
        null,
        { loadId: 7, separation: 'none' } as unknown as TrackInfo,
        null,
        null,
      ]),
    );

    it('takes its stems as they come', () => {
      engine['completeTrack'](update({ stems }));
      expect(engine.tracks()[1]?.separation).toBe('vocals');
    });

    it('leaves a later load of the deck as it is', () => {
      engine['completeTrack'](update({ loadId: 6, stems }));
      expect(engine.tracks()[1]?.separation).toBe('none');
    });
  });

  describe('what the backend changed', () => {
    const drop: Cue = { hot: 2, positionS: 12, loopLengthS: null, color: 0xff0000, label: '' };
    const level = { level: 1, on: true };
    const stemMix = { vocals: level, drums: level, bass: level, other: level };

    function controls(changes: Partial<ControlState>): ControlState {
      return {
        mixer: { decks: engine.decks() } as ControlState['mixer'],
        tracks: [null, { loadId: 7, keyShift: -2, stemMix, cues: [drop] }, null, null],
        quantize: true,
        pitchRange: 0.16,
        ...changes,
      };
    }

    beforeEach(() =>
      engine.tracks.set([null, { loadId: 7, cues: [] } as unknown as TrackInfo, null, null]),
    );

    it('shows the decks, the tracks and the preferences as the backend has them', () => {
      const decks = engine.decks().map((d, i) => (i === 0 ? { ...d, pitch: 1.04 } : d));
      engine['applyControls'](controls({ mixer: { decks } as ControlState['mixer'] }));
      expect(engine.decks()[0].pitch).toBe(1.04);
      expect(engine.tracks()[1]).toEqual(
        expect.objectContaining({ keyShift: -2, cues: [drop] }) as TrackInfo,
      );
      const dj = TestBed.inject(PreferencesService).dj();
      expect([dj.quantize, dj.pitchRange]).toEqual([true, 0.16]);
    });

    it('leaves what did not change as it was, and another load of a deck', () => {
      const before = engine.decks();
      engine['applyControls'](
        controls({ tracks: [null, { loadId: 6, keyShift: 3, stemMix, cues: [] }, null, null] }),
      );
      expect(engine.decks()).toBe(before);
      expect(engine.tracks()[1]?.cues).toEqual([]);
    });
  });
});

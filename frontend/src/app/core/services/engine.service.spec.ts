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
import { ConfirmService } from '../../shared/confirm/confirm';

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
    looping: { startS: null, endS: null, beats: null, active: false, loopInS: null },
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
      return true;
    });
  });

  afterEach(() => vi.restoreAllMocks());

  it('has the backend perform the keys of a deck, pressed and let go', () => {
    engine.press(1, 'cue');
    engine.press(1, 'cue', false);
    engine.togglePlay(0);
    engine.setKeyLock(1, false);
    engine.setVinylEnabled(0, true);
    // half the range up: three quarters of the fader
    engine.moveTempo(1, engine.tempoRange(1) / 2);
    const performed = { steps: false, confirmed: false, order: expect.anything() };
    expect(calls).toEqual([
      ['perform_action', { target: 'deck2.cue', value: 1, ...performed }],
      ['perform_action', { target: 'deck2.cue', value: 0, ...performed }],
      ['perform_action', { target: 'deck1.play', value: 1, ...performed }],
      ['perform_action', { target: 'deck2.keylock.off', value: 1, ...performed }],
      ['perform_action', { target: 'deck1.vinyl.on', value: 1, ...performed }],
      ['perform_action', { target: 'deck2.pitch', value: 0.75, ...performed }],
    ]);
  });

  it('numbers the actions it sends, for the backend to perform them in that order', () => {
    engine.press(0, 'censor');
    engine.press(0, 'censor', false);
    const orders = calls.map(
      ([, args]) => (args as { order: { run: number; number: number } }).order,
    );
    expect(orders[1].run).toBe(orders[0].run);
    expect(orders[1].number).toBe(orders[0].number + 1);
  });

  describe('a deck that plays', () => {
    afterEach(() => clearMocks());

    const refused = (eject: boolean) => ({
      code: 'deckPlays',
      params: { deck: 0, eject },
      message: 'deck 1 plays: its track stays on it',
    });

    it('says for a few seconds, and not as an error, that it keeps its track', async () => {
      vi.useFakeTimers();
      expect(await engine.failed(refused(false))).toBe(false);
      expect(engine.refusal()).toBe('Deck A is playing: nothing is loaded on it');
      expect(engine.error()).toBeNull();
      vi.advanceTimersByTime(5000);
      expect(engine.refusal()).toBeNull();
      vi.useRealTimers();
    });

    it('takes a refusal back at the next thing asked', async () => {
      await engine.failed(refused(true));
      expect(engine.refusal()).toBe('Deck A is playing: its track stays on it');
      vi.mocked(engine.call).mockRestore();
      mockIPC(() => undefined);
      await engine.press(0, 'play');
      expect(engine.refusal()).toBeNull();
    });

    it('gives its track up once the DJ says so, the action asked again', async () => {
      const confirm = vi.spyOn(TestBed.inject(ConfirmService), 'ask');
      const asked = {
        code: 'confirmReplace',
        params: { target: 'deck2.eject', deck: 1, eject: true },
        message: 'deck 2 plays',
      };
      confirm.mockResolvedValueOnce(false);
      expect(await engine.failed(asked)).toBe(false);
      expect(calls).toEqual([]);
      confirm.mockResolvedValueOnce(true);
      expect(await engine.failed(asked)).toBe(true);
      expect(calls).toEqual([
        [
          'perform_action',
          {
            target: 'deck2.eject',
            value: 1,
            steps: false,
            confirmed: true,
            order: expect.anything(),
          },
        ],
      ]);
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
        loading: [false, false, false, false],
        quantize: true,
        pitchRange: 0.16,
        perform: [],
        preview: { track: null, opening: null },
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

    it('leaves what did not change as it was', () => {
      const before = engine.decks();
      engine['applyControls'](controls({}));
      const track = engine.tracks()[1];
      engine['applyControls'](controls({}));
      expect(engine.decks()).toBe(before);
      expect(engine.tracks()[1]).toBe(track);
    });

    it('keeps the sections the backend did not send again', () => {
      engine['applyControls'](controls({}));
      const { tracks, preview } = engine.controls()!;
      const perform = [{ slicer: 4, cut: null, roll: null, fxFocus: null }];
      engine['applyControls']({ perform });
      expect(engine.controls()!.perform).toBe(perform);
      expect(engine.controls()!.tracks).toBe(tracks);
      expect(engine.controls()!.preview).toBe(preview);
      expect(engine.tracks()[1]?.keyShift).toBe(-2);
    });

    it('shows a track put on a deck whoever loaded it, and lets one taken off go', async () => {
      const loaded = { loadId: 8, deck: 1, cues: [] } as unknown as TrackInfo;
      mockIPC((command) => (command === 'loaded_tracks' ? [null, loaded, null, null] : undefined));
      const loading = [false, true, false, false];
      engine['applyControls'](controls({ loading }));
      expect(engine.loading()).toEqual(loading);
      engine['applyControls'](
        controls({ tracks: [null, { loadId: 8, keyShift: 0, stemMix, cues: [] }, null, null] }),
      );
      expect(engine.tracks()[1]?.loadId).toBe(7);
      await vi.waitFor(() => expect(engine.tracks()[1]?.loadId).toBe(8));
      engine.waveforms.set([null, {} as Waveform, null, null]);
      engine['applyControls'](controls({ tracks: [null, null, null, null] }));
      expect(engine.tracks()[1]).toBeNull();
      expect(engine.waveforms()[1]).toBeNull();
      clearMocks();
    });
  });
});

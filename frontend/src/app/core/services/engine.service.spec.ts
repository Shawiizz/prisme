import { TestBed } from '@angular/core/testing';
import { DeckStatus, EngineStatus, TrackInfo } from '../models/engine';
import { EngineService } from './engine.service';

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

  describe('CUE', () => {
    beforeEach(() => playing(false));

    it('previews from the cue point while held, then goes back to it', async () => {
      await engine.cueDown(0);
      expect(calls).toEqual([['set_playing', { deck: 0, playing: true }]]);
      playing(true);
      engine.cueUp(0);
      expect(calls.slice(1)).toEqual([
        ['set_playing', { deck: 0, playing: false }],
        ['seek', { deck: 0, seconds: 0 }],
      ]);
    });

    it('plays on once let go when PLAY was pressed while it was held', async () => {
      await engine.cueDown(0);
      playing(true);
      engine.togglePlay(0);
      engine.cueUp(0);
      expect(calls).toEqual([['set_playing', { deck: 0, playing: true }]]);
      engine.togglePlay(0);
      expect(calls.slice(1)).toEqual([['set_playing', { deck: 0, playing: false }]]);
    });
  });

  describe('SYNC off', () => {
    const locked = { locked: true, leader: false, rateOffset: 1 };

    beforeEach(() => {
      engine.tracks.set([{ tempo: { bpm: 120 } } as TrackInfo, null, null, null]);
      engine.patchDeck(0, { pitch: 1 });
    });

    it('keeps the tempo a deck playing by itself was locked to, its fader moved there', async () => {
      engine.status.set(status([deck({ playing: true, bpm: 126, sync: locked }), deck()]));
      await engine.setSync(0, false);
      expect(calls).toEqual([
        ['set_pitch', { deck: 0, ratio: 1.05 }],
        ['set_sync_locked', { deck: 0, locked: false }],
      ]);
      expect(engine.decks()[0].pitch).toBe(1.05);
    });

    it('leaves the tempo of a record to the turntable', async () => {
      const vinyl = { ...deck().vinyl, enabled: true };
      engine.status.set(status([deck({ bpm: 126, sync: locked, vinyl }), deck()]));
      await engine.setSync(0, false);
      expect(calls).toEqual([['set_sync_locked', { deck: 0, locked: false }]]);
      expect(engine.decks()[0].pitch).toBe(1);
    });
  });
});

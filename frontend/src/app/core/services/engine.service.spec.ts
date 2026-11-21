import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { ControlState, Waveform } from '../models/engine';
import { EngineService } from './engine.service';
import { ConfirmService } from '../../shared/confirm/confirm';
import type { Cue, DeckSettings, TrackInfo } from '../models/backend';

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
    engine.moveTempo(1, 0.75);
    const performed = { confirmed: false, track: null, order: expect.anything() };
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

  it('numbers a key whose press is lost after its press', () => {
    engine.press(0, 'sync');
    engine.lose(0, 'sync');
    const [pressed, lost] = calls.map(([, args]) => args['order'] as { number: number });
    expect(calls[1][0]).toBe('let_go_key');
    expect(lost.number).toBe(pressed.number + 1);
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
            confirmed: true,
            track: null,
            order: expect.anything(),
          },
        ],
      ]);
    });

    it('loads the track the DJ was asked about once the DJ says so', async () => {
      vi.spyOn(TestBed.inject(ConfirmService), 'ask').mockResolvedValueOnce(true);
      const asked = {
        code: 'confirmReplace',
        params: { target: 'library.load1.play', deck: 0, eject: false, track: '/music/a.flac' },
        message: 'deck 1 plays',
      };
      expect(await engine.failed(asked)).toBe(true);
      expect(calls).toEqual([
        [
          'perform_action',
          {
            target: 'library.load1.play',
            value: 1,
            confirmed: true,
            track: '/music/a.flac',
            order: expect.anything(),
          },
        ],
      ]);
    });
  });

  describe('what the backend changed', () => {
    const drop: Cue = { hot: 2, positionS: 12, loopLengthS: null, color: 0xff0000, label: '' };
    const level = { level: 1, on: true };
    const stemMix = { vocals: level, drums: level, bass: level, other: level };
    const pads = { fxFocus: null, fxShown: 0 };

    const settings = [1, 1, 1, 1].map((pitch) => ({ pitch }) as DeckSettings);

    function controls(changes: Partial<ControlState>): Partial<ControlState> {
      return {
        mixer: {} as ControlState['mixer'],
        decks: settings,
        tracks: [null, { loadId: 7, keyShift: -2, stemMix, cues: [drop] } as never, null, null],
        loading: [false, false, false, false],
        quantize: true,
        pitchRange: 0.16,
        perform: [pads, pads, pads, pads],
        preview: { track: null, opening: null },
        ...changes,
      };
    }

    beforeEach(() =>
      engine.tracks.set([null, { loadId: 7, cues: [] } as unknown as TrackInfo, null, null]),
    );

    it('shows the decks and the tracks as the backend has them', () => {
      const decks = settings.map((d, i) => (i === 0 ? { ...d, pitch: 1.04 } : d));
      engine['applyControls'](controls({ decks }));
      expect(engine.decks()[0].pitch).toBe(1.04);
      expect(engine.tracks()[1]).toEqual(
        expect.objectContaining({ keyShift: -2, cues: [drop] }) as TrackInfo,
      );
    });

    it('leaves what did not change as it was', () => {
      engine['applyControls'](controls({}));
      const before = engine.decks();
      const track = engine.tracks()[1];
      engine['applyControls'](controls({}));
      expect(engine.decks()).toBe(before);
      expect(engine.tracks()[1]).toBe(track);
    });

    it('takes the decks that changed, the others kept as they were', () => {
      engine['applyControls'](controls({}));
      const before = engine.decks();
      const moved = { pitch: 1.02 } as DeckSettings;
      engine['applyControls']({ decks: [{ ...before[0] }, moved, before[2], before[3]] });
      expect(engine.decks()[1]).toBe(moved);
      expect(engine.decks()[0]).toBe(before[0]);
      expect(engine.controls()!.decks).toEqual(engine.decks());
    });

    it('keeps the sections the backend did not send again', () => {
      engine['applyControls'](controls({}));
      const { tracks, preview } = engine.controls()!;
      const perform: ControlState['perform'] = [pads, pads, pads, { fxFocus: 2, fxShown: 2 }];
      engine['applyControls']({ perform });
      expect(engine.controls()!.perform).toBe(perform);
      expect(engine.controls()!.tracks).toBe(tracks);
      expect(engine.controls()!.preview).toBe(preview);
      expect(engine.tracks()[1]?.keyShift).toBe(-2);
    });

    it('shows a track put on a deck whoever loaded it, and lets one taken off go', () => {
      const watched: unknown[] = [];
      mockIPC((command, args) => {
        if (command === 'watch_waveform') watched.push(args);
      });
      const loading: ControlState['loading'] = [false, true, false, false];
      engine['applyControls'](controls({ loading }));
      expect(engine.loading()).toEqual(loading);
      const loaded = { loadId: 8, deck: 1, keyShift: 0, stemMix, cues: [] } as unknown as TrackInfo;
      engine['applyControls'](controls({ tracks: [null, loaded, null, null] }));
      expect(engine.tracks()[1]).toBe(loaded);
      expect(watched).toHaveLength(1);
      engine.waveforms.set([null, {} as Waveform, null, null]);
      engine['applyControls'](controls({ tracks: [null, null, null, null] }));
      expect(engine.tracks()[1]).toBeNull();
      expect(engine.waveforms()[1]).toBeNull();
      clearMocks();
    });
  });
});

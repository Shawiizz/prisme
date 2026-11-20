import { readFileSync } from 'node:fs';
import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { Band, CrossfaderCurve, EFFECT_KINDS, LiveSource } from '../models/engine';
import { StemPart } from '../stem-parts';
import { AutoDjService } from './auto-dj.service';
import { EngineService } from './engine.service';
import { FxService } from './fx.service';
import { LibraryService } from './library.service';
import { MixerService } from './mixer.service';
import { PerformanceService } from './performance.service';
import { PreviewService } from './preview.service';
import { SamplerService } from './sampler.service';

/** The ids of the actions a controller can be given: those the backend performs. */
const TARGETS = new Set<string>(
  (
    JSON.parse(readFileSync('../apps/prisme-desktop/controllers/targets.json', 'utf8')) as {
      id: string;
    }[]
  ).map((t) => t.id),
);

const DECKS = [0, 1, 2, 3];
const SLOTS = [0, 1, 2];
const BANDS: Band[] = ['low', 'mid', 'high'];
const PARTS: StemPart[] = ['vocals', 'drums', 'bass', 'other', 'instrumental'];
const SOURCES: LiveSource[] = ['mic', 'aux'];
const CURVES: CrossfaderCurve[] = ['smooth', 'sharp'];
/** The keys of a deck the screen presses by name, its pads aside (the backend names those). */
const KEYS = [
  'sync',
  'cue',
  'play',
  'eject',
  'end',
  'start.keep',
  'tempo.up',
  'tempo.down',
  'tempo.reset',
  'grid.here',
  'key.up',
  'key.down',
  'key.reset',
  'key.sync',
  'fx.part',
];

describe('the actions the screen sends', () => {
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
  });

  it('are each one the backend performs for a controller', () => {
    mockIPC(() => null);
    const engine = TestBed.inject(EngineService);
    const sent: string[] = [];
    vi.spyOn(engine, 'call').mockImplementation(async (command, args) => {
      if (command === 'perform_action' || command === 'let_go_key')
        sent.push(String(args['target']));
      return true;
    });
    const mixer = TestBed.inject(MixerService);
    const fx = TestBed.inject(FxService);
    const performance = TestBed.inject(PerformanceService);
    const sampler = TestBed.inject(SamplerService);
    for (const deck of DECKS) {
      PARTS.forEach((part) => {
        engine.toggleStem(deck, part);
        engine.setStemLevel(deck, part, 0.5);
      });
      [true, false].forEach((on) => {
        engine.setKeyLock(deck, on);
        engine.setFaderStart(deck, on);
        engine.setMute(deck, on);
        engine.setRepeat(deck, on);
        engine.setVinylEnabled(deck, on);
        mixer.setDeckCue(deck, on);
        fx.setChainEnabled(deck, on);
        performance.setSlip(deck, on);
        performance.setReverse(deck, on);
        performance.censor(deck, on);
        performance.spinBack(deck, on);
      });
      engine.soloCue(deck);
      engine.togglePlay(deck);
      engine.moveTempo(deck, 0.5);
      KEYS.forEach((key) => {
        engine.press(deck, key);
        engine.letGo(deck, key);
      });
      mixer.setVolume(deck, 0.5);
      mixer.setTrim(deck, 0.5);
      mixer.setFilter(deck, 0.5);
      BANDS.forEach((band) => {
        mixer.setEq(deck, band, 0.5);
        mixer.toggleKill(deck, band);
      });
      SLOTS.forEach((slot) => {
        EFFECT_KINDS.forEach((kind) => fx.press(deck, slot, kind));
        fx.setMix(deck, slot, 0.5);
        fx.setAmount(deck, slot, 0.5);
        fx.setParam2(deck, slot, 0.5);
        fx.stepBeats(deck, slot, 1);
      });
      fx.nextPart(deck);
      performance.pressHotCue(deck, 15);
      performance.deleteHotCue(deck, 15);
      performance.loopIn(deck);
      performance.loopOut(deck);
      performance.reloop(deck);
      performance.scaleLoop(deck, 0.5);
      performance.scaleLoop(deck, 2);
      performance.resize(deck, 1);
      performance.brake(deck);
      sampler.setGain(deck, 0.5);
    }
    SOURCES.forEach((source) => {
      mixer.setLiveLevel(source, 0.5);
      mixer.toggleLiveOn(source);
      mixer.toggleLiveCue(source);
    });
    CURVES.forEach((curve) => mixer.setCrossfaderCurve(curve));
    [true, false].forEach((on) => {
      mixer.setTalkover(on);
      mixer.setSplitCue(on);
      mixer.setCrossfaderReverse(on);
    });
    mixer.setMainGain(0.5);
    mixer.setBoothGain(0.5);
    mixer.setBalance(0.5);
    mixer.setCrossfader(0.5);
    mixer.setCueMix(0.5);
    mixer.setHeadphoneLevel(0.5);
    const autoDj = TestBed.inject(AutoDjService);
    autoDj.toggle();
    autoDj.fadeNow();
    autoDj.skip();
    TestBed.inject(PreviewService).stop();
    TestBed.inject(LibraryService).clearSelection();

    expect(sent.length).toBeGreaterThan(400);
    expect(sent.filter((id) => !TARGETS.has(id))).toEqual([]);
  });
});

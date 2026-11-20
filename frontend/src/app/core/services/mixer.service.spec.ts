import { TestBed } from '@angular/core/testing';
import { ControlState, MixerState } from '../models/engine';
import { EngineService } from './engine.service';
import { FxService } from './fx.service';
import { MixerService } from './mixer.service';
import { PerformanceService } from './performance.service';

describe('MixerService', () => {
  it('shows the mixer the backend tells, the same as it tells other sections', () => {
    const engine = TestBed.inject(EngineService);
    const mixer = TestBed.inject(MixerService);
    const told = { mixer: { limiter: false, mic: { on: true } } as MixerState } as ControlState;
    engine.controls.set(told);
    const mic = mixer.mic();
    expect([mixer.limiter(), mic.on]).toEqual([false, true]);
    engine.controls.set({ ...told, loading: [true, false, false, false] });
    expect(mixer.mic()).toBe(mic);
  });
});

describe('the screen', () => {
  it('has the backend perform what its knobs, faders and keys drive, as a controller would', () => {
    const engine = TestBed.inject(EngineService);
    const performed: [string, number][] = [];
    vi.spyOn(engine, 'perform').mockImplementation(async (target, value) => {
      performed.push([target, value]);
      return true;
    });
    const mixer = TestBed.inject(MixerService);
    mixer.setEq(1, 'low', 0.25);
    mixer.setCrossfader(0.5);
    mixer.setHeadphoneLevel(0.8);
    mixer.toggleLiveOn('aux');
    mixer.setDeckCue(0, false);
    mixer.setCrossfaderCurve('sharp');
    TestBed.inject(FxService).setChainEnabled(1, true);
    TestBed.inject(PerformanceService).scaleLoop(0, 0.5);
    TestBed.inject(PerformanceService).pad(1, 'jump.-4', true);
    expect(performed).toEqual([
      ['deck2.eq.low', 0.25],
      ['mixer.crossfader', 0.5],
      ['mixer.headphones', 0.8],
      ['mixer.aux.on', 1],
      ['deck1.headphones.off', 1],
      ['mixer.crossfaderCurve.sharp', 1],
      ['deck2.fx.chain.on', 1],
      ['deck1.loop.halve', 1],
      ['deck2.jump.-4', 1],
    ]);
    vi.restoreAllMocks();
  });
});

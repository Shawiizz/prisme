import { TestBed } from '@angular/core/testing';
import { ControlState, DeckPerform } from '../models/engine';
import { EngineService } from './engine.service';
import { PerformanceService } from './performance.service';

describe('PerformanceService', () => {
  afterEach(() => vi.restoreAllMocks());

  it('shows what the pads hold as the backend keeps it, and has the backend play them', () => {
    const engine = TestBed.inject(EngineService);
    const performance = TestBed.inject(PerformanceService);
    const calls: unknown[] = [];
    vi.spyOn(engine, 'perform').mockImplementation(async (target, value) => {
      calls.push([target, value]);
      return true;
    });
    const none: DeckPerform = { cut: null, roll: null, fxFocus: null };
    expect(performance.cuts()).toEqual([null, null, null, null]);
    const perform = [none, { ...none, cut: 2, roll: 0.5 }, none, none];
    engine.controls.set({ perform } as ControlState);
    expect(performance.cuts()[1]).toBe(2);
    expect(performance.rolls()[1]).toBe(0.5);

    performance.slice(1, 2, true);
    performance.roll(0, 7, false);
    performance.cut(1, 0, true);
    expect(calls).toEqual([
      ['deck2.slice3', 1],
      ['deck1.roll8', 0],
      ['deck2.cut1', 1],
    ]);
  });
});

import { TestBed } from '@angular/core/testing';
import { ControlState, DeckPerform } from '../models/engine';
import { EngineService } from './engine.service';
import { FxService } from './fx.service';

describe('FxService', () => {
  it('chooses on the screen the slot a controller focuses its knobs on', () => {
    const engine = TestBed.inject(EngineService);
    const fx = TestBed.inject(FxService);
    const none: DeckPerform = { cut: null, roll: null, fxFocus: null };
    const focused = (slot: number | null) =>
      engine.controls.set({ perform: [{ ...none, fxFocus: slot }, none] } as ControlState);
    fx.choose(0, 1);
    focused(2);
    TestBed.tick();
    expect([fx.focused(0), fx.chosen(0)]).toEqual([2, 2]);
    fx.choose(0, 0);
    focused(null);
    TestBed.tick();
    expect([fx.focused(0), fx.chosen(0)]).toEqual([null, 0]);
  });
});

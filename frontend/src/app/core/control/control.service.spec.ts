import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { EngineService } from '../services/engine.service';
import { MixerService } from '../services/mixer.service';
import { ControlService } from './control.service';

describe('ControlService', () => {
  afterEach(() => clearMocks());

  it('turn a filter heard off, even one moved since the last was set aside', () => {
    mockIPC(() => undefined);
    const mixer = TestBed.inject(MixerService);
    const toggle = TestBed.inject(ControlService).find('deck1.filter.toggle')!;
    const filter = () => mixer.strip(0).filter;

    mixer.setFilter(0, 0.6);
    toggle.apply(1);
    expect(filter()).toBe(0);
    mixer.setFilter(0, -0.3);
    toggle.apply(1);
    expect(filter()).toBe(0);
    toggle.apply(1);
    expect(filter()).toBe(-0.3);
  });

  it('tell where the tempo fader is, for the controllers to show it', () => {
    mockIPC(() => undefined);
    const engine = TestBed.inject(EngineService);
    const tempo = TestBed.inject(ControlService).find('deck1.pitch')!;
    expect(tempo.position!()).toBe(0.5);
    engine.setPitch(0, 1 + engine.tempoRange(0) / 2);
    expect(tempo.position!()).toBeCloseTo(0.75);
    engine.setPitch(0, 0.5);
    expect(tempo.position!()).toBe(0);
  });
});

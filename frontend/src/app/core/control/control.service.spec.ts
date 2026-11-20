import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
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
});

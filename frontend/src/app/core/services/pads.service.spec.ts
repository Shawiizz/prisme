import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { vi } from 'vitest';
import { PadsService } from './pads.service';
import { PerformanceService } from './performance.service';
import { ControlService } from '../control/control.service';

describe('PadsService', () => {
  beforeEach(() => mockIPC(() => undefined, { shouldMockEvents: true }));
  afterEach(() => clearMocks());

  it('turn the page of the tabs that have pages only, a deck apart from the others', () => {
    const pads = TestBed.inject(PadsService);
    expect([pads.mode(0), pads.page(0)]).toEqual(['cues', 0]);

    pads.turnPage(0);
    expect(pads.page(0)).toBe(1);
    expect(pads.page(1)).toBe(0);

    pads.choose(0, 'roll');
    pads.turnPage(0);
    expect([pads.mode(0), pads.page(0)]).toEqual(['roll', 1]);
    pads.choose(0, 'sampler');
    pads.turnPage(0);
    expect([pads.mode(0), pads.page(0)]).toEqual(['sampler', 0]);
  });

  it('put the slicer on the bar the deck plays in as its tab is chosen', () => {
    const anchor = vi.spyOn(TestBed.inject(PerformanceService), 'anchorSlicer');
    TestBed.inject(PadsService).choose(1, 'slice');
    expect(anchor).toHaveBeenCalledWith(1);
  });

  it('show what another window chose', async () => {
    const pads = TestBed.inject(PadsService);
    await pads.start();
    await emit('pads-shown', { deck: 2, mode: 'fx', page: 0 });
    expect(pads.mode(2)).toBe('fx');
  });

  it('take a key or a controller on a deck pads as the screen own', () => {
    const pads = TestBed.inject(PadsService);
    const controls = TestBed.inject(ControlService);
    controls.act('deck2.pads.jump', 1);
    controls.act('deck2.pads.cues', 0);
    expect(pads.mode(1)).toBe('jump');
    controls.act('deck1.pads.page', 1);
    expect(pads.page(0)).toBe(1);
  });
});

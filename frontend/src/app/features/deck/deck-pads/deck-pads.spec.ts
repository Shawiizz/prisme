import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { EngineStatus } from '../../../core/models/engine';
import { EngineService } from '../../../core/services/engine.service';
import { PerformanceService } from '../../../core/services/performance.service';
import { DeckPads } from './deck-pads';

describe('DeckPads', () => {
  let calls: string[];

  beforeEach(() => {
    mockIPC(() => undefined);
    Element.prototype.setPointerCapture = () => undefined;
    calls = [];
    const performance = TestBed.inject(PerformanceService);
    vi.spyOn(performance, 'loopRoll').mockImplementation(
      async (d, b) => void calls.push(`roll ${d} ${b}`),
    );
    vi.spyOn(performance, 'censor').mockImplementation(
      async (d, a) => void calls.push(`censor ${d} ${a}`),
    );
    TestBed.inject(EngineService).status.set({
      decks: [0, 1].map(() => ({ loaded: true, looping: {}, samplesPlaying: 0 })),
    } as unknown as EngineStatus);
  });

  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
  });

  function pads(deck: number) {
    const fixture = TestBed.createComponent(DeckPads);
    fixture.componentRef.setInput('deck', deck);
    fixture.detectChanges();
    const element = fixture.nativeElement as HTMLElement;
    const pointer = (type: string, target: Element) =>
      target.dispatchEvent(new PointerEvent(type, { pointerId: 1, button: 0, bubbles: true }));
    const mode = (label: string) =>
      [...element.querySelectorAll<HTMLButtonElement>('.modes button')]
        .find((b) => b.textContent?.trim() === label)!
        .click();
    return { fixture, element, pointer, mode };
  }

  it('let a roll go on the deck it rolls, the mode or the deck changed since', () => {
    const { fixture, element, pointer, mode } = pads(0);
    mode('ROLL');
    fixture.detectChanges();
    const pad = element.querySelector('.pr-pad')!;
    pointer('pointerdown', pad);
    mode('LOOP');
    fixture.componentRef.setInput('deck', 1);
    fixture.detectChanges();
    pointer('pointerup', pad);
    expect(calls).toEqual(['roll 0 0.0625', 'roll 0 null']);
  });

  it('let what is held go when the pads go', () => {
    const { fixture, element, pointer, mode } = pads(0);
    mode('ROLL');
    fixture.detectChanges();
    pointer('pointerdown', element.querySelector('.pr-pad')!);
    pointer('pointerdown', element.querySelector('button.censor')!);
    fixture.destroy();
    expect(calls).toEqual(['roll 0 0.0625', 'censor 0 true', 'censor 0 false', 'roll 0 null']);
  });

  it('let CENSOR go when its pointer is lost, off the key', () => {
    const { pointer, element } = pads(0);
    const key = element.querySelector('button.censor')!;
    pointer('pointerdown', key);
    pointer('lostpointercapture', key);
    expect(calls).toEqual(['censor 0 true', 'censor 0 false']);
  });
});

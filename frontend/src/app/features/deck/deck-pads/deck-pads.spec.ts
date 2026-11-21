import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { FX_SLOTS } from '../../../core/models/engine';
import { EngineService } from '../../../core/services/engine.service';
import { PerformanceService } from '../../../core/services/performance.service';
import { DeckPads } from './deck-pads';
import type { DeckSettings, EngineStatus, FxSettings } from '../../../core/models/backend';

describe('DeckPads', () => {
  let calls: string[];

  beforeEach(() => {
    const page = (page: string, action: string) => ({
      page,
      pads: [{ action, caption: '1', label: { key: action } }],
    });
    mockIPC((command) =>
      command === 'pad_pages' ? [page('loops', 'loop.0.25'), page('roll', 'roll1')] : undefined,
    );
    Element.prototype.setPointerCapture = () => undefined;
    calls = [];
    const performance = TestBed.inject(PerformanceService);
    void performance.loadPages();
    vi.spyOn(performance, 'pad').mockImplementation(
      async (d, action, held) => calls.push(`${action} ${d} ${held}`) > 0,
    );
    vi.spyOn(performance, 'censor').mockImplementation(
      async (d, a) => calls.push(`censor ${d} ${a}`) > 0,
    );
    const engine = TestBed.inject(EngineService);
    const slot: FxSettings = {
      kind: 'echo',
      enabled: false,
      mix: 0.5,
      amount: 0.5,
      param2: 0.5,
      beats: 1,
    };
    const fx = { slots: Array(FX_SLOTS).fill(slot), enabled: true, part: null };
    engine.decks.set(
      [0, 1].map(
        () => ({ fx, samples: [], samplerGain: 1, sizeBeats: 4 }) as unknown as DeckSettings,
      ),
    );
    engine.status.set({
      decks: [0, 1].map(() => ({
        loaded: true,
        canMove: true,
        looping: {},
        samplesPlaying: 0,
        lights: { cue: 'off', pads: { roll: { lit: 0, marked: 0 } } },
      })),
    } as unknown as EngineStatus);
  });

  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
  });

  async function pads(deck: number) {
    // the pages asked for come
    await new Promise((resolve) => setTimeout(resolve));
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

  it('let a roll go on the deck it rolls, the mode or the deck changed since', async () => {
    const { fixture, element, pointer, mode } = await pads(0);
    mode('ROLL');
    fixture.detectChanges();
    const pad = element.querySelector('.pr-pad')!;
    pointer('pointerdown', pad);
    mode('LOOP');
    fixture.componentRef.setInput('deck', 1);
    fixture.detectChanges();
    pointer('pointerup', pad);
    expect(calls).toEqual(['roll1 0 true', 'roll1 0 false']);
  });

  it('let what is held go when the pads go', async () => {
    const { fixture, element, pointer, mode } = await pads(0);
    mode('ROLL');
    fixture.detectChanges();
    pointer('pointerdown', element.querySelector('.pr-pad')!);
    pointer('pointerdown', element.querySelector('button.censor')!);
    fixture.destroy();
    expect(calls).toEqual(['roll1 0 true', 'censor 0 true', 'censor 0 false', 'roll1 0 false']);
  });

  it('mark hot cues on a track its record holds in place, which no loop moves', async () => {
    const engine = TestBed.inject(EngineService);
    engine.status.update((s) => ({
      ...s!,
      decks: s!.decks.map((d) => ({ ...d, canMove: false })),
    }));
    const { fixture, element, mode } = await pads(0);
    const disabled = () => element.querySelector<HTMLButtonElement>('.pr-pad')!.disabled;
    expect(disabled()).toBe(false);
    mode('LOOP');
    fixture.detectChanges();
    expect(disabled()).toBe(true);
    expect(element.querySelector<HTMLButtonElement>('button.slip')!.disabled).toBe(true);
  });

  it('let CENSOR go when its pointer is lost, off the key', async () => {
    const { pointer, element } = await pads(0);
    const key = element.querySelector('button.censor')!;
    pointer('pointerdown', key);
    pointer('lostpointercapture', key);
    expect(calls).toEqual(['censor 0 true', 'censor 0 false']);
  });
});

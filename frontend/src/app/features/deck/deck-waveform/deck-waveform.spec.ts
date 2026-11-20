import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { DeckSettings, TrackInfo } from '../../../core/models/engine';
import { EngineService } from '../../../core/services/engine.service';

describe('DeckWaveform', () => {
  /** What the canvases' observer, made once as the module loads, is told of their sizes. */
  let resized: (entries: unknown[]) => void = () => undefined;

  beforeEach(() => {
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(told: (entries: unknown[]) => void) {
          resized = told;
        }
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
  });

  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  /** The waveform of deck 0 with a track on decks 0 and 1, the hands sent as `deck at`. */
  async function shown() {
    const { DeckWaveform } = await import('./deck-waveform');
    mockIPC(() => undefined);
    const engine = TestBed.inject(EngineService);
    const hands: string[] = [];
    vi.spyOn(engine, 'seek').mockResolvedValue(true);
    vi.spyOn(engine, 'hand').mockImplementation(
      async (deck, at) => hands.push(`${deck} ${at}`) > 0,
    );
    const track = { path: '/a.mp3', durationS: 60, separation: 'none' } as TrackInfo;
    engine.tracks.set([track, track]);
    engine.decks.set(
      [0, 1].map(() => ({ passthrough: false, vinylEnabled: false }) as DeckSettings),
    );
    const fixture = TestBed.createComponent(DeckWaveform);
    fixture.componentRef.setInput('deck', 0);
    fixture.detectChanges();
    return { fixture, hands };
  }

  /** The waveform of deck 0, its overview pressed. */
  async function pressed() {
    Element.prototype.setPointerCapture = () => undefined;
    const { fixture, hands } = await shown();
    const overview = (fixture.nativeElement as HTMLElement).querySelector('.overview')!;
    overview.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 1, button: 0 }));
    expect(hands.length).toBe(1);
    return { fixture, hands };
  }

  it('takes the hand off the record when it goes, a press still held', async () => {
    const { fixture, hands } = await pressed();
    fixture.destroy();
    expect(hands.at(-1)).toBe('0 null');
  });

  it('takes the hand off the record pressed when it shows another deck', async () => {
    const { fixture, hands } = await pressed();
    fixture.componentRef.setInput('deck', 1);
    fixture.detectChanges();
    expect(hands.at(-1)).toBe('0 null');
  });

  it('draws a deck at rest again once shown after being hidden', async () => {
    const { WaveformPainter } = await import('./waveform-painter');
    const drawn = vi.spyOn(WaveformPainter.prototype, 'drawZoomed').mockImplementation(() => {});
    vi.spyOn(WaveformPainter.prototype, 'drawOverview').mockImplementation(() => {});
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
      clearRect() {},
    } as unknown as CanvasRenderingContext2D);
    const { fixture } = await shown();
    await fixture.whenStable();
    const canvases = [...(fixture.nativeElement as HTMLElement).querySelectorAll('canvas')];
    const sized = (width: number, height: number) =>
      resized(
        canvases.map((target) => ({
          target,
          devicePixelContentBoxSize: [{ inlineSize: width, blockSize: height }],
        })),
      );
    sized(800, 100);
    const before = drawn.mock.calls.length;
    sized(800, 100);
    expect(drawn.mock.calls.length).toBe(before);
    sized(0, 0);
    sized(800, 100);
    expect(drawn.mock.calls.length).toBe(before + 1);
  });
});

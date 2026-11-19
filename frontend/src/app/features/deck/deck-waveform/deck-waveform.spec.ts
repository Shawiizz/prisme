import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { TrackInfo } from '../../../core/models/engine';
import { EngineService } from '../../../core/services/engine.service';

describe('DeckWaveform', () => {
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  /** The waveform of deck 0, its overview pressed: the hands sent, as `deck at`. */
  async function pressed() {
    // the canvases' sizes are watched from the module's load, which the test runner cannot do
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    const { DeckWaveform } = await import('./deck-waveform');
    mockIPC(() => undefined);
    Element.prototype.setPointerCapture = () => undefined;
    const engine = TestBed.inject(EngineService);
    const hands: string[] = [];
    vi.spyOn(engine, 'seek').mockResolvedValue(undefined);
    vi.spyOn(engine, 'hand').mockImplementation(
      async (deck, at) => void hands.push(`${deck} ${at}`),
    );
    const track = { path: '/a.mp3', durationS: 60, separation: 'none' } as TrackInfo;
    engine.tracks.set([track, track]);
    const fixture = TestBed.createComponent(DeckWaveform);
    fixture.componentRef.setInput('deck', 0);
    fixture.detectChanges();
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
});

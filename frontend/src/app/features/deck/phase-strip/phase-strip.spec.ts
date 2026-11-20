import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { DeckStatus, EngineStatus } from '../../../core/models/engine';
import { EngineService } from '../../../core/services/engine.service';

function deck(bpm: number, beatPhase: number): DeckStatus {
  return { loaded: true, playing: true, positionS: 30, rate: 1, bpm, beatPhase } as DeckStatus;
}

describe('PhaseStrip', () => {
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  /** What the strip says of deck A and deck B. */
  async function says(a: DeckStatus, b: DeckStatus): Promise<string> {
    // the canvases' sizes are watched from the module's load, which the test runner cannot do
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
    const { PhaseStrip } = await import('./phase-strip');
    mockIPC(() => undefined);
    TestBed.inject(EngineService).status.set({ decks: [a, b] } as EngineStatus);
    const fixture = TestBed.createComponent(PhaseStrip);
    fixture.detectChanges();
    const words = (fixture.nativeElement as HTMLElement).querySelector('.phase')!.textContent!;
    fixture.destroy();
    return words.trim();
  }

  it('tells decks at different tempos when they will be off the beat, never that they match', async () => {
    expect(await says(deck(120, 0.5), deck(150, 0.5))).toBe('Off the beat: adjust the pitch');
    expect(await says(deck(128, 0.5), deck(128.1, 0.5))).toBe('Off by 1/16 in 38 s');
  });

  it('tells decks at one tempo whether their beats meet', async () => {
    expect(await says(deck(128, 0.5), deck(128, 0.5))).toBe('Matched');
    expect(await says(deck(128, 0.5), deck(128, 0.6))).toBe('B ahead by 47 ms');
  });
});

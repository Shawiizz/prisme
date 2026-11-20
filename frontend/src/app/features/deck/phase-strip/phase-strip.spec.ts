import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { ControlState, EngineStatus, PhaseReading } from '../../../core/models/engine';
import { EngineService } from '../../../core/services/engine.service';

describe('PhaseStrip', () => {
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  /** What the strip says of deck B against deck A when the backend reads them so. */
  async function says(reading: PhaseReading): Promise<string> {
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
    const phase = { decks: [0, 1], ...reading };
    const engine = TestBed.inject(EngineService);
    engine.status.set({ decks: [{}, {}], phase } as EngineStatus);
    engine.controls.set({ routing: { internal: false } } as ControlState);
    const fixture = TestBed.createComponent(PhaseStrip);
    fixture.detectChanges();
    const words = (fixture.nativeElement as HTMLElement).querySelector('.phase')!.textContent!;
    fixture.destroy();
    return words.trim();
  }

  it('tells decks drifting apart when they will be heard off the beat', async () => {
    expect(await says({ state: 'drifting', seconds: 0 })).toBe('Off the beat: adjust the pitch');
    expect(await says({ state: 'drifting', seconds: 38 })).toBe('Off by 1/16 in 38 s');
  });

  it('tells decks at one tempo whether their beats meet', async () => {
    expect(await says({ state: 'matched' })).toBe('Matched');
    expect(await says({ state: 'ahead', ms: 47 })).toBe('B ahead by 47 ms');
    expect(await says({ state: 'off' })).toBe('');
  });
});

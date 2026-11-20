import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { ControlState, EngineStatus, PreviewTrack } from '../models/engine';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';
import { PreviewService } from './preview.service';

describe('PreviewService', () => {
  afterEach(() => clearMocks());

  it('shows what the backend previews, and has it stop a preview the engine ended', () => {
    const performed: unknown[] = [];
    mockIPC((command, args) => {
      if (command === 'perform_action') performed.push((args as { target: string }).target);
    });
    const engine = TestBed.inject(EngineService);
    // the mixer's part of what the backend tells is not this test's
    vi.spyOn(TestBed.inject(MixerService), 'apply').mockImplementation(() => undefined);
    const preview = TestBed.inject(PreviewService);
    const track: PreviewTrack = {
      id: 3,
      path: '/a.mp3',
      title: 'A',
      artist: null,
      durationS: 200,
      waveformReady: false,
    };
    engine.controls.set({ preview: { track, opening: '/b.mp3' } } as ControlState);
    expect(preview.previewing('/a.mp3')).toBe(true);
    expect(preview.previewing('/b.mp3')).toBe(true);
    expect(preview.previewing('/c.mp3')).toBe(false);

    const reported = { id: 3, positionS: 60, durationS: 200, waiting: false };
    engine.status.set({ preview: reported } as EngineStatus);
    TestBed.tick();
    expect(performed).toEqual([]);
    engine.status.set({ preview: null } as EngineStatus);
    TestBed.tick();
    expect(performed).toEqual(['library.preview.stop']);
  });
});

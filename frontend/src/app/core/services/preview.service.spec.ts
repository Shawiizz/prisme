import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { ControlState, PreviewTrack } from '../models/engine';
import { EngineService } from './engine.service';
import { PreviewService } from './preview.service';

describe('PreviewService', () => {
  afterEach(() => clearMocks());

  it('shows what the backend previews', () => {
    mockIPC(() => undefined);
    const engine = TestBed.inject(EngineService);
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
  });
});

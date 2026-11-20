import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Waveform, addWaveformPiece, decodeWaveform } from '../models/engine';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';

/**
 * The preview: a track of the library heard in the headphones alone before it goes on a deck,
 * about a third in. The backend previews the selected track (`library.preview`) or one the
 * screen names, and tells what it previews with the engine's state; the engine stops it at the
 * end of the track, when the track is loaded on a deck, or when the audio restarts. Its
 * waveform comes from the library, or once its file is read (`preview-update`).
 */
@Injectable({ providedIn: 'root' })
export class PreviewService {
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);

  /** The track previewed, from when the engine has it. */
  readonly track = computed(() => this.engine.controls()?.preview.track ?? null);
  private readonly trackId = computed(() => this.track()?.id ?? null);
  readonly waveform = signal<Waveform | null>(null);
  /** The track asked for while its file opens. */
  readonly opening = computed(() => this.engine.controls()?.preview.opening ?? null);
  /** Where the preview plays, once the engine reports it. */
  readonly status = computed(() => {
    const status = this.engine.status()?.preview ?? null;
    return status !== null && status.id === this.trackId() ? status : null;
  });
  /** The headphones are apart from what the audience hears: a preview can be heard. */
  readonly heard = computed(
    () => this.engine.audioRunning() && this.mixer.headphonesInUse() !== null,
  );
  /** The preview the engine has reported: once it no longer does, it has stopped. */
  private reported: number | null = null;

  constructor() {
    effect(() => {
      const id = this.trackId();
      untracked(() => {
        this.waveform.set(null);
        if (id !== null && this.track()?.waveformReady) void this.fetchWaveform(id);
      });
    });
    effect(() => {
      const id = this.trackId();
      const status = this.status();
      if (status) {
        this.reported = status.id;
      } else if (id !== null && this.reported === id) {
        untracked(() => this.stop());
      }
    });
  }

  async init(): Promise<void> {
    await listen<number>('preview-update', ({ payload: id }) => {
      if (id === this.trackId()) void this.fetchWaveform(id);
    });
  }

  /** Whether `path` is previewed, or about to be. */
  previewing(path: string): boolean {
    return this.track()?.path === path || this.opening() === path;
  }

  /** Previews `path` in place of the track previewed, or stops it if it is the one previewed. */
  toggle(path: string) {
    return this.engine.call('toggle_preview', { path });
  }

  stop() {
    return this.engine.perform('library.preview.stop', 1);
  }

  seek(seconds: number) {
    if (!this.track()) return Promise.resolve(false);
    return this.engine.call('seek_preview', { seconds: Math.max(0, seconds) });
  }

  private async fetchWaveform(id: number) {
    const piece = decodeWaveform(await invoke<ArrayBuffer>('preview_waveform', { id }));
    if (piece && this.trackId() === id) this.waveform.set(addWaveformPiece(null, piece));
  }
}

import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Waveform, decodeWaveform } from '../models/engine';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';
import { same } from '../same';

/**
 * The preview: a track of the library heard in the headphones alone before it goes on a deck,
 * about a third in. The backend previews the selected track (`library.preview`) or one the
 * screen names, and tells what it previews with the engine's state; the engine stops it at the
 * end of the track, when the track is loaded on a deck, or when the audio restarts, and the
 * backend then tells it previews nothing. Its
 * waveform comes from the library, or once its file is read (`preview-update`).
 */
@Injectable({ providedIn: 'root' })
export class PreviewService {
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);

  /** The track previewed, from when the engine has it. */
  readonly track = computed(() => this.engine.told().preview.track, { equal: same });
  private readonly trackId = computed(() => this.track()?.id ?? null);
  readonly waveform = signal<Waveform | null>(null);
  /** Where the preview plays, once the engine reports it. */
  readonly status = computed(() => {
    const status = this.engine.status()?.preview ?? null;
    return status !== null && status.id === this.trackId() ? status : null;
  });
  /** The headphones are apart from what the audience hears: a preview can be heard. */
  readonly heard = computed(
    () => this.engine.audioRunning() && this.mixer.headphonesInUse() !== null,
  );

  constructor() {
    // from the backend's first status on: the service starts with the app
    effect(() => {
      const id = this.engine.ready() ? this.trackId() : null;
      untracked(() => {
        this.waveform.set(null);
        if (id !== null && this.track()?.waveformReady) void this.fetchWaveform(id);
      });
    });
  }

  async init(): Promise<void> {
    await listen<number>('preview-update', ({ payload: id }) => {
      if (id === this.trackId()) void this.fetchWaveform(id);
    });
  }

  /** Previews `path` in place of the track previewed, or stops it if it is the one previewed. */
  toggle(path: string) {
    return this.engine.call('toggle_preview', { path });
  }

  stop() {
    return this.engine.perform('library.preview.stop', 1);
  }

  /** The preview put at `share` (0 to 1) of its track. */
  seek(share: number) {
    return this.engine.call('seek_preview', { share });
  }

  private async fetchWaveform(id: number) {
    const waveform = decodeWaveform(await invoke<ArrayBuffer>('preview_waveform', { id }));
    if (waveform && this.trackId() === id) this.waveform.set(waveform);
  }
}

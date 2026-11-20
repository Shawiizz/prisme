import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { PreviewTrack, Waveform, addWaveformPiece, decodeWaveform } from '../models/engine';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';
import { errorText } from '../errors';

/** How far a step of a controller's encoder moves the preview (s). */
export const PREVIEW_STEP_S = 4;

/**
 * The preview: a track of the library heard in the headphones alone before it goes on a deck,
 * about a third in. The engine stops it at the end of the track, when the track is loaded on
 * a deck, or when the audio restarts; its waveform comes from the library, or once its file is
 * read (`preview-update`).
 */
@Injectable({ providedIn: 'root' })
export class PreviewService {
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);

  /** The track previewed, from when the engine has it. */
  readonly track = signal<PreviewTrack | null>(null);
  readonly waveform = signal<Waveform | null>(null);
  /** The track asked for while its file opens. */
  readonly opening = signal<string | null>(null);
  /** Where the preview plays, once the engine reports it. */
  readonly status = computed(() => {
    const status = this.engine.status()?.preview ?? null;
    return status !== null && status.id === this.track()?.id ? status : null;
  });
  /** The headphones are apart from what the audience hears: a preview can be heard. */
  readonly heard = computed(
    () => this.engine.audioRunning() && this.mixer.headphonesInUse() !== null,
  );
  /** The preview the engine has reported: once it no longer does, it has stopped. */
  private reported: number | null = null;

  constructor() {
    effect(() => {
      const track = this.track();
      const status = this.status();
      if (status) {
        this.reported = status.id;
      } else if (track && this.reported === track.id) {
        untracked(() => this.stop());
      }
    });
  }

  async init(): Promise<void> {
    await listen<number>('preview-update', ({ payload: id }) => {
      if (id === this.track()?.id) void this.fetchWaveform(id);
    });
  }

  /** Whether `path` is previewed, or about to be. */
  previewing(path: string): boolean {
    return this.track()?.path === path || this.opening() === path;
  }

  /** Previews `path`, or stops it if it is the one previewed. */
  toggle(path: string) {
    return this.previewing(path) ? this.stop() : this.start(path);
  }

  /** Previews `path` in place of the track previewed (which stops if it cannot be). */
  async start(path: string): Promise<void> {
    this.opening.set(path);
    // a track asked for after it, or a stop, has the last word
    const latest = () => this.opening() === path;
    try {
      const track = await invoke<PreviewTrack | null>('preview_track', { path });
      if (track && latest()) {
        this.track.set(track);
        this.waveform.set(null);
        if (track.waveformReady) void this.fetchWaveform(track.id);
      }
      this.engine.error.set(null);
    } catch (e) {
      this.engine.error.set(errorText(e));
      if (latest()) void this.stop();
    } finally {
      if (latest()) this.opening.set(null);
    }
  }

  stop() {
    this.track.set(null);
    this.waveform.set(null);
    this.opening.set(null);
    return this.engine.call('stop_preview', {});
  }

  seek(seconds: number) {
    if (this.track()) return this.engine.call('seek_preview', { seconds: Math.max(0, seconds) });
    return Promise.resolve();
  }

  /** Moves the preview by `seconds`, forwards or back. */
  skip(seconds: number) {
    if (this.track()) return this.engine.call('skip_preview', { seconds });
    return Promise.resolve();
  }

  private async fetchWaveform(id: number) {
    const piece = decodeWaveform(await invoke<ArrayBuffer>('preview_waveform', { id }));
    if (piece && this.track()?.id === id) this.waveform.set(addWaveformPiece(null, piece));
  }
}

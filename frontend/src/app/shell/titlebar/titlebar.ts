import { Component, computed, inject, output, signal } from '@angular/core';
import { RecordOption, RecordingService } from '../../core/services/recording.service';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SettingsService } from '../../core/services/settings.service';
import { EngineService } from '../../core/services/engine.service';
import { UpdateService } from '../../core/services/update.service';
import { MixerService } from '../../core/services/mixer.service';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { AudioBackend } from '../../core/models/audio';
import { decimal } from '../../shared/time';
import { levelSegments } from '../../shared/level';

/** Custom title bar: drag area, audio state, recording, update, dialogs and window buttons. */
@Component({
  selector: 'app-titlebar',
  templateUrl: './titlebar.html',
  styleUrl: './titlebar.scss',
  imports: [TPipe],
})
export class Titlebar {
  protected readonly settings = inject(SettingsService);
  protected readonly engine = inject(EngineService);
  protected readonly recording = inject(RecordingService);
  protected readonly updates = inject(UpdateService);
  protected readonly mixer = inject(MixerService);
  protected readonly recordMenu = signal(false);
  protected readonly lastTitle = computed(() => {
    const last = this.recording.last();
    return last
      ? t('Last recording: {path} ({time}, {tracks} tracks)', {
          path: last.path,
          time: this.clock(last.seconds),
          tracks: last.tracks,
        })
      : t('Record the mix (WAV and tracklist in Documents/Prisme Recordings)');
  });
  /** The period in milliseconds, once the driver has called the engine. */
  protected readonly bufferMs = computed(() => {
    const frames = this.engine.status()?.health?.periodFrames;
    const rate = this.settings.info()?.sampleRate;
    return frames && rate ? decimal((frames / rate) * 1000, 1) : null;
  });
  protected readonly level = computed(() => levelSegments(this.engine.status()?.mainPeak ?? 0, 24));
  readonly openSettings = output<void>();
  readonly openShortcuts = output<void>();
  readonly openControllers = output<void>();
  readonly openTurntables = output<void>();

  record(option: RecordOption) {
    this.recordMenu.set(false);
    this.recording.start(option.source);
  }

  backendName(backend: AudioBackend): string {
    return backend === 'asio' ? 'ASIO' : 'WASAPI';
  }

  /** `1:02:05` or `12:34`. */
  clock(seconds: number): string {
    const s = Math.floor(seconds);
    const mmss = `${String(Math.floor(s / 60) % 60).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
    return s >= 3600 ? `${Math.floor(s / 3600)}:${mmss}` : mmss;
  }

  private readonly window = getCurrentWindow();

  minimize() {
    this.window.minimize();
  }
  toggleMaximize() {
    this.window.toggleMaximize();
  }
  close() {
    this.window.close();
  }
}

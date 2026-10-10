import { Component, computed, inject, output, signal } from '@angular/core';
import { RecordOption, RecordingService } from '../../core/services/recording.service';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { SettingsService } from '../../core/services/settings.service';
import { EngineService } from '../../core/services/engine.service';

/** Custom title bar: drag area, audio state, settings and window buttons. */
@Component({
  selector: 'app-titlebar',
  templateUrl: './titlebar.html',
  styleUrl: './titlebar.scss',
})
export class Titlebar {
  protected readonly settings = inject(SettingsService);
  protected readonly engine = inject(EngineService);
  protected readonly recording = inject(RecordingService);
  protected readonly recordMenu = signal(false);
  protected readonly lastTitle = computed(() => {
    const last = this.recording.last();
    return last
      ? `Last recording: ${last.path} (${this.clock(last.seconds)}, ${last.tracks} tracks)`
      : 'Record the mix (WAV and tracklist in Documents/Prisme Recordings)';
  });
  readonly openSettings = output<void>();
  readonly openShortcuts = output<void>();
  readonly openControllers = output<void>();
  readonly openTurntables = output<void>();

  record(option: RecordOption) {
    this.recordMenu.set(false);
    this.recording.start(option.source);
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

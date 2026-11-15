import { Component, computed, inject, signal } from '@angular/core';
import { RecordOption, RecordingService } from '../../core/services/recording.service';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { LibraryService } from '../../core/services/library.service';
import { SettingsService } from '../../core/services/settings.service';
import { EngineService } from '../../core/services/engine.service';
import { UpdateService } from '../../core/services/update.service';
import { MixerService } from '../../core/services/mixer.service';
import { NavigationService } from '../../core/services/navigation.service';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { BACKEND_NAMES } from '../../core/models/audio';
import { formatClock } from '../../shared/time';
import { levelMeter } from '../../shared/level';
import { CTRL, MAC } from '../../core/keyboard/shortcuts';
import { Dismiss, overlay } from '../../shared/overlay';

/** Custom title bar: drag area, audio state, recording, update, dialogs and window buttons. */
@Component({
  selector: 'app-titlebar',
  templateUrl: './titlebar.html',
  styleUrl: './titlebar.scss',
  imports: [TPipe, Dismiss],
})
export class Titlebar {
  protected readonly CTRL = CTRL;
  protected readonly MAC = MAC;
  protected readonly BACKEND_NAMES = BACKEND_NAMES;
  protected readonly settings = inject(SettingsService);
  protected readonly engine = inject(EngineService);
  protected readonly recording = inject(RecordingService);
  protected readonly updates = inject(UpdateService);
  protected readonly mixer = inject(MixerService);
  protected readonly navigation = inject(NavigationService);
  protected readonly library = inject(LibraryService);
  protected readonly recordMenu = signal(false);
  protected readonly lastTitle = computed(() => {
    const last = this.recording.last();
    return last
      ? t('Last recording: {path} ({time}, {tracks} tracks)', {
          path: last.path,
          time: formatClock(last.seconds),
          tracks: last.tracks,
        })
      : t('Record the mix (WAV and tracklist in Documents/Prisme Recordings)');
  });
  protected readonly level = levelMeter(() => this.engine.status()?.mainPeak ?? 0, 24);

  constructor() {
    overlay(this.recordMenu, () => this.recordMenu.set(false));
  }

  record(option: RecordOption) {
    this.recordMenu.set(false);
    this.recording.start(option.source);
  }

  protected readonly clock = formatClock;

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

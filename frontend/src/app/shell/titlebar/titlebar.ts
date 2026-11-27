import { Component, computed, inject, signal } from '@angular/core';
import { RecordOption, RecordingService } from '../../core/services/recording.service';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { LibraryService } from '../../core/services/library.service';
import { SettingsService } from '../../core/services/settings.service';
import { AssignService } from '../../core/assign/assign.service';
import { AssignTarget } from '../../shared/assign/assign';
import { EngineService } from '../../core/services/engine.service';
import { UpdateService } from '../../core/services/update.service';
import { MixerService } from '../../core/services/mixer.service';
import { NavigationService } from '../../core/services/navigation.service';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { BACKEND_NAMES } from '../../core/models/audio';
import { formatClock } from '../../shared/time';
import { meterSegments } from '../../shared/meter';
import { percent } from '../../shared/readout';
import { MAC } from '../../core/keyboard/shortcuts';
import { KeyboardService } from '../../core/keyboard/keyboard.service';
import { Dismiss, overlay } from '../../shared/overlay';
import { WindowService } from '../../core/services/window.service';

/** Custom title bar: drag area, audio state, recording, update, dialogs and window buttons. */
@Component({
  selector: 'app-titlebar',
  templateUrl: './titlebar.html',
  styleUrl: './titlebar.scss',
  imports: [TPipe, Dismiss, AssignTarget],
})
export class Titlebar {
  protected readonly MAC = MAC;
  protected readonly BACKEND_NAMES = BACKEND_NAMES;
  protected readonly settings = inject(SettingsService);
  protected readonly engine = inject(EngineService);
  protected readonly recording = inject(RecordingService);
  protected readonly updates = inject(UpdateService);
  protected readonly percent = percent;
  protected readonly mixer = inject(MixerService);
  protected readonly navigation = inject(NavigationService);
  protected readonly library = inject(LibraryService);
  protected readonly keyboard = inject(KeyboardService);
  protected readonly window = inject(WindowService);
  protected readonly fullscreenTitle = computed(() => {
    const what = this.window.fullscreen() ? t('Exit full screen') : t('Full screen');
    const key = this.keyboard.firstKey('general.fullscreen');
    return key ? `${what} (${key})` : what;
  });
  protected readonly assign = inject(AssignService);
  protected readonly assignTitle = computed(() => {
    const what = t("Give the buttons their keys, or a controller's controls");
    const key = this.keyboard.firstKey('general.assign');
    return key ? `${what} (${key})` : what;
  });
  protected readonly settingsTitle = computed(() => {
    const key = this.keyboard.firstKey('general.settings');
    return key ? t('Settings ({key})', { key }) : t('Settings');
  });
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
  protected readonly level = meterSegments(() => this.engine.status()?.meters.main, 16);

  constructor() {
    overlay(this.recordMenu, () => this.recordMenu.set(false));
  }

  record(option: RecordOption) {
    this.recordMenu.set(false);
    this.recording.start(option.source);
  }

  protected readonly clock = formatClock;

  private readonly own = getCurrentWindow();

  minimize() {
    this.own.minimize();
  }
  toggleMaximize() {
    this.own.toggleMaximize();
  }
  close() {
    this.own.close();
  }
}

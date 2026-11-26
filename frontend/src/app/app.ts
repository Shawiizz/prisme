import { Component, OnInit, computed, inject, signal } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { PreviewService } from './core/services/preview.service';
import { LibraryService } from './core/services/library.service';
import { Titlebar } from './shell/titlebar/titlebar';
import { DeckLane } from './features/deck/deck-lane/deck-lane';
import { PhaseStrip } from './features/deck/phase-strip/phase-strip';
import { Library } from './features/library/library';
import { SettingsDialog } from './features/settings/settings-dialog';
import { CommandPalette } from './features/command-palette/command-palette';
import { SelectPicker } from './shared/select-picker/select-picker';
import { Confirm } from './shared/confirm/confirm';
import { Tooltip } from './shared/tooltip/tooltip';
import { Phone } from './features/phone/phone';
import { NowPlaying } from './shell/now-playing/now-playing';
import { ScreensService } from './core/services/screens.service';
import { clickWithEveryFinger } from './core/every-finger';
import { dismissSplash } from './core/splash';
import { DECKS_WINDOW, PHONE } from './core/platform';
import { WindowService } from './core/services/window.service';
import { KeyboardService } from './core/keyboard/keyboard.service';
import { ControllerService } from './core/control/controller.service';
import { UpdateService } from './core/services/update.service';
import { EngineService } from './core/services/engine.service';
import { SettingsService } from './core/services/settings.service';
import { PreferencesService } from './core/services/preferences.service';
import { MixerService } from './core/services/mixer.service';
import { NavigationService } from './core/services/navigation.service';
import { RecordingService } from './core/services/recording.service';
import { FileDropService } from './core/services/file-drop.service';
import { StageZoomService } from './core/services/stage-zoom.service';
import { SoundCardNoticesService } from './core/services/sound-card-notices.service';
import { NoticeView } from './shared/notice/notice';
import { ModelsDialog } from './features/stems/models-dialog';

/** The height whole lanes for two decks take in the decks' own window (CSS px). */
const WHOLE_LANES_PX = 420;

/**
 * Main window layout: title bar, a lane per deck (compact ones for four decks) with the phase
 * strip after the first, the library; and the dialogs. The decks in a window of their own (see
 * `ScreensService`), what plays recalled above the library, and that window the lanes alone,
 * sharing its height. On a phone, its own screens.
 */
@Component({
  selector: 'app-root',
  imports: [
    NgTemplateOutlet,
    SelectPicker,
    Confirm,
    Tooltip,
    Titlebar,
    DeckLane,
    PhaseStrip,
    Library,
    SettingsDialog,
    CommandPalette,
    Phone,
    NowPlaying,
    NoticeView,
    ModelsDialog,
  ],
  templateUrl: './app.html',
  styleUrl: './app.scss',
  host: { '(window:resize)': 'height.set($any($event.target).innerHeight)' },
})
export class App implements OnInit {
  private readonly engine = inject(EngineService);
  private readonly preview = inject(PreviewService);
  private readonly library = inject(LibraryService);
  private readonly settings = inject(SettingsService);
  private readonly soundCardNotices = inject(SoundCardNoticesService);
  private readonly stageZoom = inject(StageZoomService);
  /** Follows the preferences the backend tells from the start. */
  private readonly preferences = inject(PreferencesService);
  private readonly keyboard = inject(KeyboardService);
  protected readonly mixer = inject(MixerService);
  protected readonly decks = this.engine.deckIndexes;
  /** The backend told what the app mirrors. */
  protected readonly ready = this.engine.ready;
  private readonly controllers = inject(ControllerService);
  private readonly updates = inject(UpdateService);
  protected readonly navigation = inject(NavigationService);
  protected readonly PHONE = PHONE;
  protected readonly DECKS_WINDOW = DECKS_WINDOW;
  protected readonly screens = inject(ScreensService);
  private readonly window = inject(WindowService);
  private readonly recording = inject(RecordingService);
  private readonly fileDrops = inject(FileDropService);
  protected readonly height = signal(innerHeight);
  /** Four decks, or the decks' own window too short for whole lanes: compact lanes. */
  protected readonly compact = computed(
    () => this.decks().length > 2 || (DECKS_WINDOW && this.height() < WHOLE_LANES_PX),
  );
  /** The library over the whole main window: what plays recalled above it. */
  protected readonly libraryOnly = computed(
    () => this.navigation.libraryOnly() || this.screens.decksApart(),
  );

  constructor() {
    // a touch screen, a phone's or a laptop's, takes several fingers at once
    clickWithEveryFinger();
  }

  async ngOnInit() {
    try {
      // what the others read (the preferences among it) comes with the first status
      await this.engine.connect();
      // the decks' own window: their keys, the rest is the main window's
      await Promise.all(
        DECKS_WINDOW
          ? [this.keyboard.start(), this.stageZoom.start()]
          : [
              this.preview.init(),
              this.library.init(),
              this.settings.init(),
              this.soundCardNotices.start(),
              this.keyboard.start(),
              this.controllers.start(),
              this.screens.init(),
              this.window.init(),
              this.recording.init(),
              this.fileDrops.start(),
            ],
      );
    } catch (err) {
      console.error(err);
    } finally {
      void dismissSplash();
    }
    // a computer's backend looks by itself
    if (PHONE) void this.updates.look();
  }
}

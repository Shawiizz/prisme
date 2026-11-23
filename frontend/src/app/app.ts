import { Component, OnInit, computed, inject, signal } from '@angular/core';
import { PreviewService } from './core/services/preview.service';
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
import { PHONE } from './core/platform';
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
import { NoticeView } from './shared/notice/notice';
import { ModelsDialog } from './features/stems/models-dialog';

/** The height whole lanes for two decks take on a screen below (CSS px). */
const WHOLE_LANES_PX = 420;

/**
 * Main window layout: title bar, a lane per deck (compact ones for four decks) with the
 * phase strip after the first, the library; and the dialogs. Spread over a screen below, the
 * library above and the decks on the screen below. On a phone, its own screens.
 */
@Component({
  selector: 'app-root',
  imports: [
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
  private readonly settings = inject(SettingsService);
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
  private readonly screens = inject(ScreensService);
  private readonly recording = inject(RecordingService);
  private readonly fileDrops = inject(FileDropService);
  protected readonly height = signal(innerHeight);
  /** Four decks, or a screen below too short for whole lanes: compact lanes. */
  protected readonly compact = computed(() => {
    const under = this.under();
    return this.decks().length > 2 || (under !== null && under / this.underZoom() < WHOLE_LANES_PX);
  });
  /** Spread over a screen below: the height the decks have on it (CSS px). */
  protected readonly under = computed(() => {
    const span = this.screens.span();
    return span === null ? null : Math.max(this.height() - span.split, 0);
  });
  /** The scale of the screen below against the main one's (see `ScreensService`). */
  protected readonly underZoom = computed(() => this.screens.span()?.zoom ?? 1);

  constructor() {
    // a touch screen, a phone's or a laptop's, takes several fingers at once
    clickWithEveryFinger();
  }

  async ngOnInit() {
    try {
      // what the others read (the preferences among it) comes with the first status
      await this.engine.connect();
      await Promise.all([
        this.preview.init(),
        this.settings.init(),
        this.keyboard.start(),
        this.controllers.start(),
        this.screens.init(),
        this.recording.init(),
        this.fileDrops.start(),
      ]);
    } catch (err) {
      console.error(err);
    } finally {
      void dismissSplash();
    }
    // a computer's backend looks by itself
    if (PHONE) void this.updates.look();
  }
}

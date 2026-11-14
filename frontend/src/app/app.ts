import { Component, OnInit, inject } from '@angular/core';
import { StemsService } from './core/services/stems.service';
import { Titlebar } from './shell/titlebar/titlebar';
import { DeckLane } from './features/deck/deck-lane/deck-lane';
import { PhaseStrip } from './features/deck/phase-strip/phase-strip';
import { Library } from './features/library/library';
import { SettingsDialog } from './features/settings/settings-dialog';
import { CommandPalette } from './features/command-palette/command-palette';
import { SelectPicker } from './shared/select-picker/select-picker';
import { Phone } from './features/phone/phone';
import { PHONE } from './core/platform';
import { KeyboardService } from './core/keyboard/keyboard.service';
import { ControllerService } from './core/control/controller.service';
import { UpdateService } from './core/services/update.service';
import { EngineService } from './core/services/engine.service';
import { LibraryService } from './core/services/library.service';
import { SettingsService } from './core/services/settings.service';
import { PreferencesService } from './core/services/preferences.service';
import { MixerService } from './core/services/mixer.service';
import { NavigationService } from './core/services/navigation.service';

/**
 * Main window layout: title bar, a lane per deck (compact ones for four decks) with the
 * phase strip after the first, the library; and the dialogs. On a phone, its own screens.
 */
@Component({
  selector: 'app-root',
  imports: [
    SelectPicker,
    Titlebar,
    DeckLane,
    PhaseStrip,
    Library,
    SettingsDialog,
    CommandPalette,
    Phone,
  ],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit {
  private readonly engine = inject(EngineService);
  private readonly library = inject(LibraryService);
  private readonly stems = inject(StemsService);
  private readonly settings = inject(SettingsService);
  private readonly preferences = inject(PreferencesService);
  private readonly keyboard = inject(KeyboardService);
  protected readonly mixer = inject(MixerService);
  protected readonly decks = this.engine.deckIndexes;
  private readonly controllers = inject(ControllerService);
  private readonly updates = inject(UpdateService);
  protected readonly navigation = inject(NavigationService);
  protected readonly PHONE = PHONE;

  async ngOnInit() {
    await this.preferences.load();
    const [mixer] = await Promise.all([
      this.engine.connect(),
      this.library.init(),
      this.stems.init(),
      this.settings.init(),
      this.keyboard.start(),
      this.controllers.start(),
    ]);
    this.mixer.apply(mixer);
    this.updates.look();
  }
}

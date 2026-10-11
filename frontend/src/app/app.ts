import { Component, OnInit, inject } from '@angular/core';
import { Titlebar } from './shell/titlebar/titlebar';
import { DeckCard } from './features/deck/deck-card/deck-card';
import { DeckWaveform } from './features/deck/deck-waveform/deck-waveform';
import { PhaseStrip } from './features/deck/phase-strip/phase-strip';
import { DeckControls } from './features/deck/deck-controls/deck-controls';
import { DeckPads } from './features/deck/deck-pads/deck-pads';
import { Mixer } from './features/mixer/mixer';
import { Library } from './features/library/library';
import { SettingsDialog } from './features/settings/settings-dialog';
import { CommandPalette } from './features/command-palette/command-palette';
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
 * Main window layout: title bar, the two decks with the phase strip between them, the
 * controls with the mixer, the library; and the dialogs.
 */
@Component({
  selector: 'app-root',
  imports: [
    Titlebar,
    DeckCard,
    DeckWaveform,
    PhaseStrip,
    DeckControls,
    DeckPads,
    Mixer,
    Library,
    SettingsDialog,
    CommandPalette,
  ],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit {
  private readonly engine = inject(EngineService);
  private readonly library = inject(LibraryService);
  private readonly settings = inject(SettingsService);
  private readonly preferences = inject(PreferencesService);
  private readonly keyboard = inject(KeyboardService);
  protected readonly mixer = inject(MixerService);
  private readonly controllers = inject(ControllerService);
  private readonly updates = inject(UpdateService);
  protected readonly navigation = inject(NavigationService);

  async ngOnInit() {
    await this.preferences.load();
    const [mixer] = await Promise.all([
      this.engine.connect(),
      this.library.init(),
      this.settings.refresh(),
      this.keyboard.start(),
      this.controllers.start(),
    ]);
    this.mixer.apply(mixer);
    this.updates.look();
  }
}

import { Component, OnInit, inject, signal } from '@angular/core';
import { Titlebar } from './shell/titlebar/titlebar';
import { DeckCard } from './features/deck/deck-card/deck-card';
import { DeckWaveform } from './features/deck/deck-waveform/deck-waveform';
import { DeckControls } from './features/deck/deck-controls/deck-controls';
import { DeckPads } from './features/deck/deck-pads/deck-pads';
import { Mixer } from './features/mixer/mixer';
import { Library } from './features/library/library';
import { SettingsDialog } from './features/settings/settings-dialog';
import { ShortcutsDialog } from './features/shortcuts/shortcuts-dialog';
import { TurntablesDialog } from './features/turntables/turntables-dialog';
import { KeyboardService } from './core/keyboard/keyboard.service';
import { EngineService } from './core/services/engine.service';
import { LibraryService } from './core/services/library.service';
import { SettingsService } from './core/services/settings.service';
import { PreferencesService } from './core/services/preferences.service';
import { MixerService } from './core/services/mixer.service';

/** Main window layout: title bar, two deck rows, controls with the mixer, library. */
@Component({
  selector: 'app-root',
  imports: [Titlebar, DeckCard, DeckWaveform, DeckControls, DeckPads, Mixer, Library, SettingsDialog, ShortcutsDialog, TurntablesDialog],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit {
  private readonly engine = inject(EngineService);
  private readonly library = inject(LibraryService);
  private readonly settings = inject(SettingsService);
  private readonly preferences = inject(PreferencesService);
  protected readonly keyboard = inject(KeyboardService);
  private readonly mixer = inject(MixerService);
  protected readonly settingsOpen = signal(false);
  protected readonly turntablesOpen = signal(false);

  async ngOnInit() {
    await this.preferences.load();
    const [mixer] = await Promise.all([
      this.engine.connect(),
      this.library.init(),
      this.settings.refresh(),
      this.keyboard.start(),
    ]);
    this.mixer.apply(mixer);
  }
}

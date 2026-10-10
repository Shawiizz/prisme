import { Component, OnInit, inject, signal } from '@angular/core';
import { Titlebar } from './shell/titlebar/titlebar';
import { DeckCard } from './features/deck/deck-card/deck-card';
import { DeckWaveform } from './features/deck/deck-waveform/deck-waveform';
import { DeckControls } from './features/deck/deck-controls/deck-controls';
import { Mixer } from './features/mixer/mixer';
import { Library } from './features/library/library';
import { SettingsDialog } from './features/settings/settings-dialog';
import { EngineService } from './core/services/engine.service';
import { LibraryService } from './core/services/library.service';
import { SettingsService } from './core/services/settings.service';

/** Main window layout: title bar, two deck rows, controls with the mixer, library. */
@Component({
  selector: 'app-root',
  imports: [Titlebar, DeckCard, DeckWaveform, DeckControls, Mixer, Library, SettingsDialog],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit {
  private readonly engine = inject(EngineService);
  private readonly library = inject(LibraryService);
  private readonly settings = inject(SettingsService);
  protected readonly settingsOpen = signal(false);

  async ngOnInit() {
    await Promise.all([this.engine.connect(), this.library.init(), this.settings.refresh()]);
  }
}

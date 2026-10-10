import { Component, inject, output } from '@angular/core';
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
  readonly openSettings = output<void>();

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

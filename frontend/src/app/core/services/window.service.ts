import { Injectable, signal } from '@angular/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

/** The window of the page, full screen or not as the DJ switches it (its key, F11). */
@Injectable({ providedIn: 'root' })
export class WindowService {
  readonly fullscreen = signal(false);

  /** Follows the window as the system resizes it. */
  async init(): Promise<void> {
    const window = getCurrentWindow();
    await window.onResized(() => void this.look());
    await this.look();
  }

  async toggleFullscreen(): Promise<void> {
    const window = getCurrentWindow();
    await window.setFullscreen(!(await window.isFullscreen()));
    await this.look();
  }

  private async look() {
    this.fullscreen.set(await getCurrentWindow().isFullscreen());
  }
}

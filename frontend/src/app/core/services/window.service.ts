import { Injectable, signal } from '@angular/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

/** The window of the page, full screen or not as the DJ switches it (its key, F11). */
@Injectable({ providedIn: 'root' })
export class WindowService {
  readonly fullscreen = signal(false);
  /** Whether the window filled its screen before it went full screen: it does at the start. */
  private maximizedBefore = true;

  /** Follows the window as the system resizes it. */
  async init(): Promise<void> {
    const window = getCurrentWindow();
    await window.onResized(() => void this.look());
    await this.look();
  }

  /** Full screen, or back as the window was; a window filling its screen is let go first, or
   * Windows keeps its size full screen, the taskbar's place left empty. */
  async toggleFullscreen(): Promise<void> {
    const window = getCurrentWindow();
    if (await window.isFullscreen()) {
      await window.setFullscreen(false);
      if (this.maximizedBefore) await window.maximize();
    } else {
      this.maximizedBefore = await window.isMaximized();
      if (this.maximizedBefore) await window.unmaximize();
      await window.setFullscreen(true);
    }
    await this.look();
  }

  private async look() {
    this.fullscreen.set(await getCurrentWindow().isFullscreen());
  }
}

import { Injectable } from '@angular/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';

/** The size the decks are drawn for (CSS px): their window is zoomed to fill its screen as a
 * screen of this size would be filled, never below it. */
const STAGE_WIDTH = 1500;
const STAGE_HEIGHT = 860;
/** The most the decks are zoomed. */
const MOST = 2;

/**
 * The decks' own window, full screen on a screen of any size: zoomed as the browser zooms a
 * page, so that its keys, knobs and figures keep the size they have on a laptop's screen in
 * proportion to the screen, the waveforms no taller than a lane needs, rather than tiny
 * controls beside waveforms stretched over a large screen. Drawn sharp at any zoom.
 */
@Injectable({ providedIn: 'root' })
export class StageZoomService {
  private zoom = 1;

  start() {
    void this.fit();
    window.addEventListener('resize', () => void this.fit());
  }

  /** The zoom of the screen's size: at zoom 1 the page is as wide as it is wide times the zoom. */
  private async fit() {
    const width = window.innerWidth * this.zoom;
    const height = window.innerHeight * this.zoom;
    const fitting = Math.min(width / STAGE_WIDTH, height / STAGE_HEIGHT);
    const zoom = Math.round(Math.min(Math.max(fitting, 1), MOST) * 100) / 100;
    if (zoom === this.zoom) return;
    this.zoom = zoom;
    await getCurrentWebview().setZoom(zoom);
  }
}

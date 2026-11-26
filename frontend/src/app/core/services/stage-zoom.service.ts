import { Injectable, Injector, effect, inject, untracked } from '@angular/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { EngineService } from './engine.service';

/** The size the decks are drawn for (CSS px): their window is zoomed to fill its screen as a
 * screen of this size would be filled, never below it. Four decks are drawn for a wider one:
 * their channels side by side leave the pads less room. */
const STAGE_WIDTH = 1700;
const STAGE_WIDTH_FOUR = 2000;
const STAGE_HEIGHT = 860;
/** The most the decks are zoomed. */
const MOST = 2;

/**
 * The decks' own window, full screen on a screen of any size: zoomed as the browser zooms a
 * page, so that its keys, knobs and figures keep the size they have on a laptop's screen in
 * proportion to the screen, rather than tiny controls beside waveforms stretched over a large
 * screen. Drawn sharp at any zoom.
 */
@Injectable({ providedIn: 'root' })
export class StageZoomService {
  private readonly engine = inject(EngineService);
  private readonly injector = inject(Injector);
  private zoom = 1;

  /** From the page at its own size (a page loaded again keeps the zoom it had), then as the
   * window is resized or the decks are more or fewer. */
  async start() {
    await getCurrentWebview().setZoom(1);
    window.addEventListener('resize', () => void this.fit());
    effect(
      () => {
        this.engine.deckIndexes();
        untracked(() => void this.fit());
      },
      { injector: this.injector },
    );
  }

  /** The zoom of the screen's size: at zoom 1 the page is as wide as it is wide times the zoom. */
  private async fit() {
    const wide = this.engine.deckIndexes().length > 2 ? STAGE_WIDTH_FOUR : STAGE_WIDTH;
    const width = window.innerWidth * this.zoom;
    const height = window.innerHeight * this.zoom;
    const fitting = Math.min(width / wide, height / STAGE_HEIGHT);
    const zoom = Math.round(Math.min(Math.max(fitting, 1), MOST) * 100) / 100;
    if (zoom === this.zoom) return;
    this.zoom = zoom;
    await getCurrentWebview().setZoom(zoom);
  }
}

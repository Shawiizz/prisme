import { ErrorHandler, Injectable } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';

/** The most errors told to the backend in a minute: one thrown at each frame would fill the log. */
const PER_MINUTE = 20;
const MINUTE_MS = 60_000;
/** How long an error told is not told again: long enough not to repeat one thrown at each
 * frame, short enough that one met early in a long set is told when it comes back. */
const AGAIN_MS = 10 * MINUTE_MS;

/** Tells the backend's log the errors the page meets, besides the console. */
@Injectable()
export class PageErrors implements ErrorHandler {
  /** When each error was last told. */
  private readonly told = new Map<string, number>();

  handleError(error: unknown): void {
    console.error(error);
    const message = error instanceof Error ? (error.stack ?? error.message) : String(error);
    const now = Date.now();
    for (const [told, at] of this.told) if (now - at >= AGAIN_MS) this.told.delete(told);
    if (this.told.has(message)) return;
    const lastMinute = [...this.told.values()].filter((at) => now - at < MINUTE_MS);
    if (lastMinute.length >= PER_MINUTE) return;
    this.told.set(message, now);
    invoke('page_error', { message }).catch(() => {});
  }
}

import { ErrorHandler, Injectable } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';

/** The most errors of one page told to the backend: one thrown at each frame would fill the log. */
const MOST = 20;

/** Tells the backend's log each error the page meets, once, besides the console. */
@Injectable()
export class PageErrors implements ErrorHandler {
  private readonly told = new Set<string>();

  handleError(error: unknown): void {
    console.error(error);
    const message = error instanceof Error ? (error.stack ?? error.message) : String(error);
    if (this.told.size >= MOST || this.told.has(message)) return;
    this.told.add(message);
    invoke('page_error', { message }).catch(() => {});
  }
}

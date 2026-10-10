import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { HistoryPlay, HistorySession } from '../models/library';

/** The history of the sets, updated as tracks get played (`history-changed`). */
@Injectable({ providedIn: 'root' })
export class HistoryService {
  readonly sessions = signal<HistorySession[]>([]);
  /** Incremented whenever a play is recorded. */
  readonly changes = signal(0);

  async init(): Promise<void> {
    await listen('history-changed', async () => {
      await this.refresh();
      this.changes.update((n) => n + 1);
    });
    await this.refresh();
  }

  async refresh(): Promise<void> {
    this.sessions.set(await invoke<HistorySession[]>('history_sessions'));
  }

  plays(id: number): Promise<HistoryPlay[]> {
    return invoke<HistoryPlay[]>('history_plays', { id });
  }

  async delete(id: number): Promise<void> {
    await invoke('delete_history_session', { id });
    await this.refresh();
  }

  /** Asks where to save the tracklist (CSV); `false` if cancelled. */
  async export(session: HistorySession): Promise<boolean> {
    const path = await save({
      defaultPath: `Set ${fileDate(session.startedAtMs)}.csv`,
      filters: [{ name: 'CSV', extensions: ['csv'] }],
    });
    if (!path) return false;
    await invoke('export_history', { id: session.id, path });
    return true;
  }
}

/** `2026-10-10 21h30`: sortable and valid in a file name. */
function fileDate(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}h${two(d.getMinutes())}`;
}

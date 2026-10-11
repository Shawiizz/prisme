import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { t } from '../i18n/i18n';
import { HistoryPlay, HistorySession } from '../models/library';

/** A set that started longer ago than this is over. */
const TONIGHT_MS = 12 * 3600 * 1000;

/** The history of the sets, updated as tracks get played (`history-changed`). */
@Injectable({ providedIn: 'root' })
export class HistoryService {
  readonly sessions = signal<HistorySession[]>([]);
  /** Incremented whenever a play is recorded. */
  readonly changes = signal(0);
  /** Tracks played in the set going on: the last one, if it started less than 12 hours ago. */
  readonly playedTonight = signal<ReadonlySet<string>>(new Set());

  async init(): Promise<void> {
    await listen('history-changed', async () => {
      await this.refresh();
      this.changes.update((n) => n + 1);
    });
    await this.refresh();
  }

  async refresh(): Promise<void> {
    const sessions = await invoke<HistorySession[]>('history_sessions');
    this.sessions.set(sessions);
    const last = sessions.reduce<HistorySession | null>(
      (latest, s) => (!latest || s.startedAtMs > latest.startedAtMs ? s : latest),
      null,
    );
    const tonight = last && Date.now() - last.startedAtMs < TONIGHT_MS;
    const plays = tonight ? await this.plays(last.id) : [];
    this.playedTonight.set(new Set(plays.map((p) => p.path)));
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
      defaultPath: t('Set {date}.csv', { date: fileDate(session.startedAtMs) }),
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

import { Injectable } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { t } from '../i18n/i18n';
import type { HistorySession } from '../models/backend';

/** The history of the sets: a set's tracklist written out. */
@Injectable({ providedIn: 'root' })
export class HistoryService {
  /** Asks where to save the tracklist (CSV); `false` if cancelled. */
  async export(session: HistorySession): Promise<boolean> {
    const path = await save({
      get defaultPath() {
        return t('Set {date}.csv', { date: fileDate(session.startedAtMs) });
      },
      filters: [{ name: 'CSV', extensions: ['csv'] }],
    });
    if (!path) return false;
    await invoke('export_history', { id: session.id, path });
    return true;
  }
}

/** `2026-10-10 21h30`: sortable and valid in a file name. */
export function fileDate(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}h${two(d.getMinutes())}`;
}

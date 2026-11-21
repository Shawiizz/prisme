import { Injectable, inject } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { t } from '../i18n/i18n';
import { EngineService } from './engine.service';
import { fileDate } from './history.service';
import type { ExportSummary } from '../models/backend';

/** A list of the library (see `ListDto`); a set of the history with when it started, for its name. */
export type ListRef =
  | { kind: 'crate'; id: number }
  | { kind: 'prepare' }
  | { kind: 'session'; id: number; startedAtMs: number };

/**
 * Exports for other DJ software: a list as an M3U playlist, the library or chosen lists as
 * rekordbox's XML.
 */
@Injectable({ providedIn: 'root' })
export class ExportService {
  private readonly engine = inject(EngineService);

  /** The name a list goes by in an export: a set by when it started, sortable. */
  name(list: ListRef): string {
    switch (list.kind) {
      case 'crate':
        return this.engine.controls()?.lists.crates.find((c) => c.id === list.id)?.name ?? '';
      case 'prepare':
        return t('Prepare');
      case 'session':
        return setName(list.startedAtMs);
    }
  }

  /** Asks where to save `list` as an M3U playlist; `false` if cancelled. */
  async exportM3u(list: ListRef): Promise<boolean> {
    const name =
      list.kind === 'session'
        ? t('Set {date}', { date: fileDate(list.startedAtMs) })
        : this.name(list);
    const path = await save({
      defaultPath: `${fileName(name)}.m3u8`,
      filters: [{ name: 'M3U', extensions: ['m3u8', 'm3u'] }],
    });
    if (!path) return false;
    await invoke('export_m3u', { list: listDto(list), path });
    return true;
  }

  /**
   * Asks where to save the XML, then writes `lists` (the sets in a History folder) and, with
   * `wholeLibrary`, every track of the library; `null` if cancelled.
   */
  async exportRekordbox(
    wholeLibrary: boolean,
    lists: ListRef[],
  ): Promise<{ path: string; summary: ExportSummary } | null> {
    const path = await save({
      defaultPath: 'Prisme.xml',
      filters: [{ name: 'rekordbox XML', extensions: ['xml'] }],
    });
    if (!path) return null;
    const named = lists.map((list) => ({
      list: listDto(list),
      folder: list.kind === 'session' ? t('History') : null,
      name: this.name(list),
    }));
    const summary = await invoke<ExportSummary>('export_rekordbox', {
      wholeLibrary,
      lists: named,
      path,
    });
    return { path, summary };
  }
}

function listDto(list: ListRef) {
  return list.kind === 'prepare' ? { kind: list.kind } : { kind: list.kind, id: list.id };
}

/** `name` without what a file name cannot hold (a crate imported from folders has slashes). */
export function fileName(name: string): string {
  return name.replace(/[\\/:*?"<>|]/g, '-');
}

/** `2026-11-14 22:00` */
export function setName(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}`;
}

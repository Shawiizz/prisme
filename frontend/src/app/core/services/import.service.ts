import { Injectable } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { t } from '../i18n/i18n';
import type { ImportSummary } from '../models/backend';

/**
 * Import from Serato and rekordbox. Nothing already set in Prisme is overwritten, so
 * importing again is harmless; the track lists update as each track is announced.
 */
@Injectable({ providedIn: 'root' })
export class ImportService {
  /** The `_Serato_` folder of the music folder, if any. */
  seratoFolder(): Promise<string | null> {
    return invoke<string | null>('serato_folder');
  }

  /** Asks for a `_Serato_` folder; `null` if cancelled. */
  async pickSeratoFolder(): Promise<string | null> {
    return open({ directory: true, title: t('The _Serato_ folder') });
  }

  async importSerato(seratoDir: string | null): Promise<ImportSummary> {
    return this.finish(await invoke<ImportSummary>('import_serato', { seratoDir }));
  }

  /** Asks for a rekordbox XML export and imports it; `null` if cancelled. */
  async importRekordbox(): Promise<ImportSummary | null> {
    const path = await open({ filters: [{ name: 'rekordbox XML', extensions: ['xml'] }] });
    if (!path) return null;
    return this.finish(await invoke<ImportSummary>('import_rekordbox', { path }));
  }

  private async finish(summary: ImportSummary): Promise<ImportSummary> {
    return summary;
  }
}

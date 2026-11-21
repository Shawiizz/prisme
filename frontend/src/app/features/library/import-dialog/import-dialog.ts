import { Component, inject, output, signal } from '@angular/core';
import { ImportService } from '../../../core/services/import.service';
import { TPipe } from '../../../core/i18n/t.pipe';
import { errorText } from '../../../core/errors';
import { Dismiss, overlay } from '../../../shared/overlay';
import type { ImportSummary } from '../../../core/models/backend';

/** Import from Serato or rekordbox, with what it brought. */
@Component({
  selector: 'app-import-dialog',
  templateUrl: './import-dialog.html',
  styleUrl: './import-dialog.scss',
  imports: [TPipe, Dismiss],
})
export class ImportDialog {
  readonly closed = output();

  private readonly imports = inject(ImportService);
  protected readonly seratoFolder = signal<string | null>(null);
  protected readonly busy = signal(false);
  protected readonly summary = signal<ImportSummary | null>(null);
  protected readonly error = signal<string | null>(null);

  constructor() {
    this.imports.seratoFolder().then((dir) => this.seratoFolder.set(dir));
    overlay(
      () => true,
      () => this.close(),
    );
  }

  /** Closes, unless an import runs. */
  protected close() {
    if (!this.busy()) this.closed.emit();
  }

  async pickSeratoFolder() {
    const dir = await this.imports.pickSeratoFolder();
    if (dir) this.seratoFolder.set(dir);
  }

  importSerato() {
    return this.run(() => this.imports.importSerato(this.seratoFolder()));
  }

  importRekordbox() {
    return this.run(() => this.imports.importRekordbox());
  }

  private async run(action: () => Promise<ImportSummary | null>) {
    this.busy.set(true);
    this.error.set(null);
    try {
      const summary = await action();
      if (summary) this.summary.set(summary);
    } catch (e) {
      this.error.set(errorText(e));
    } finally {
      this.busy.set(false);
    }
  }
}

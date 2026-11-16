import { Component, computed, inject, output, signal } from '@angular/core';
import { CratesService } from '../../../core/services/crates.service';
import { ExportService, ExportSummary, ListRef } from '../../../core/services/export.service';
import { HistoryService } from '../../../core/services/history.service';
import { PrepareService } from '../../../core/services/prepare.service';
import { Text, t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { errorText } from '../../../core/errors';
import { Dismiss, overlay } from '../../../shared/overlay';

/** A list the export can carry, as the dialog shows it. */
interface Choice {
  key: string;
  list: ListRef;
  name: string;
  count: number;
}

interface Section {
  key: string;
  title: Text;
  choices: Choice[];
}

/** The library, or chosen lists, as a rekordbox XML file. */
@Component({
  selector: 'app-export-dialog',
  templateUrl: './export-dialog.html',
  styleUrl: './export-dialog.scss',
  imports: [TPipe, Dismiss],
})
export class ExportDialog {
  readonly closed = output();

  private readonly exports = inject(ExportService);
  private readonly crates = inject(CratesService).crates;
  private readonly sessions = inject(HistoryService).sessions;
  private readonly prepare = inject(PrepareService).list;

  /** Every track and every list, or the lists chosen. */
  protected readonly whole = signal(true);
  protected readonly chosen = signal<ReadonlySet<string>>(new Set());
  protected readonly busy = signal(false);
  protected readonly done = signal<{ path: string; summary: ExportSummary } | null>(null);
  protected readonly error = signal<string | null>(null);

  /** The lists by kind, in the order of the XML: the crates, the prepare list, the sets. */
  protected readonly sections = computed<Section[]>(() => [
    {
      key: 'crates',
      title: () => t('Crates'),
      choices: this.crates().map((c) =>
        this.choice(`crate:${c.id}`, { kind: 'crate', id: c.id }, c.trackCount),
      ),
    },
    {
      key: 'prepare',
      title: () => t('Prepare'),
      choices: [this.choice('prepare', { kind: 'prepare' }, this.prepare().length)],
    },
    {
      key: 'history',
      title: () => t('History'),
      choices: this.sessions().map((s) =>
        this.choice(
          `session:${s.id}`,
          { kind: 'session', id: s.id, startedAtMs: s.startedAtMs },
          s.playCount,
        ),
      ),
    },
  ]);
  /** The lists that go. */
  private readonly lists = computed(() =>
    this.sections()
      .flatMap((s) => s.choices)
      .filter((c) => this.whole() || this.chosen().has(c.key))
      .map((c) => c.list),
  );
  protected readonly empty = computed(() => this.lists().length === 0);

  constructor() {
    overlay(
      () => true,
      () => this.close(),
    );
  }

  /** Closes, unless an export runs. */
  protected close() {
    if (!this.busy()) this.closed.emit();
  }

  protected toggle(key: string) {
    this.chosen.update((chosen) => {
      const next = new Set(chosen);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }

  protected async export() {
    this.busy.set(true);
    this.error.set(null);
    try {
      const done = await this.exports.exportRekordbox(this.whole(), this.lists());
      if (done) this.done.set(done);
    } catch (e) {
      this.error.set(errorText(e));
    } finally {
      this.busy.set(false);
    }
  }

  private choice(key: string, list: ListRef, count: number): Choice {
    return { key, list, name: this.exports.name(list), count };
  }
}

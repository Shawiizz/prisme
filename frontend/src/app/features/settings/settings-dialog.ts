import { Component, OnInit, computed, inject, output, signal } from '@angular/core';
import { SettingsService } from '../../core/services/settings.service';
import { EngineService } from '../../core/services/engine.service';
import { AudioSettings } from '../../core/models/audio';
import { VinylSyncMode } from '../../core/models/engine';

/** Settings: audio devices, DVS input, timecode format, simulated turntable, sync behaviour. */
@Component({
  selector: 'app-settings-dialog',
  templateUrl: './settings-dialog.html',
  styleUrl: './settings-dialog.scss',
})
export class SettingsDialog implements OnInit {
  protected readonly service = inject(SettingsService);
  private readonly engine = inject(EngineService);
  readonly closed = output<void>();

  protected readonly draft = signal<AudioSettings | null>(null);
  protected readonly vinylSync = signal<VinylSyncMode>(this.engine.vinylSync());
  protected readonly busy = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly outputs = computed(() => this.service.devices().filter((d) => d.kind === 'output'));
  protected readonly inputs = computed(() => this.service.devices().filter((d) => d.kind === 'input'));

  async ngOnInit() {
    await this.service.refresh();
    this.draft.set(structuredClone(this.service.settings()));
  }

  update(changes: Partial<AudioSettings>) {
    const d = this.draft();
    if (d) this.draft.set({ ...d, ...changes });
  }

  /** Restarts the audio only if the audio settings changed. */
  async apply() {
    const d = this.draft();
    if (!d) return;
    this.engine.vinylSync.set(this.vinylSync());
    this.busy.set(true);
    this.error.set(null);
    try {
      if (JSON.stringify(d) !== JSON.stringify(this.service.settings())) {
        await this.service.apply(d);
      }
      this.closed.emit();
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.busy.set(false);
    }
  }

  /** Select value -> setting (empty string = default / none). */
  orNull(v: string): string | null {
    return v === '' ? null : v;
  }
}

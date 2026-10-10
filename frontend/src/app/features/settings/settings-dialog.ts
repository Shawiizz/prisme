import { Component, OnInit, computed, inject, output, signal } from '@angular/core';
import { SettingsService } from '../../core/services/settings.service';
import { PreferencesService } from '../../core/services/preferences.service';
import { AudioSettings } from '../../core/models/audio';
import { DjPreferences, PITCH_RANGES } from '../../core/models/preferences';

/** Settings: audio devices, DVS input, timecode format, simulated turntable, DJ preferences. */
@Component({
  selector: 'app-settings-dialog',
  templateUrl: './settings-dialog.html',
  styleUrl: './settings-dialog.scss',
})
export class SettingsDialog implements OnInit {
  protected readonly service = inject(SettingsService);
  private readonly preferences = inject(PreferencesService);
  readonly closed = output<void>();

  protected readonly pitchRanges = PITCH_RANGES;
  protected readonly draft = signal<AudioSettings | null>(null);
  protected readonly dj = signal<DjPreferences>(this.preferences.dj());
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

  updateDj(changes: Partial<DjPreferences>) {
    this.dj.set({ ...this.dj(), ...changes });
  }

  /** Restarts the audio only if the audio settings changed. */
  async apply() {
    const d = this.draft();
    if (!d) return;
    this.busy.set(true);
    this.error.set(null);
    try {
      await this.preferences.updateDj(this.dj());
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

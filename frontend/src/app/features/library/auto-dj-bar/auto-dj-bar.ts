import { Component, booleanAttribute, computed, inject, input } from '@angular/core';
import { AutoDjService } from '../../../core/services/auto-dj.service';
import { PreferencesService } from '../../../core/services/preferences.service';
import { EngineService } from '../../../core/services/engine.service';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';
import { formatClock } from '../../../shared/time';

/**
 * The Auto DJ over the prepare list: its key (lit while it plays the list), what it does,
 * what comes next and when the mix into it starts, fade now and skip, and how long a mix
 * lasts. Over the table on a computer; on a phone, rows for a finger.
 */
@Component({
  selector: 'app-auto-dj-bar',
  templateUrl: './auto-dj-bar.html',
  styleUrl: './auto-dj-bar.scss',
  host: { '[class.phone]': 'phone()' },
  imports: [TPipe],
})
export class AutoDjBar {
  /** Laid out for a finger. */
  readonly phone = input(false, { transform: booleanAttribute });

  protected readonly autoDj = inject(AutoDjService);
  private readonly engine = inject(EngineService);
  /** The lengths the mixes are offered at. */
  protected readonly choices = inject(PreferencesService).choices;

  /** What the Auto DJ does, in a line. */
  protected readonly line = computed(() => {
    const status = this.autoDj.status();
    const reason = this.autoDj.reason();
    if (!this.autoDj.available() && status.state === 'off') {
      return t('The Auto DJ fades with the software mixer: choose it in the settings');
    }
    if (status.state === 'paused') return `${reason} · ${t('press Auto DJ to go on')}`;
    if (status.state === 'mixing' && status.incoming !== null) {
      const title = this.engine.tracks()[status.incoming]?.title ?? '';
      return t('Mixing into {title}', { title });
    }
    if (status.state === 'off') {
      return reason ?? t('Plays the prepare list, each track mixed into the next');
    }
    const next = this.autoDj.next();
    if (!next) return t('Nothing next: the list ends with this track');
    const title = next.title;
    const mixIn = this.autoDj.mixIn();
    return mixIn === null
      ? t('Next: {title}', { title })
      : t('Next: {title} · mix in {time}', { title, time: formatClock(mixIn) });
  });

  /** Something went wrong while it plays on: said beside what it does. */
  protected readonly warning = computed(() =>
    this.autoDj.running() ? this.autoDj.reason() : null,
  );

  protected setBeats(beats: number) {
    this.autoDj.setFadeBeats(beats);
  }

  protected setSeconds(seconds: number) {
    this.autoDj.setFadeSeconds(seconds);
  }
}

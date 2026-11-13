import { Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { BACKEND_NAMES } from '../../core/models/audio';
import { EngineService } from '../../core/services/engine.service';
import { LibraryService } from '../../core/services/library.service';
import { NavigationService } from '../../core/services/navigation.service';
import { RecordingService } from '../../core/services/recording.service';
import { SettingsService } from '../../core/services/settings.service';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { decimal, formatClock } from '../../shared/time';
import { LibrarySidebar } from '../library/library-sidebar/library-sidebar';
import { PhoneControls } from './phone-controls';
import { PhoneDeck } from './phone-deck';
import { PhoneTracks } from './phone-tracks';

type Tab = 'mix' | 'controls' | 'crates';

/**
 * Prisme on a phone. With a controller the hands are on it: the Mix screen shows what it
 * has not, the decks one under the other and the tracks to load. Without one, Controls
 * puts a deck under the fingers. Crates leads to the lists of the library. Held sideways,
 * the decks take the width and the tracks the side.
 */
@Component({
  selector: 'app-phone',
  templateUrl: './phone.html',
  styleUrl: './phone.scss',
  imports: [LibrarySidebar, PhoneControls, PhoneDeck, PhoneTracks, TPipe],
})
export class Phone {
  protected readonly engine = inject(EngineService);
  protected readonly navigation = inject(NavigationService);
  protected readonly recording = inject(RecordingService);
  private readonly library = inject(LibraryService);
  private readonly settings = inject(SettingsService);

  protected readonly decks = this.engine.deckIndexes;
  protected readonly tab = signal<Tab>('mix');
  protected readonly tabs: { id: Tab; label: string }[] = [
    { id: 'mix', label: t('Mix') },
    { id: 'controls', label: t('Controls') },
    { id: 'crates', label: t('Crates') },
  ];

  /** The sound card and how long it waits: `AAudio · DDJ-FLX2 · 5.3 ms`. */
  protected readonly audio = computed(() => {
    const info = this.settings.info();
    if (!this.engine.audioRunning() || !info) return null;
    const frames = this.engine.status()?.health?.periodFrames;
    const period = frames ? [`${decimal((frames / info.sampleRate) * 1000, 1)} ms`] : [];
    return [BACKEND_NAMES[info.backend], info.device, ...period].join(' · ');
  });

  protected readonly clock = formatClock;

  constructor() {
    // a list chosen among the crates is shown where the tracks are
    effect(() => {
      this.library.source();
      this.library.listing();
      if (untracked(this.tab) === 'crates') this.tab.set('mix');
    });
  }

  /** Records what the phone offers first: the master of its software mixer. */
  protected record() {
    const [first] = this.recording.options();
    if (first) this.recording.start(first.source);
  }
}

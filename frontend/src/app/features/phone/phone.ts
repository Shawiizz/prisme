import { Component, computed, effect, inject, signal } from '@angular/core';
import { ControllerService } from '../../core/control/controller.service';
import { clickWithEveryFinger } from '../../core/every-finger';
import { BACKEND_NAMES } from '../../core/models/audio';
import { EngineService } from '../../core/services/engine.service';
import { LibraryService } from '../../core/services/library.service';
import { NavigationService } from '../../core/services/navigation.service';
import { RecordingService } from '../../core/services/recording.service';
import { SettingsService } from '../../core/services/settings.service';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { decimal, formatClock } from '../../shared/time';
import { PhoneDecks } from './phone-decks';
import { PhoneLibrary } from './phone-library';
import { PhoneSettings } from './phone-settings';

type Tab = 'decks' | 'library' | 'settings';

/**
 * Prisme on a phone: the decks, the library and the settings, a screen each. The decks
 * follow what is plugged in: a controller, or records through a sound card, and they only
 * show what is read from afar; nothing, and their controls come under the thumbs (the key
 * at the top says otherwise).
 */
@Component({
  selector: 'app-phone',
  templateUrl: './phone.html',
  styleUrl: './phone.scss',
  imports: [PhoneDecks, PhoneLibrary, PhoneSettings, TPipe],
})
export class Phone {
  protected readonly engine = inject(EngineService);
  protected readonly library = inject(LibraryService);
  protected readonly navigation = inject(NavigationService);
  protected readonly recording = inject(RecordingService);
  private readonly settings = inject(SettingsService);
  private readonly controllers = inject(ControllerService);

  protected readonly tab = signal<Tab>('decks');
  protected readonly tabs: { id: Tab; label: string }[] = [
    { id: 'decks', label: t('Decks') },
    { id: 'library', label: t('Library') },
    { id: 'settings', label: t('Settings') },
  ];

  /** The sound card and how long it waits: `AAudio · DDJ-FLX2 · 5.3 ms`. */
  protected readonly audio = computed(() => {
    const info = this.settings.info();
    if (!this.engine.audioRunning() || !info) return null;
    const frames = this.engine.status()?.health?.periodFrames;
    const period = frames ? [`${decimal((frames / info.sampleRate) * 1000, 1)} ms`] : [];
    return [BACKEND_NAMES[info.backend], info.device, ...period].join(' · ');
  });

  /** The hands are elsewhere: on a controller in use, or on the records that drive a deck. */
  private readonly hardware = computed(
    () =>
      this.controllers.controllers().some((c) => c.enabled && c.plugged) ||
      this.engine.deckIndexes().some((d) => this.engine.decks()[d].vinylEnabled),
  );
  /** What the DJ chose instead, with the key at the top. */
  private readonly chosen = signal<boolean | null>(null);
  protected readonly touch = computed(() => this.chosen() ?? !this.hardware());

  protected readonly clock = formatClock;

  constructor() {
    clickWithEveryFinger();
    // a page of the settings asked for from elsewhere is shown on their screen
    effect(() => {
      if (this.navigation.settings()) this.tab.set('settings');
    });
  }

  protected toggleTouch() {
    this.chosen.set(!this.touch());
  }

  /** Records what the phone offers first: the master of its software mixer. */
  protected record() {
    const [first] = this.recording.options();
    if (first) this.recording.start(first.source);
  }
}

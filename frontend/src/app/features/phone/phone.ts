import { invoke } from '@tauri-apps/api/core';
import { Component, DestroyRef, computed, effect, inject, signal } from '@angular/core';
import { ControllerService } from '../../core/control/controller.service';
import { closedByBack } from '../../core/back';
import { BACKEND_NAMES } from '../../core/models/audio';
import { ANDROID } from '../../core/platform';
import { EngineService } from '../../core/services/engine.service';
import { PreferencesService } from '../../core/services/preferences.service';
import { LibraryService } from '../../core/services/library.service';
import { NavigationService } from '../../core/services/navigation.service';
import { RecordingService } from '../../core/services/recording.service';
import { SettingsService } from '../../core/services/settings.service';
import { Text, t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { errorText } from '../../core/errors';
import { decimal, formatClock } from '../../shared/time';
import { CommandPalette } from '../command-palette/command-palette';
import { PhoneDecks } from './phone-decks';
import { PhoneLibrary } from './phone-library';
import { PhoneRecord } from './phone-record';
import { PhoneSettings } from './phone-settings';

type Tab = 'decks' | 'library' | 'settings';

/**
 * Prisme on a phone: the decks, the library and the settings, a screen each. The decks
 * follow what is plugged in: a controller, or records through a sound card, and they only
 * show what is read from afar; nothing, and their controls come under the thumbs (the key
 * at the top says otherwise, kept from one start to the next).
 */
@Component({
  selector: 'app-phone',
  templateUrl: './phone.html',
  styleUrl: './phone.scss',
  host: { '[class.android]': 'ANDROID' },
  imports: [CommandPalette, PhoneDecks, PhoneLibrary, PhoneRecord, PhoneSettings, TPipe],
})
export class Phone {
  protected readonly ANDROID = ANDROID;
  protected readonly engine = inject(EngineService);
  protected readonly library = inject(LibraryService);
  protected readonly navigation = inject(NavigationService);
  protected readonly recording = inject(RecordingService);
  private readonly settings = inject(SettingsService);
  private readonly controllers = inject(ControllerService);

  protected readonly tab = signal<Tab>('decks');
  /** The choice of what to record is open. */
  protected readonly recordMenu = signal(false);
  protected readonly tabs: { id: Tab; label: Text }[] = [
    {
      id: 'decks',
      label: () => t('Decks'),
    },
    {
      id: 'library',
      label: () => t('Library'),
    },
    {
      id: 'settings',
      label: () => t('Settings'),
    },
  ];

  /** The sound card and how long it waits: `AAudio · DDJ-FLX2 · 5.3 ms`. */
  protected readonly audio = computed(() => {
    const info = this.settings.info();
    if (!this.engine.audioRunning() || !info) return null;
    const period = this.settings.period();
    return [BACKEND_NAMES[info.backend], info.device, ...(period ? [period] : [])].join(' · ');
  });
  /** The sound card chosen is missing, the default device playing meanwhile: its name, '' for
   * the first one its driver finds. */
  protected readonly missing = computed(() => this.settings.info()?.missing ?? null);

  /** The hands are elsewhere: on a controller in use, or on the records that drive a deck. */
  private readonly preferences = inject(PreferencesService);
  private readonly hardware = computed(
    () =>
      this.controllers.controllers().some((c) => c.enabled && c.plugged) ||
      this.engine.deckIndexes().some((d) => this.engine.decks()[d].vinylEnabled),
  );
  /** What the DJ chose instead, with the key at the top. */
  protected readonly touch = computed(() => this.preferences.ui().phoneTouch ?? !this.hardware());

  protected readonly clock = formatClock;

  constructor() {
    // the system may stop calling the audio of an application out of sight: Prisme knows
    const hidden = () =>
      invoke('set_hidden', { hidden: document.hidden }).catch((e) =>
        this.engine.error.set(errorText(e)),
      );
    document.addEventListener('visibilitychange', hidden);
    inject(DestroyRef).onDestroy(() => document.removeEventListener('visibilitychange', hidden));
    // a page of the settings asked for from elsewhere is shown on their screen, as the library
    effect(() => {
      if (this.navigation.settings()) this.tab.set('settings');
    });
    effect(() => {
      if (this.navigation.libraryAsked()) this.tab.set('library');
    });
    closedByBack(
      () => this.navigation.palette(),
      () => this.navigation.palette.set(false),
    );
  }

  protected toggleTouch() {
    this.preferences
      .set('phoneTouch', !this.touch())
      .catch((e) => this.engine.error.set(errorText(e)));
  }
}

import { Component, computed, effect, inject, signal, untracked, viewChild } from '@angular/core';
import { ControllerService } from '../../core/control/controller.service';
import { closedByBack } from '../../core/back';
import { BACKEND_NAMES } from '../../core/models/audio';
import { ANDROID } from '../../core/platform';
import { EngineService } from '../../core/services/engine.service';
import { PreferencesService } from '../../core/services/preferences.service';
import { LibraryService } from '../../core/services/library.service';
import { LibrarySight, NavigationService } from '../../core/services/navigation.service';
import { RecordingService } from '../../core/services/recording.service';
import { SettingsService } from '../../core/services/settings.service';
import { Text, t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { errorText } from '../../core/errors';
import { formatClock } from '../../shared/time';
import { CommandPalette } from '../command-palette/command-palette';
import { PhoneDecks } from './phone-decks';
import { PhoneLibrary } from './phone-library';
import { PhoneMoved } from './phone-moved';
import { PhoneRecord } from './phone-record';
import { PhoneSettings } from './phone-settings';
import { PhoneSight } from './phone-sight';

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
  providers: [PhoneSight],
  imports: [
    CommandPalette,
    PhoneDecks,
    PhoneLibrary,
    PhoneMoved,
    PhoneRecord,
    PhoneSettings,
    TPipe,
  ],
})
export class Phone {
  protected readonly ANDROID = ANDROID;
  protected readonly engine = inject(EngineService);
  protected readonly library = inject(LibraryService);
  protected readonly navigation = inject(NavigationService);
  protected readonly recording = inject(RecordingService);
  private readonly settings = inject(SettingsService);
  private readonly controllers = inject(ControllerService);
  private readonly inSight = inject(PhoneSight);

  protected readonly tab = signal<Tab>('decks');
  /** The screen the library was brought over by an action, gone back to once a track is
   * loaded from it; none once the DJ chose the screen. */
  private lentFrom: Tab | null = null;
  private readonly settingsScreen = viewChild(PhoneSettings);
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
  /** The USB card the DJ is asked to let Prisme open, or refused it, in words. */
  protected readonly asked = this.settings.asked;

  /** The hands are elsewhere: on a controller open, or on the records that drive a deck. */
  private readonly preferences = inject(PreferencesService);
  private readonly hardware = computed(
    () =>
      this.controllers.canLearn() ||
      this.engine.deckIndexes().some((d) => this.engine.decks()[d].vinylEnabled),
  );
  /** What the DJ chose instead, with the key at the top. */
  protected readonly touch = computed(() => this.preferences.ui().phoneTouch ?? !this.hardware());

  protected readonly clock = formatClock;

  constructor() {
    // a page of the settings asked for from elsewhere is shown on their screen
    effect(() => {
      if (this.navigation.settings()) untracked(() => this.show('settings'));
    });
    // the library brought into sight by what is done in it, a controller's knob say
    effect(() => {
      const wanted = this.navigation.libraryWanted();
      if (wanted) untracked(() => this.sight(wanted.sight));
    });
    closedByBack(
      () => this.navigation.palette(),
      () => this.navigation.palette.set(false),
    );
  }

  /** Shows the screen of `tab`; the settings touched again while shown go back to their list. */
  protected select(tab: Tab) {
    if (tab === 'settings' && this.tab() === 'settings') this.settingsScreen()?.home();
    else this.show(tab);
  }

  private show(tab: Tab) {
    this.lentFrom = null;
    this.tab.set(tab);
  }

  /** Brings the library into sight over the screen shown, the palette closed, or goes back to
   * that screen, as `sight` asks: a track loaded leaves only a library brought up so. Its tracks
   * gone through stay where the screen shows them (under the decks), but for its lists. */
  private sight(sight: LibrarySight) {
    const shown = this.tab() === 'library';
    const tracksShown = this.inSight.tracks() && this.library.view()?.focus !== 'lists';
    if (sight === 'browse' && tracksShown) {
      this.navigation.palette.set(false);
    } else if (sight === 'show' || sight === 'browse' || (sight === 'toggle' && !shown)) {
      this.navigation.palette.set(false);
      if (shown) return;
      this.lentFrom = this.tab();
      this.tab.set('library');
    } else if (shown && (sight === 'toggle' || this.lentFrom)) {
      this.show(this.lentFrom ?? 'decks');
    }
  }

  protected toggleTouch() {
    this.preferences
      .set('phoneTouch', !this.touch())
      .catch((e) => this.engine.error.set(errorText(e)));
  }
}

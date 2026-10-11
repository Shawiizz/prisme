import {
  Component,
  ElementRef,
  OnInit,
  afterRenderEffect,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { getVersion } from '@tauri-apps/api/app';
import { SettingsService } from '../../core/services/settings.service';
import { PreferencesService } from '../../core/services/preferences.service';
import { MixerService } from '../../core/services/mixer.service';
import { UpdateService } from '../../core/services/update.service';
import { NavigationService, SettingsCategory } from '../../core/services/navigation.service';
import { AudioBackend, AudioSettings, BUFFER_SIZES } from '../../core/models/audio';
import { DECK_COUNT, MixMode } from '../../core/models/engine';
import { DjPreferences, PITCH_RANGES } from '../../core/models/preferences';
import { LANGUAGE, LANGUAGES, Language, setLanguage, t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { TurntablePanel } from '../turntables/turntable-panel';
import { ControllersSettings } from '../controllers/controllers-settings';
import { ShortcutsSettings } from '../shortcuts/shortcuts-settings';
import { SETTINGS, SETTINGS_PAGES, SettingEntry, normalize } from './settings-catalog';

/**
 * The settings: a sidebar of pages with a search over every setting, the page or the
 * results as rows grouped in cards. Most settings apply at once; those that restart the
 * audio or the interface wait in a bar until applied, and the window does not close
 * over them.
 */
@Component({
  selector: 'app-settings-dialog',
  templateUrl: './settings-dialog.html',
  styleUrl: './settings-dialog.scss',
  imports: [TPipe, TurntablePanel, ControllersSettings, ShortcutsSettings],
})
export class SettingsDialog implements OnInit {
  protected readonly service = inject(SettingsService);
  private readonly preferences = inject(PreferencesService);
  protected readonly mixer = inject(MixerService);
  protected readonly updates = inject(UpdateService);
  private readonly navigation = inject(NavigationService);

  protected readonly pages = SETTINGS_PAGES;
  protected readonly pitchRanges = PITCH_RANGES;
  protected readonly languages = LANGUAGES;
  protected readonly decks = Array.from({ length: DECK_COUNT }, (_, i) => i);

  protected readonly category = computed(() => this.navigation.settings()?.category ?? 'audio');
  protected readonly page = computed(
    () => this.pages.find((p) => p.id === this.category()) ?? this.pages[0],
  );
  protected readonly query = signal('');
  private readonly words = computed(() => normalize(this.query()).split(/\s+/).filter(Boolean));
  /** The settings the search finds, `null` without a search. */
  protected readonly found = computed(() => {
    const words = this.words();
    if (!words.length) return null;
    const pagesFound = new Set(
      this.pages
        .filter((p) => matches(words, `${p.label} ${p.description} ${p.keywords}`))
        .map((p) => p.id),
    );
    const settings = new Set(
      SETTINGS.filter((s) => matches(words, `${s.label} ${s.description}`)).map((s) => s.id),
    );
    return { pages: pagesFound, settings };
  });
  protected readonly foundCount = computed(() => {
    const found = this.found();
    return found
      ? found.settings.size + this.panelPages().filter((p) => found.pages.has(p)).length
      : 0;
  });

  private readonly body = viewChild.required<ElementRef<HTMLElement>>('body');
  /** The setting pointed out (from the command palette), lit once. */
  protected readonly pointed = signal<string | null>(null);

  protected readonly language = signal<Language>(LANGUAGE);
  protected readonly draft = signal<AudioSettings | null>(null);
  protected readonly mixMode = signal<MixMode>(this.mixer.mode());
  protected readonly dj = this.preferences.dj;
  protected readonly writeTags = computed(() => this.preferences.library().writeTags);
  protected readonly busy = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly diagnosticsSaved = signal(false);
  protected readonly version = signal('');
  /** The bar of pending changes flashes when the window is asked to close over it. */
  protected readonly nudged = signal(false);

  protected readonly outputs = computed(() =>
    this.service.devices().filter((d) => d.kind === 'output'),
  );
  protected readonly inputs = computed(() =>
    this.service.devices().filter((d) => d.kind === 'input'),
  );
  protected readonly asio = computed(() => this.draft()?.backend === 'asio');
  /** Buffer sizes the chosen output's driver accepts. */
  protected readonly bufferSizes = computed(() => {
    const d = this.draft();
    const device =
      this.outputs().find((o) => o.name === d?.outputDevice) ??
      this.outputs().find((o) => o.isDefault);
    const range = device?.bufferRange;
    return range ? BUFFER_SIZES.filter((n) => n >= range[0] && n <= range[1]) : BUFFER_SIZES;
  });

  /** What waits for Apply: the audio restarts, or the interface starts again. */
  private readonly audioChanged = computed(() => {
    const d = this.draft();
    return !!d && JSON.stringify(d) !== JSON.stringify(this.service.settings());
  });
  protected readonly pending = computed(
    () =>
      this.audioChanged() || this.mixMode() !== this.mixer.mode() || this.language() !== LANGUAGE,
  );
  protected readonly pendingText = computed(() =>
    this.language() !== LANGUAGE && !this.audioChanged() && this.mixMode() === this.mixer.mode()
      ? t('Prisme starts again in the new language.')
      : t('The audio restarts to apply these changes.'),
  );

  constructor() {
    afterRenderEffect(() => {
      const setting = this.navigation.settings()?.setting;
      if (!setting) return;
      const row = this.body().nativeElement.querySelector(`[data-setting="${setting}"]`);
      row?.scrollIntoView({ block: 'center' });
      this.pointed.set(setting);
    });
  }

  async ngOnInit() {
    this.version.set(await getVersion().catch(() => ''));
    await this.service.refresh();
    this.draft.set(structuredClone(this.service.settings()));
  }

  /** Shows a page (and leaves the search). */
  go(category: SettingsCategory) {
    this.query.set('');
    this.navigation.openSettings(category);
    this.body().nativeElement.scrollTop = 0;
  }

  /** Escape empties the search first, then closes. */
  clearSearch(event: Event) {
    if (!this.query()) return;
    event.stopPropagation();
    this.query.set('');
  }

  /** Whether a page shows: the current one, or any page with something found. */
  shows(category: SettingsCategory): boolean {
    const found = this.found();
    if (!found) return this.category() === category;
    return (
      found.pages.has(category) ||
      SETTINGS.some((s) => s.page === category && found.settings.has(s.id))
    );
  }

  /** Whether a setting's row shows. */
  row(id: string): boolean {
    const found = this.found();
    if (!found) return true;
    const entry = SETTINGS.find((s) => s.id === id);
    return found.settings.has(id) || (!!entry && found.pages.has(entry.page));
  }

  entry(id: string): SettingEntry {
    return SETTINGS.find((s) => s.id === id)!;
  }

  pageLabel(category: SettingsCategory): string {
    return this.pages.find((p) => p.id === category)?.label ?? '';
  }

  /** Pages that are panels rather than rows: shown whole when the search finds them. */
  private panelPages(): SettingsCategory[] {
    return ['controllers', 'shortcuts'];
  }

  /** Closes, unless changes wait for Apply: then their bar flashes. */
  requestClose() {
    if (this.pending()) {
      this.nudged.set(false);
      requestAnimationFrame(() => this.nudged.set(true));
      return;
    }
    this.navigation.closeSettings();
  }

  update(changes: Partial<AudioSettings>) {
    const d = this.draft();
    if (d) this.draft.set({ ...d, ...changes });
  }

  updateDj(changes: Partial<DjPreferences>) {
    this.preferences.updateDj({ ...this.dj(), ...changes });
  }

  setWriteTags(writeTags: boolean) {
    this.preferences.updateLibrary({ writeTags });
  }

  /** Another backend has other devices: start again from its defaults. */
  async setBackend(backend: AudioBackend) {
    this.update({ backend, outputDevice: null, inputDevice: null, bufferFrames: null });
    await this.service.loadDevices(backend);
  }

  /** ASIO: one driver for the outputs and, if the turntables are on it, the inputs. */
  setDriver(name: string | null) {
    const d = this.draft();
    this.update({ outputDevice: name, inputDevice: d?.inputDevice ? name : null });
  }

  setTurntablesOnDriver(enabled: boolean) {
    this.update({ inputDevice: enabled ? (this.draft()?.outputDevice ?? null) : null });
  }

  /** Restarts the audio if its settings changed, then the interface if the language did. */
  async apply() {
    const d = this.draft();
    if (!d) return;
    this.busy.set(true);
    this.error.set(null);
    try {
      if (this.audioChanged()) await this.service.apply(d);
      if (this.mixMode() !== this.mixer.mode()) await this.mixer.setMode(this.mixMode());
      if (this.language() !== LANGUAGE) setLanguage(this.language());
      this.draft.set(structuredClone(this.service.settings()));
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.busy.set(false);
    }
  }

  async discard() {
    this.error.set(null);
    this.mixMode.set(this.mixer.mode());
    this.language.set(LANGUAGE);
    const settings = this.service.settings();
    if (settings) {
      await this.service.loadDevices(settings.backend);
      this.draft.set(structuredClone(settings));
    }
  }

  async saveDiagnostics() {
    try {
      this.diagnosticsSaved.set(await this.service.saveDiagnostics());
    } catch (e) {
      this.error.set(String(e));
    }
  }

  /** Select value -> setting (empty string = default / none). */
  orNull(v: string): string | null {
    return v === '' ? null : v;
  }

  bufferLabel(frames: number): string {
    const rate = this.service.info()?.sampleRate ?? 48_000;
    return t('{frames} samples · {ms} ms', { frames, ms: ((frames / rate) * 1000).toFixed(1) });
  }
}

/** Every word typed is found in the text. */
function matches(words: string[], text: string): boolean {
  const haystack = normalize(text);
  return words.every((w) => haystack.includes(w));
}

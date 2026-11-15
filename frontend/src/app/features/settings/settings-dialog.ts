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
import { invoke } from '@tauri-apps/api/core';
import { SettingsService } from '../../core/services/settings.service';
import { EngineService } from '../../core/services/engine.service';
import { PreferencesService } from '../../core/services/preferences.service';
import { MixerService } from '../../core/services/mixer.service';
import { UpdateService } from '../../core/services/update.service';
import { StemsService } from '../../core/services/stems.service';
import { NavigationService, SettingsCategory } from '../../core/services/navigation.service';
import { AudioBackend, AudioSettings, BACKEND_NAMES, BUFFER_SIZES } from '../../core/models/audio';
import { MAX_DECKS, MixMode } from '../../core/models/engine';
import { DjPreferences, PITCH_RANGES } from '../../core/models/preferences';
import { LANGUAGES, language, setLanguage, t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { TurntablePanel } from '../turntables/turntable-panel';
import { ControllersSettings } from '../controllers/controllers-settings';
import { ShortcutsSettings } from '../shortcuts/shortcuts-settings';
import { PHONE } from '../../core/platform';
import { ScreensService } from '../../core/services/screens.service';
import { SETTINGS, SETTINGS_PAGES, SettingEntry, matches, normalize } from './settings-catalog';
import { SettingsIcon } from './settings-icon';
import { decimal } from '../../shared/time';
import { errorText } from '../../core/errors';

/** `1, 2, … n` */
function numbers(n: number): number[] {
  return Array.from({ length: n }, (_, i) => i + 1);
}

/**
 * The settings: a sidebar of pages with a search over every setting, the page or the
 * results as rows grouped in cards. Most settings apply at once; those that restart the
 * audio or the interface wait in a bar until applied, and the window does not close
 * over them. On a phone, a page of its settings screen (`app-phone-settings`).
 */
@Component({
  selector: 'app-settings-dialog',
  templateUrl: './settings-dialog.html',
  styleUrls: ['./settings-dialog.scss', './settings-dialog.phone.scss'],
  imports: [TPipe, TurntablePanel, ControllersSettings, SettingsIcon, ShortcutsSettings],
})
export class SettingsDialog implements OnInit {
  protected readonly service = inject(SettingsService);
  private readonly engine = inject(EngineService);
  protected readonly preferences = inject(PreferencesService);
  protected readonly mixer = inject(MixerService);
  protected readonly updates = inject(UpdateService);
  protected readonly stems = inject(StemsService);
  private readonly navigation = inject(NavigationService);
  protected readonly screens = inject(ScreensService);

  protected readonly pages = SETTINGS_PAGES;
  /** On a phone the settings are a screen with its pages: this is one of them. */
  protected readonly PHONE = PHONE;
  protected readonly pitchRanges = PITCH_RANGES;
  protected readonly languages = LANGUAGES;
  protected readonly decks = this.engine.deckIndexes;
  protected readonly MAX_DECKS = MAX_DECKS;
  protected readonly BACKEND_NAMES = BACKEND_NAMES;

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

  protected readonly language = language;
  protected readonly setLanguage = setLanguage;
  protected readonly draft = signal<AudioSettings | null>(null);
  protected readonly mixMode = signal<MixMode>(this.mixer.mode());
  protected readonly dj = this.preferences.dj;
  /** The pairs the headphones can take on the running device, with the one chosen. */
  protected readonly headphonesPairs = computed(() => {
    const pairs = new Set<number>();
    for (let first = 2; first + 2 <= (this.mixer.outputChannels() ?? 0); first += 2)
      pairs.add(first);
    const chosen = this.mixer.headphones();
    if (chosen.kind === 'pair') pairs.add(chosen.first);
    return [...pairs].sort((a, b) => a - b);
  });
  protected readonly headphonesState = computed(() => {
    if (!this.mixer.internal() || this.mixer.outputChannels() === null) return '';
    const first = this.mixer.headphonesInUse();
    return first === null
      ? t('Not heard on this device.')
      : t('Now on outputs {a}-{b}.', { a: first + 1, b: first + 2 });
  });
  protected readonly writeTags = computed(() => this.preferences.library().writeTags);
  protected readonly replaceTags = computed(() => this.preferences.library().replaceTags);
  protected readonly pauseWhilePlaying = computed(
    () => this.preferences.library().pauseWhilePlaying,
  );
  protected readonly separateOnLoad = computed(
    () => this.preferences.library().stemsOnLoad ?? this.separatesOnGpu(),
  );
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
  /** One device for the outputs and, if the turntables are on it, the inputs: an ASIO driver,
   * a USB card Prisme drives itself. */
  protected readonly oneDevice = computed(() =>
    ['asio', 'usb'].includes(this.draft()?.backend ?? ''),
  );
  protected readonly driverLabel = computed(() =>
    this.draft()?.backend === 'usb' ? t('USB sound card') : t('ASIO driver'),
  );
  protected readonly firstDriverLabel = computed(() =>
    this.draft()?.backend === 'usb' ? t('The first plugged in') : t('Default driver'),
  );
  /** The output and the input a latency measurement joins, from 1. */
  protected readonly latencyOutput = signal(1);
  protected readonly latencyInput = signal(1);
  protected readonly latency = signal<'measuring' | { ms: number } | { error: string } | null>(
    null,
  );
  protected readonly latencyOutputs = computed(() =>
    numbers(this.service.info()?.outputChannels ?? 2),
  );
  protected readonly latencyInputs = computed(() => {
    const d = this.service.settings();
    const device = this.inputs().find((i) => i.name === (d?.inputDevice ?? d?.outputDevice));
    return numbers(Math.max(this.service.info()?.inputChannels ?? 0, device?.channels ?? 0, 2));
  });
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
    () => this.audioChanged() || this.mixMode() !== this.mixer.mode(),
  );
  protected readonly pendingText = computed(() => t('The audio restarts to apply these changes.'));

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
    return this.preferences.updateDj({ ...this.dj(), ...changes });
  }

  /** Another number of decks is another engine: the audio starts again with it. */
  async setDeckCount(deckCount: number) {
    try {
      await this.updateDj({ deckCount });
    } catch (e) {
      this.engine.error.set(errorText(e));
    } finally {
      await Promise.all([this.service.refresh(), this.engine.refreshTracks()]);
    }
  }

  /** `0.42` → `42 %` */
  percent(fraction: number): string {
    return `${Math.round(fraction * 100)} %`;
  }

  megabytes(bytes: number): number {
    return Math.round(bytes / 1e6);
  }

  setWriteTags(writeTags: boolean) {
    this.preferences.updateLibrary({ writeTags });
  }

  setHeadphones(value: string) {
    this.mixer.setHeadphones(
      value === 'auto' || value === 'off'
        ? { kind: value }
        : { kind: 'pair', first: Number(value) },
    );
  }

  setPauseWhilePlaying(pauseWhilePlaying: boolean) {
    this.preferences.updateLibrary({ pauseWhilePlaying });
  }

  setReplaceTags(replaceTags: boolean) {
    this.preferences.updateLibrary({ replaceTags });
  }

  /** A graphics chip worth it is there, and the DJ lets it separate. */
  protected readonly separatesOnGpu = computed(
    () => (this.stems.gpu() ?? false) && this.preferences.library().stemsGpu,
  );

  protected readonly stemsFast = computed(() => this.preferences.library().stemsFast);

  setStemsFast(stemsFast: boolean) {
    void this.preferences.updateLibrary({ stemsFast });
  }

  setStemsGpu(stemsGpu: boolean) {
    void this.preferences.updateLibrary({ stemsGpu });
  }

  setSeparateOnLoad(stemsOnLoad: boolean) {
    this.preferences.updateLibrary({ stemsOnLoad });
  }

  /** Another backend has other devices: start again from its defaults. */
  async setBackend(backend: AudioBackend) {
    this.update({ backend, outputDevice: null, inputDevice: null, bufferFrames: null });
    await this.service.loadDevices(backend);
  }

  /** One device (`oneDevice`) for the outputs and, if the turntables are on it, the inputs. */
  setDriver(name: string | null) {
    const d = this.draft();
    this.update({ outputDevice: name, inputDevice: d?.inputDevice ? name : null });
  }

  setTurntablesOnDriver(enabled: boolean) {
    const device = this.draft()?.outputDevice ?? this.outputs()[0]?.name ?? null;
    this.update({ inputDevice: enabled ? device : null });
  }

  /** Plays clicks on the output chosen and times their return on the input chosen. */
  async measureLatency() {
    this.latency.set('measuring');
    try {
      const measured = await invoke<{ roundTripMs: number }>('measure_latency', {
        output: this.latencyOutput(),
        input: this.latencyInput(),
      });
      this.latency.set({ ms: measured.roundTripMs });
    } catch (e) {
      this.latency.set({ error: errorText(e) });
    }
  }

  /** `12.3 ms` */
  latencyLabel(ms: number): string {
    return `${decimal(ms, 1)} ms`;
  }

  /** Restarts the audio if its settings changed. */
  async apply() {
    const d = this.draft();
    if (!d) return;
    this.busy.set(true);
    this.error.set(null);
    try {
      if (this.audioChanged()) await this.service.apply(d);
      if (this.mixMode() !== this.mixer.mode()) await this.mixer.setMode(this.mixMode());
      this.draft.set(structuredClone(this.service.settings()));
    } catch (e) {
      this.error.set(errorText(e));
    } finally {
      this.busy.set(false);
    }
  }

  async discard() {
    this.error.set(null);
    this.mixMode.set(this.mixer.mode());
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
      this.error.set(errorText(e));
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

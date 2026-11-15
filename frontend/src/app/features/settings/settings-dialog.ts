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
import { DjPreferences, LibraryPreferences, PITCH_RANGES } from '../../core/models/preferences';
import { LANGUAGES, language, setLanguage, t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { TurntablePanel } from '../turntables/turntable-panel';
import { ControllersSettings } from '../controllers/controllers-settings';
import { ShortcutsSettings } from '../shortcuts/shortcuts-settings';
import { ANDROID, PHONE } from '../../core/platform';
import { ScreensService } from '../../core/services/screens.service';
import { SETTINGS_PAGES, SETTING_BY_ID } from './settings-catalog';
import { SettingsSearch } from './settings-search';
import { SettingControl, SettingRow } from './setting-row';
import { SettingsIcon } from './settings-icon';
import { decimal } from '../../shared/time';
import { errorText } from '../../core/errors';
import { Dismiss, overlay } from '../../shared/overlay';

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
  imports: [
    TPipe,
    TurntablePanel,
    ControllersSettings,
    SettingsIcon,
    ShortcutsSettings,
    Dismiss,
    SettingRow,
    SettingControl,
  ],
  providers: [SettingsSearch],
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
  protected readonly search = inject(SettingsSearch);

  protected readonly pages = SETTINGS_PAGES;
  /** On a phone the head's button goes back to the pages, and there is no screen below. */
  protected readonly PHONE = PHONE;
  protected readonly ANDROID = ANDROID;
  protected readonly pitchRanges = PITCH_RANGES;
  protected readonly languages = LANGUAGES;
  protected readonly decks = this.engine.deckIndexes;
  protected readonly MAX_DECKS = MAX_DECKS;
  protected readonly BACKEND_NAMES = BACKEND_NAMES;

  protected readonly category = computed(() => this.navigation.settings()?.category ?? 'audio');
  protected readonly page = computed(
    () => this.pages.find((p) => p.id === this.category()) ?? this.pages[0],
  );

  private readonly body = viewChild.required<ElementRef<HTMLElement>>('body');

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
  /** What the choice of the headphones does, and where they are heard on the running device. */
  protected readonly headphonesHelp = computed(() => {
    const help = SETTING_BY_ID.get('mixing.headphones')!.description();
    if (!this.mixer.internal() || this.mixer.outputChannels() === null) return help;
    const first = this.mixer.headphonesInUse();
    return first === null
      ? `${help} ${t('Not heard on this device.')}`
      : `${help} ${t('Now on outputs {a}-{b}.', { a: first + 1, b: first + 2 })}`;
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

  constructor() {
    // what opens it answers the back gesture
    overlay(
      () => true,
      () => this.requestClose(),
      { back: false },
    );
    afterRenderEffect(() => {
      const setting = this.navigation.settings()?.setting;
      if (!setting) return;
      const row = this.body().nativeElement.querySelector(`[data-setting="${setting}"]`);
      row?.scrollIntoView({ block: 'center' });
      this.search.pointed.set(setting);
    });
  }

  async ngOnInit() {
    const [version] = await Promise.all([
      getVersion().catch(() => ''),
      this.service.refresh().catch((e) => this.error.set(errorText(e))),
    ]);
    this.version.set(version);
    this.draft.set(structuredClone(this.service.settings()));
  }

  /** Shows a page (and leaves the search). */
  go(category: SettingsCategory) {
    this.search.query.set('');
    this.navigation.openSettings(category);
    this.body().nativeElement.scrollTop = 0;
  }

  /** Escape empties the search first, then closes. */
  clearSearch(event: Event) {
    if (!this.search.query()) return;
    event.stopPropagation();
    this.search.query.set('');
  }

  /** Whether a page shows: the current one, or any page with something found. */
  shows(category: SettingsCategory): boolean {
    const found = this.search.found();
    if (!found) return this.category() === category;
    return found.pages.has(category) || found.holding.has(category);
  }

  pageLabel(category: SettingsCategory): string {
    return this.pages.find((p) => p.id === category)?.label() ?? '';
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

  setDj(changes: Partial<DjPreferences>) {
    this.saved(this.preferences.updateDj(changes));
  }

  setLibrary(changes: Partial<LibraryPreferences>) {
    this.saved(this.preferences.updateLibrary(changes));
  }

  /** A preference saving: what fails is told under the page. */
  private saved(saving: Promise<void>) {
    saving.catch((e) => this.error.set(errorText(e)));
  }

  /** Another number of decks is another engine: the audio starts again with it. */
  async setDeckCount(deckCount: number) {
    try {
      await this.preferences.updateDj({ deckCount });
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

  setHeadphones(value: string) {
    this.mixer.setHeadphones(
      value === 'auto' || value === 'off'
        ? { kind: value }
        : { kind: 'pair', first: Number(value) },
    );
  }

  /** A graphics chip worth it is there, and the DJ lets it separate. */
  protected readonly separatesOnGpu = computed(
    () => (this.stems.gpu() ?? false) && this.preferences.library().stemsGpu,
  );

  protected readonly stemsFast = computed(() => this.preferences.library().stemsFast);

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

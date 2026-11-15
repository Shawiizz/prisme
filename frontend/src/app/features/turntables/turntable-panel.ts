import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  computed,
  inject,
  input,
  viewChild,
} from '@angular/core';
import { Channel, invoke } from '@tauri-apps/api/core';
import { EngineService } from '../../core/services/engine.service';
import { SettingsService } from '../../core/services/settings.service';
import { Rpm, SignalState, VinylMode } from '../../core/models/engine';
import { deckLetter, formatSpeed, formatTime } from '../../shared/time';
import { ScopePainter } from './scope-painter';
import { Text, t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import { errorText } from '../../core/errors';

const SCOPE_SIZE = 220;
/** Carrier level shown as a full bar: a healthy line-level timecode reaches it. */
const FULL_LEVEL = 0.5;
const SIGNAL_TEXT: Record<SignalState, Text> = {
  none: () => t('none'),
  bounce: () => t('unsteady'),
  ok: () => t('locked'),
};

/** One turntable: its signal on a scope, what the decoder reads, the vinyl settings. */
@Component({
  selector: 'app-turntable-panel',
  templateUrl: './turntable-panel.html',
  styleUrl: './turntable-panel.scss',
  imports: [TPipe],
})
export class TurntablePanel {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  private readonly audio = inject(SettingsService);
  protected readonly size = SCOPE_SIZE;
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  /** Where the track starts on the record when no lead-in is chosen. */
  protected readonly recordLeadIn = computed(() => {
    const id = this.audio.settings()?.timecodeFormat;
    return String(this.audio.formats().find((f) => f.id === id)?.leadInS ?? 0);
  });
  protected readonly vinyl = computed(
    () => this.engine.status()?.decks[this.deck()]?.vinyl ?? null,
  );
  protected readonly signalText = computed(() => SIGNAL_TEXT[this.vinyl()?.signal ?? 'none']());
  protected readonly levelPercent = computed(() =>
    Math.min(100, ((this.vinyl()?.level ?? 0) / FULL_LEVEL) * 100),
  );
  protected readonly speed = computed(() => formatSpeed(this.vinyl()?.recordSpeed ?? 0));
  protected readonly position = computed(() => {
    const p = this.vinyl()?.recordPositionS;
    return p == null ? '—' : formatTime(p);
  });

  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('scope');
  private readonly painter = new ScopePainter();

  constructor() {
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      const ctx = this.canvas().nativeElement.getContext('2d');
      const channel = new Channel<[number, number][]>();
      channel.onmessage = (points) => ctx && this.painter.draw(ctx, points, SCOPE_SIZE);
      invoke('subscribe_scope', { deck: this.deck(), channel }).catch((e) =>
        this.engine.error.set(errorText(e)),
      );
    });
    destroyRef.onDestroy(() => invoke('unsubscribe_scope', { deck: this.deck() }));
  }

  setEnabled(enabled: boolean) {
    this.engine.setVinyl(this.deck(), { enabled });
  }

  setMode(mode: VinylMode) {
    this.engine.setVinyl(this.deck(), { mode });
  }

  setRpm(rpm: Rpm) {
    this.engine.setVinyl(this.deck(), { rpm });
  }

  /** Emptied, the record's own lead-in again. */
  setLeadIn(event: Event) {
    const value = (event.target as HTMLInputElement).value.trim();
    const seconds = Number(value);
    if (value === '') this.engine.setVinyl(this.deck(), { leadInS: null });
    else if (Number.isFinite(seconds) && seconds >= 0)
      this.engine.setVinyl(this.deck(), { leadInS: seconds });
  }

  setSwap(swap: boolean) {
    this.engine.setVinylSwap(this.deck(), swap);
  }
}

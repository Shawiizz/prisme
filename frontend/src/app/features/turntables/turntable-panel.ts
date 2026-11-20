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
  /** The deck whose turntable this one shares, its record chosen there. */
  protected readonly layeredOn = computed(() => this.settings().layeredOn);
  protected readonly layeredLetter = computed(() => deckLetter(this.layeredOn() ?? 0));
  protected readonly makers = this.audio.makers;
  private readonly records = computed(() => this.makers().flatMap((m) => m.records));
  /** The record on the turntable, with its sides. */
  protected readonly current = computed(() => {
    const id = this.settings().timecode;
    return this.records().find((r) => r.sides.some((s) => s.id === id)) ?? null;
  });
  /** Where the track starts on the record when no lead-in is chosen, at its speed. */
  protected readonly recordLeadIn = computed(() =>
    String(Math.round(this.settings().recordLeadInS * 10) / 10),
  );
  protected readonly vinyl = computed(
    () => this.engine.status()?.decks[this.deck()]?.vinyl ?? null,
  );
  protected readonly signalText = computed(() => SIGNAL_TEXT[this.vinyl()?.signal ?? 'none']());
  protected readonly levelPercent = computed(() => (this.vinyl()?.level ?? 0) * 100);
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
    this.engine.setVinylEnabled(this.deck(), enabled);
  }

  setMode(mode: VinylMode) {
    this.engine.setVinyl(this.deck(), { setting: 'mode', value: mode });
  }

  setRpm(rpm: Rpm) {
    this.engine.setVinyl(this.deck(), { setting: 'rpm', value: rpm });
  }

  /** Emptied, the record's own lead-in again. */
  setLeadIn(event: Event) {
    const value = (event.target as HTMLInputElement).value.trim();
    const seconds = Number(value);
    if (value === '') this.engine.setVinyl(this.deck(), { setting: 'leadIn', value: null });
    else if (Number.isFinite(seconds) && seconds >= 0)
      this.engine.setVinyl(this.deck(), { setting: 'leadIn', value: seconds });
  }

  setTimecode(format: string) {
    this.engine.setTimecode(this.deck(), format);
  }

  /** Another record (the id of its first side): on the side it was on, if it has that side. */
  setRecord(record: string) {
    this.engine.call('set_record', { deck: this.deck(), record });
  }

  setSwap(swap: boolean) {
    this.engine.setVinylSwap(this.deck(), swap);
  }
}

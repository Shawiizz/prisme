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
import { SignalState, VinylMode } from '../../core/models/engine';
import { deckLetter, formatSpeed, formatTime } from '../../shared/time';
import { ScopePainter } from './scope-painter';
import { t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';

const SCOPE_SIZE = 220;
/** Carrier level shown as a full bar: a healthy line-level timecode reaches it. */
const FULL_LEVEL = 0.5;
const SIGNAL_TEXT: Record<SignalState, string> = {
  none: t('none'),
  bounce: t('unsteady'),
  ok: t('locked'),
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
  protected readonly size = SCOPE_SIZE;
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly vinyl = computed(
    () => this.engine.status()?.decks[this.deck()]?.vinyl ?? null,
  );
  protected readonly signalText = computed(() => SIGNAL_TEXT[this.vinyl()?.signal ?? 'none']);
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
      const style = getComputedStyle(document.documentElement);
      const colors = {
        dots: style.getPropertyValue('--pr-light').trim(),
        guides: style.getPropertyValue('--pr-line-2').trim(),
      };
      const channel = new Channel<[number, number][]>();
      channel.onmessage = (points) => ctx && this.painter.draw(ctx, points, SCOPE_SIZE, colors);
      invoke('subscribe_scope', { deck: this.deck(), channel }).catch((e) =>
        this.engine.error.set(String(e)),
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

  setLeadIn(event: Event) {
    const seconds = Number((event.target as HTMLInputElement).value);
    if (Number.isFinite(seconds) && seconds >= 0)
      this.engine.setVinyl(this.deck(), { leadInS: seconds });
  }

  setSwap(swap: boolean) {
    this.engine.setVinylSwap(this.deck(), swap);
  }
}

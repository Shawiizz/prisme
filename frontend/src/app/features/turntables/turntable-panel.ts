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
import { VinylMode } from '../../core/models/engine';
import { deckColor, formatSpeed, formatTime } from '../../shared/time';
import { ScopePainter } from './scope-painter';

const SCOPE_SIZE = 220;
/** Carrier level shown as a full bar: a healthy line-level timecode reaches it. */
const FULL_LEVEL = 0.5;

/** One turntable: its signal on a scope, what the decoder reads, the vinyl settings. */
@Component({
  selector: 'app-turntable-panel',
  templateUrl: './turntable-panel.html',
  styleUrl: './turntable-panel.scss',
})
export class TurntablePanel {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  protected readonly size = SCOPE_SIZE;
  protected readonly color = computed(() => deckColor(this.deck()));
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly vinyl = computed(
    () => this.engine.status()?.decks[this.deck()]?.vinyl ?? null,
  );
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
      const stroke =
        getComputedStyle(this.canvas().nativeElement).getPropertyValue('--accent-color').trim() ||
        '#2fd6ff';
      const channel = new Channel<[number, number][]>();
      channel.onmessage = (points) => ctx && this.painter.draw(ctx, points, SCOPE_SIZE, stroke);
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

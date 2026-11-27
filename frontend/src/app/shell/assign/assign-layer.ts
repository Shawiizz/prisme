import { Component, ElementRef, computed, inject, signal, viewChild } from '@angular/core';
import { AssignService } from '../../core/assign/assign.service';
import { KeyboardService } from '../../core/keyboard/keyboard.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { labelText, t } from '../../core/i18n/i18n';
import { everyFrame } from '../../shared/frames';

/** The key or the control of an action, on its button. */
interface Cap {
  control: string;
  text: string;
  x: number;
  y: number;
}

/** Where the bubble is: beside its button, on the side with room. */
interface Place {
  x: number;
  y: number;
  /** On the button's left. */
  left: boolean;
}

/** Between the button and its bubble. */
const GAP = 14;
const BUBBLE_WIDTH = 320;

/**
 * Over the page while assigning: each outlined button's key (or control), and the bubble of the
 * one chosen, which waits for a key or a control, says whose key the one pressed is, and takes
 * keys and controls off.
 */
@Component({
  selector: 'app-assign-layer',
  template: `
    @for (cap of caps(); track cap.control) {
      <span
        class="cap"
        [class.pointed]="assign.pointed() === cap.control"
        [style.left.px]="cap.x"
        [style.top.px]="cap.y"
        >{{ cap.text }}</span
      >
    }
    @if (assign.chosen(); as chosen) {
      @let at = place();
      <div
        #bubble
        class="bubble"
        data-assign-ui
        [class.left]="at?.left"
        [style.left.px]="at?.x ?? -9999"
        [style.top.px]="at?.y ?? -9999"
        role="dialog"
        [attr.aria-label]="name().label"
      >
        <div class="what">
          <b>{{ name().label }}</b
          ><span>{{ name().group }}</span>
        </div>
        @switch (assign.step()?.kind) {
          @case ('listening') {
            <div class="listen">
              <span class="dot"></span>
              {{
                assign.way() === 'keys'
                  ? ('Press a key…' | t)
                  : ('Move the control on the controller…' | t)
              }}
            </div>
            @if (assign.way() === 'keys') {
              <div class="list">
                <span class="pr-label">{{ 'Its keys' | t }}</span>
                @for (key of keys(); track $index) {
                  <span class="item pr-num"
                    >{{ key
                    }}<button
                      [attr.aria-label]="'Take off' | t"
                      [title]="'Take off' | t"
                      (click)="assign.removeKey($index)"
                    >
                      ×
                    </button></span
                  >
                } @empty {
                  <span class="none">{{ 'No key' | t }}</span>
                }
              </div>
              <div class="foot">
                <button class="pr-btn pr-btn--sm" (click)="assign.resetKeys()">
                  ↺ {{ 'Prisme’s key' | t }}
                </button>
                <span class="hint">{{ 'Esc: cancel' | t }}</span>
              </div>
            } @else {
              <div class="list">
                <span class="pr-label">{{ 'Its controls' | t }}</span>
                @for (c of controls(); track $index) {
                  <span class="item"
                    >{{ c.name
                    }}<button
                      [attr.aria-label]="'Take off' | t"
                      [title]="c.device"
                      (click)="assign.forget(c.device)"
                    >
                      ×
                    </button></span
                  >
                } @empty {
                  <span class="none">{{ 'None' | t }}</span>
                }
              </div>
              <div class="foot">
                <span class="hint">{{ 'Esc: cancel' | t }}</span>
              </div>
            }
          }
          @case ('taken') {
            <div class="listen heard">
              <span class="big pr-num">{{ takenKey() }}</span>
            </div>
            <p class="warn">
              {{
                '{key} is already the key of {action}.' | t: { key: takenKey(), action: owner() }
              }}
              {{ 'Taking it takes it off that action.' | t }}
            </p>
            <div class="foot">
              <button class="pr-btn pr-btn--light pr-btn--sm" (click)="assign.take()">
                {{ 'Take it' | t }}
              </button>
              <button class="pr-btn pr-btn--sm" (click)="assign.again()">
                {{ 'Another' | t }}
              </button>
            </div>
          }
          @case ('learnt') {
            <div class="listen heard">
              <span class="big learnt">{{ controls()[0]?.name ?? ('Learnt' | t) }}</span>
            </div>
            <div class="foot">
              <button class="pr-btn pr-btn--light pr-btn--sm" (click)="assign.drop()">
                {{ 'Keep' | t }}
              </button>
              <button class="pr-btn pr-btn--sm" (click)="assign.again()">
                {{ 'Another' | t }}
              </button>
            </div>
          }
        }
      </div>
    }
  `,
  styles: `
    :host {
      position: fixed;
      inset: 0;
      z-index: var(--pr-z-menu);
      pointer-events: none;
    }
    .cap {
      position: absolute;
      transform: translate(-70%, -55%);
      display: inline-grid;
      place-items: center;
      min-width: 18px;
      height: 17px;
      padding: 0 4px;
      border-radius: var(--pr-radius);
      background: var(--pr-light);
      color: var(--pr-on-light);
      font: 600 9.5px / 1 var(--pr-font-mono);
      font-stretch: var(--pr-mono-width);
      white-space: nowrap;
      box-shadow: 0 0 0 2px var(--pr-void);
      &.pointed {
        background: var(--pr-warn);
      }
    }
    .bubble {
      position: absolute;
      width: 320px;
      padding: 14px 16px;
      pointer-events: auto;
      background: var(--pr-raise);
      border: 1px solid var(--pr-line-2);
      border-radius: var(--pr-radius);
      box-shadow: 0 18px 48px rgb(0 0 0 / 0.6);
      animation: pr-fade-in 120ms ease-out;
      // the arrow to the button
      &::before {
        content: '';
        position: absolute;
        top: 18px;
        left: -7px;
        width: 12px;
        height: 12px;
        background: var(--pr-raise);
        border-left: 1px solid var(--pr-line-2);
        border-bottom: 1px solid var(--pr-line-2);
        transform: rotate(45deg);
      }
      &.left::before {
        left: auto;
        right: -7px;
        transform: rotate(225deg);
      }
    }
    .what {
      display: flex;
      align-items: baseline;
      gap: 8px;
      min-width: 0;
      b {
        font: 700 14px var(--pr-font);
        white-space: nowrap;
      }
      span {
        color: var(--pr-text-3);
        font-size: var(--pr-size-small);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
    }
    .listen {
      margin: 12px 0 10px;
      min-height: 58px;
      padding: 10px;
      display: grid;
      grid-auto-flow: column;
      place-content: center;
      align-items: center;
      gap: 8px;
      border: 1px dashed var(--pr-line-2);
      border-radius: var(--pr-radius);
      color: var(--pr-text-2);
      &.heard {
        border-style: solid;
        border-color: var(--pr-light);
      }
    }
    .dot {
      width: 7px;
      height: 7px;
      border-radius: 50%;
      background: var(--pr-ok);
      animation: pr-blink 1.2s steps(1) infinite;
    }
    .big {
      display: inline-grid;
      place-items: center;
      min-width: 48px;
      height: 36px;
      padding: 0 12px;
      border-radius: var(--pr-radius);
      background: var(--pr-light);
      color: var(--pr-on-light);
      font: 600 17px var(--pr-font-mono);
      font-stretch: var(--pr-mono-width);
      &.learnt {
        font: 600 13px var(--pr-font);
      }
    }
    .list {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 6px;
      .pr-label {
        margin-right: 4px;
      }
    }
    .item {
      display: inline-flex;
      align-items: center;
      gap: 2px;
      height: 26px;
      padding: 0 2px 0 8px;
      border: 1px solid var(--pr-line-2);
      border-radius: var(--pr-radius);
      font-size: var(--pr-size-small);
      button {
        width: 20px;
        height: 20px;
        border: none;
        background: transparent;
        color: var(--pr-text-3);
        cursor: pointer;
        &:hover {
          color: var(--pr-text);
        }
      }
    }
    .none {
      color: var(--pr-text-3);
      font-size: var(--pr-size-small);
    }
    .warn {
      margin: 0;
      line-height: 1.45;
    }
    .foot {
      margin-top: 12px;
      display: flex;
      align-items: center;
      gap: 8px;
    }
    .hint {
      margin-left: auto;
      color: var(--pr-text-3);
      font-size: 11.5px;
    }
  `,
  imports: [TPipe],
})
export class AssignLayer {
  protected readonly assign = inject(AssignService);
  private readonly keyboard = inject(KeyboardService);
  private readonly bubble = viewChild<ElementRef<HTMLElement>>('bubble');

  protected readonly caps = signal<Cap[]>([]);
  protected readonly place = signal<Place | null>(null);
  private placed = '';

  protected readonly name = computed(() => {
    const chosen = this.assign.chosen();
    return chosen ? this.assign.name(chosen.control) : { label: '', group: '' };
  });
  protected readonly keys = computed(() => {
    const chosen = this.assign.chosen();
    return chosen ? this.assign.keysOf(chosen.control) : [];
  });
  protected readonly controls = computed(() => {
    const chosen = this.assign.chosen();
    return chosen ? this.assign.controlsOf(chosen.control) : [];
  });
  protected readonly takenKey = computed(() => {
    const step = this.assign.step();
    return step?.kind === 'taken' ? this.keyboard.label(step.key) : '';
  });
  /** "CUE (Deck B)" */
  protected readonly owner = computed(() => {
    const step = this.assign.step();
    const action = step?.kind === 'taken' ? step.from[0]?.action : null;
    return action ? `${labelText(action.label)} (${labelText(action.group)})` : t('another action');
  });

  constructor() {
    // the buttons move with the page: the caps and the bubble follow them
    everyFrame(() => this.follow());
  }

  private follow() {
    if (!this.assign.on()) {
      if (this.placed !== '') {
        this.placed = '';
        this.caps.set([]);
        this.place.set(null);
      }
      return;
    }
    const caps: Cap[] = [];
    for (const element of document.querySelectorAll<HTMLElement>('.pr-assignable')) {
      const control = element.dataset['assign'];
      const text = control && this.assign.cap(control);
      if (!control || !text) continue;
      const r = element.getBoundingClientRect();
      if (r.width === 0) continue;
      // held inside the window: the title bar's keys are at its top
      caps.push({ control, text, x: Math.round(r.right), y: Math.max(Math.round(r.top), 10) });
    }
    const place = this.placeBubble();
    const placed = JSON.stringify([caps, place]);
    if (placed === this.placed) return;
    this.placed = placed;
    this.caps.set(caps);
    this.place.set(place);
  }

  /** Beside the button chosen, on the right if there is room, held inside the window. */
  private placeBubble(): Place | null {
    const chosen = this.assign.chosen();
    if (!chosen) return null;
    const r = chosen.element.getBoundingClientRect();
    const height = this.bubble()?.nativeElement.offsetHeight ?? 200;
    const left = r.right + GAP + BUBBLE_WIDTH > innerWidth - 8;
    const x = left ? r.left - GAP - BUBBLE_WIDTH : r.right + GAP;
    const y = Math.min(Math.max(r.top - 6, 8), innerHeight - height - 8);
    return { x: Math.round(x), y: Math.round(y), left };
  }
}

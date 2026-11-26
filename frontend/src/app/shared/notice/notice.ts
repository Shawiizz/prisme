import { Component, Injectable, inject, signal } from '@angular/core';
import { TPipe } from '../../core/i18n/t.pipe';
import { PHONE } from '../../core/platform';

/** Something to tell that does not wait for an answer: a line, what follows, and what can be
 * done about it, the main thing first. */
export interface Notice {
  text: string;
  detail?: string;
  actions?: { label: string; run: () => void }[];
  /** Something the DJ must see: at the top of the screen, bigger. */
  prominent?: boolean;
  /** Something wrong the DJ must see: prominent, and marked so. */
  warning?: boolean;
  /** Stays until closed or followed by another, not gone after a while. */
  stays?: boolean;
}

/** How long a notice stays, unless closed. */
const SHOWN_MS = 15_000;

/** Tells the DJ, without stopping anything, what happened out of sight. */
@Injectable({ providedIn: 'root' })
export class NoticeService {
  readonly shown = signal<Notice | null>(null);
  private timer?: ReturnType<typeof setTimeout>;

  /** Shows `notice` in place of the one shown, if any. */
  show(notice: Notice) {
    clearTimeout(this.timer);
    this.shown.set(notice);
    if (!notice.stays) this.timer = setTimeout(() => this.shown.set(null), SHOWN_MS);
  }

  close() {
    clearTimeout(this.timer);
    this.shown.set(null);
  }
}

/** The notice of [`NoticeService`]: at the bottom of a computer's screen, over the tabs of a
 * phone's. */
@Component({
  selector: 'app-notice',
  template: `
    @if (service.shown(); as n) {
      <div
        class="pr-popover notice"
        [class.phone]="PHONE"
        [class.prominent]="n.prominent || n.warning"
        [class.warning]="n.warning"
        [attr.role]="n.warning ? 'alert' : 'status'"
        animate.enter="pr-fade-in"
        animate.leave="pr-fade-out"
      >
        @if (n.warning) {
          <svg class="sign" viewBox="0 0 16 16" aria-hidden="true">
            <path d="M8 1.8l6.6 11.7H1.4z" />
            <path d="M8 6.3v3.4M8 11.6v.1" />
          </svg>
        }
        <div class="words">
          <span class="text">{{ n.text }}</span>
          @if (n.detail) {
            <span class="detail">{{ n.detail }}</span>
          }
        </div>
        @for (action of n.actions ?? []; track action.label) {
          <button
            class="pr-btn pr-btn--sm"
            [class.pr-btn--light]="$first && n.actions!.length > 1"
            [class.pr-reach]="PHONE"
            (click)="run(action.run)"
          >
            {{ action.label }}
          </button>
        }
        <button
          class="pr-btn pr-btn--sm pr-btn--ghost pr-btn--icon"
          [class.pr-reach]="PHONE"
          (click)="service.close()"
          [attr.aria-label]="'Close' | t"
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </div>
    }
  `,
  styles: `
    :host {
      --pr-layer: var(--pr-z-menu);
    }
    .notice {
      position: fixed;
      bottom: var(--pr-space-5);
      left: 50%;
      translate: -50% 0;
      display: flex;
      align-items: center;
      gap: var(--pr-space-2);
      width: max-content;
      max-width: calc(100vw - 32px);
      padding: var(--pr-space-2) var(--pr-space-2) var(--pr-space-2) var(--pr-space-4);
      box-shadow: 0 16px 48px rgb(0 0 0 / 0.5);
    }
    // to be seen: bigger, at the top of the screen where the eyes are
    .prominent {
      top: calc(var(--pr-titlebar, 40px) + var(--pr-space-3));
      bottom: auto;
      max-width: min(720px, calc(100vw - 32px));
      padding-left: var(--pr-space-3);
      // the keys on the line of what is asked, not between the two lines
      align-items: end;
      .text {
        font-size: var(--pr-size);
      }
      .detail {
        color: var(--pr-text-2);
      }
    }
    // something wrong: marked
    .warning {
      border-color: var(--pr-warn);
      box-shadow:
        inset 3px 0 0 var(--pr-warn),
        0 16px 48px rgb(0 0 0 / 0.5);
    }
    .sign {
      flex: none;
      align-self: center;
      width: 20px;
      height: 20px;
      color: var(--pr-warn);
      stroke-width: 1.4;
      stroke-linejoin: round;
    }
    // over the tabs, under the thumb
    .phone {
      bottom: calc(72px + env(safe-area-inset-bottom));
      width: calc(100vw - 24px);
    }
    .words {
      display: grid;
      gap: 2px;
      min-width: 0;
      flex: 1;
    }
    .text {
      font-size: var(--pr-size-small);
      font-weight: 600;
      color: var(--pr-text);
      overflow-wrap: anywhere;
    }
    .detail {
      font-size: var(--pr-size-small);
      color: var(--pr-text-3);
      overflow-wrap: anywhere;
    }
    svg {
      width: 12px;
      height: 12px;
      fill: none;
      stroke: currentColor;
      stroke-width: 1.5;
      stroke-linecap: round;
    }
  `,
  imports: [TPipe],
})
export class NoticeView {
  protected readonly PHONE = PHONE;
  protected readonly service = inject(NoticeService);

  protected run(action: () => void) {
    this.service.close();
    action();
  }
}

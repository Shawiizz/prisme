import { Component, Injectable, inject, signal } from '@angular/core';
import { TPipe } from '../../core/i18n/t.pipe';
import { PHONE } from '../../core/platform';

/** Something to tell that does not wait for an answer: a line, what follows, and what can be
 * done about it. */
export interface Notice {
  text: string;
  detail?: string;
  action?: { label: string; run: () => void };
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
    this.timer = setTimeout(() => this.shown.set(null), SHOWN_MS);
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
        role="status"
        animate.enter="pr-fade-in"
        animate.leave="pr-fade-out"
      >
        <div class="words">
          <span class="text">{{ n.text }}</span>
          @if (n.detail) {
            <span class="detail">{{ n.detail }}</span>
          }
        </div>
        @if (n.action; as action) {
          <button class="pr-btn pr-btn--sm" [class.pr-reach]="PHONE" (click)="run(action.run)">
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

import { Component, DestroyRef, ElementRef, inject, viewChild } from '@angular/core';

/** The mouse rests this long on an element (ms) before its tip shows. */
const SHOW_MS = 450;
/** Room between the tip, the element it tells of and the edges of the window (px). */
const GAP = 6;

/**
 * The tips of the page, in its own look: what an element's `title` says, under it (over it
 * near the bottom of the window) while the mouse rests on it. The title is held aside
 * meanwhile, so that the system does not show its own tip too.
 */
@Component({
  selector: 'app-tooltip',
  template: `<div #tip class="tip" role="tooltip" hidden></div>`,
  styles: `
    .tip {
      position: fixed;
      z-index: var(--pr-z-tip);
      max-width: 300px;
      padding: 5px 8px;
      font: 400 var(--pr-size-small) / 1.4 var(--pr-font);
      color: var(--pr-text);
      white-space: pre-line;
      background: var(--pr-base);
      border: 1px solid var(--pr-line-2);
      border-radius: var(--pr-radius-sm);
      pointer-events: none;
      animation: tip-in 120ms ease-out;
    }
    @keyframes tip-in {
      from {
        opacity: 0;
      }
    }
  `,
})
export class Tooltip {
  private readonly tip = viewChild.required<ElementRef<HTMLElement>>('tip');
  /** The element the mouse rests on, its title held aside. */
  private over: HTMLElement | null = null;
  private title = '';
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    const listeners: [string, (event: Event) => void][] = [
      ['pointerover', (e) => this.enter(e as PointerEvent)],
      ['pointerout', (e) => this.out(e as PointerEvent)],
      ['pointermove', () => this.follow()],
      ['pointerdown', () => this.hide()],
      ['wheel', () => this.hide()],
      ['keydown', () => this.hide()],
    ];
    for (const [type, listener] of listeners) {
      document.addEventListener(type, listener, { capture: true, passive: true });
    }
    inject(DestroyRef).onDestroy(() => {
      this.leave();
      for (const [type, listener] of listeners) {
        document.removeEventListener(type, listener, { capture: true });
      }
    });
  }

  private enter(event: PointerEvent) {
    const target = event.target as Element;
    if (this.over?.contains(target)) return;
    this.leave();
    const over = event.pointerType === 'mouse' ? target.closest<HTMLElement>('[title]') : null;
    if (!over?.title) return;
    this.over = over;
    this.holdTitle();
    this.timer = setTimeout(() => this.show(), SHOW_MS);
  }

  private out(event: PointerEvent) {
    const to = event.relatedTarget as Node | null;
    if (this.over && !(to && this.over.contains(to))) this.leave();
  }

  /** A title given again meanwhile is held aside too; an element gone takes its tip along. */
  private follow() {
    if (!this.over) return;
    if (!this.over.isConnected) {
      this.leave();
    } else if (this.over.hasAttribute('title')) {
      this.holdTitle();
      if (!this.tip().nativeElement.hidden) this.show();
    }
  }

  private holdTitle() {
    this.title = this.over!.title;
    this.over!.removeAttribute('title');
  }

  private show() {
    const over = this.over;
    if (!over?.isConnected || !this.title) return;
    const tip = this.tip().nativeElement;
    tip.textContent = this.title;
    tip.hidden = false;
    const at = over.getBoundingClientRect();
    const { width, height } = tip.getBoundingClientRect();
    const left = at.left + at.width / 2 - width / 2;
    const below = at.bottom + GAP;
    tip.style.left = `${Math.min(Math.max(left, GAP), innerWidth - width - GAP)}px`;
    tip.style.top = `${below + height > innerHeight - GAP ? at.top - GAP - height : below}px`;
  }

  /** A press or a key: the tip goes, and comes back only on another element. */
  private hide() {
    clearTimeout(this.timer);
    this.tip().nativeElement.hidden = true;
  }

  /** The title given back, unless the page gave it again meanwhile. */
  private leave() {
    this.hide();
    if (this.over && !this.over.hasAttribute('title')) this.over.title = this.title;
    this.over = null;
  }
}

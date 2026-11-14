import {
  Component,
  DestroyRef,
  ElementRef,
  afterNextRender,
  inject,
  Injector,
  signal,
} from '@angular/core';
import { closedByBack } from '../../core/back';
import { PHONE } from '../../core/platform';

interface Choice {
  index: number;
  label: string;
  /** The name of its group, on the first choice of a group. */
  group: string | null;
  disabled: boolean;
  selected: boolean;
}

interface Picking {
  select: HTMLSelectElement;
  title: string;
  choices: Choice[];
  /** Where the list goes on a computer: under the field, or over it near the bottom. */
  left: number;
  top: number | null;
  bottom: number | null;
  width: number;
}

const ROW_PX = 30;
const MAX_PX = 320;

/** What names a field: its label, or the row of the settings it is on. */
function titleOf(select: HTMLSelectElement): string {
  return (
    select.getAttribute('aria-label') ??
    select.labels?.[0]?.textContent?.trim() ??
    select.closest('.pr-row')?.querySelector('.pr-row__name')?.textContent?.trim() ??
    select.closest('[aria-label]')?.getAttribute('aria-label') ??
    ''
  );
}

function choicesOf(select: HTMLSelectElement): Choice[] {
  let group: HTMLOptGroupElement | null = null;
  return Array.from(select.options, (option, index) => {
    const parent =
      option.parentElement instanceof HTMLOptGroupElement ? option.parentElement : null;
    const heading = parent && parent !== group ? parent.label : null;
    group = parent;
    return {
      index,
      label: option.label || option.text,
      group: heading,
      disabled: option.disabled || (parent?.disabled ?? false),
      selected: index === select.selectedIndex,
    };
  });
}

/**
 * The choices of every drop-down list of Prisme, drawn by Prisme rather than by the
 * system: under the field on a computer, rising from the bottom on a phone. The list
 * itself stays the source of truth: a choice sets it and tells it changed, as the system
 * would. (The keyboard still opens the system's list, as it always has.)
 */
@Component({
  selector: 'app-select-picker',
  template: `
    @if (picking(); as p) {
      <div
        class="veil"
        animate.enter="pr-fade-in"
        animate.leave="pr-fade-out"
        [class.dim]="PHONE"
        (pointerdown)="close()"
      ></div>
      <div
        class="picker"
        [class.sheet]="PHONE"
        [animate.enter]="PHONE ? 'pr-rise-in' : 'pr-pop-in'"
        [animate.leave]="PHONE ? 'pr-rise-out' : 'pr-pop-out'"
        role="listbox"
        [attr.aria-label]="p.title || null"
        [style.left.px]="PHONE ? null : p.left"
        [style.top.px]="PHONE ? null : p.top"
        [style.bottom.px]="PHONE ? null : p.bottom"
        [style.min-width.px]="PHONE ? null : p.width"
        (keydown)="key($event)"
      >
        @if (PHONE && p.title) {
          <h4>{{ p.title }}</h4>
        }
        @for (c of p.choices; track c.index) {
          @if (c.group) {
            <div class="group">{{ c.group }}</div>
          }
          <button
            role="option"
            [attr.aria-selected]="c.selected"
            [class.is-on]="c.selected"
            [disabled]="c.disabled"
            (click)="pick(c)"
          >
            <span>{{ c.label }}</span>
            @if (c.selected) {
              <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8.5l3.2 3L13 4.5" /></svg>
            }
          </button>
        }
      </div>
    }
  `,
  styleUrl: './select-picker.scss',
  host: { '(document:keydown.escape)': 'close()' },
})
export class SelectPicker {
  protected readonly PHONE = PHONE;
  protected readonly picking = signal<Picking | null>(null);
  private readonly host: HTMLElement = inject(ElementRef).nativeElement;
  private readonly injector = inject(Injector);

  constructor() {
    // the system's list never opens under a finger or a mouse: Prisme's does
    const hold = (event: Event) => {
      const select = this.selectOf(event);
      if (select) event.preventDefault();
    };
    const open = (event: Event) => {
      const select = this.selectOf(event);
      if (!select) return;
      event.preventDefault();
      this.open(select);
    };
    document.addEventListener('mousedown', hold, true);
    document.addEventListener('click', open, true);
    inject(DestroyRef).onDestroy(() => {
      document.removeEventListener('mousedown', hold, true);
      document.removeEventListener('click', open, true);
    });
    if (PHONE) {
      closedByBack(
        () => this.picking() !== null,
        () => this.picking.set(null),
      );
    }
  }

  protected pick(choice: Choice) {
    const select = this.picking()?.select;
    this.picking.set(null);
    if (!select || choice.index === select.selectedIndex) return;
    select.selectedIndex = choice.index;
    select.dispatchEvent(new Event('input', { bubbles: true }));
    select.dispatchEvent(new Event('change', { bubbles: true }));
  }

  protected close() {
    this.picking.set(null);
  }

  /** Up and down go through the choices. */
  protected key(event: KeyboardEvent) {
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    if (!step) return;
    event.preventDefault();
    const buttons = Array.from(
      this.host.querySelectorAll<HTMLButtonElement>('.picker button:enabled'),
    );
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[Math.min(Math.max(at + step, 0), buttons.length - 1)]?.focus();
  }

  private selectOf(event: Event): HTMLSelectElement | null {
    const select = (event.target as Element | null)?.closest?.('select');
    return select && !select.disabled && !select.multiple ? select : null;
  }

  private open(select: HTMLSelectElement) {
    const rect = select.getBoundingClientRect();
    const choices = choicesOf(select);
    const height = Math.min(MAX_PX, choices.length * ROW_PX + 8);
    const below = innerHeight - rect.bottom - 8 >= height || rect.top < height;
    this.picking.set({
      select,
      title: titleOf(select),
      choices,
      left: Math.min(rect.left, innerWidth - rect.width - 8),
      top: below ? rect.bottom + 4 : null,
      bottom: below ? null : innerHeight - rect.top + 4,
      width: rect.width,
    });
    afterNextRender(
      () => {
        const picker = this.host.querySelector('.picker');
        const chosen = picker?.querySelector<HTMLElement>('.is-on');
        chosen?.scrollIntoView({ block: 'nearest' });
        if (!PHONE) (chosen ?? picker?.querySelector<HTMLElement>('button:enabled'))?.focus();
      },
      { injector: this.injector },
    );
  }
}

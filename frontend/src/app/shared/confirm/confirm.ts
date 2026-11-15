import {
  Component,
  ElementRef,
  Injectable,
  Injector,
  afterNextRender,
  effect,
  inject,
  signal,
} from '@angular/core';
import { PHONE } from '../../core/platform';
import { Dismiss, overlay } from '../overlay';

/** What is asked: a title, what follows, and the word that does it. */
export interface Question {
  title: string;
  detail: string;
  confirm: string;
  cancel: string;
}

interface Asking extends Question {
  answer: (yes: boolean) => void;
}

/** Asks the DJ before something that cannot be taken back. */
@Injectable({ providedIn: 'root' })
export class ConfirmService {
  readonly asking = signal<Asking | null>(null);

  /** True if the DJ confirms; a question asked meanwhile is answered no. */
  ask(question: Question): Promise<boolean> {
    this.answer(false);
    return new Promise((answer) => this.asking.set({ ...question, answer }));
  }

  answer(yes: boolean) {
    const asking = this.asking();
    if (!asking) return;
    this.asking.set(null);
    asking.answer(yes);
  }
}

/** The question of [`ConfirmService`]: in the middle of a computer's screen, rising from the
 * bottom of a phone's. */
@Component({
  selector: 'app-confirm',
  template: `
    @if (service.asking(); as a) {
      <div
        class="pr-veil pr-veil--dim"
        animate.enter="pr-fade-in"
        animate.leave="pr-fade-out"
        (dismiss)="service.answer(false)"
      ></div>
      <div
        [class.pr-popover]="!PHONE"
        [class.pr-sheet]="PHONE"
        [animate.enter]="PHONE ? 'pr-rise-in' : 'pr-pop-in'"
        [animate.leave]="PHONE ? 'pr-rise-out' : 'pr-pop-out'"
        role="alertdialog"
        aria-modal="true"
        [attr.aria-label]="a.title"
      >
        <h4>{{ a.title }}</h4>
        <p>{{ a.detail }}</p>
        <div class="actions">
          <button class="pr-btn no" (click)="service.answer(false)">{{ a.cancel }}</button>
          <button class="pr-btn pr-btn--light" (click)="service.answer(true)">
            {{ a.confirm }}
          </button>
        </div>
      </div>
    }
  `,
  styleUrl: './confirm.scss',
  imports: [Dismiss],
})
export class Confirm {
  protected readonly PHONE = PHONE;
  protected readonly service = inject(ConfirmService);

  constructor() {
    const host: HTMLElement = inject(ElementRef).nativeElement;
    const injector = inject(Injector);
    // on a computer, the keys go to the question; Enter at once keeps things as they are
    effect(() => {
      if (this.service.asking() && !PHONE) {
        afterNextRender(() => host.querySelector<HTMLButtonElement>('.no')?.focus(), {
          injector,
        });
      }
    });
    overlay(
      () => this.service.asking() !== null,
      () => this.service.answer(false),
    );
  }
}

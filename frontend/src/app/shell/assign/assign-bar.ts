import { Component, computed, inject } from '@angular/core';
import { AssignService } from '../../core/assign/assign.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { t } from '../../core/i18n/i18n';

/**
 * The bar of the assigning mode, over the page: keys or a controller's controls, what to do, and
 * the end of the mode.
 */
@Component({
  selector: 'app-assign-bar',
  template: `
    <b class="name">{{ 'Assign' | t }}</b>
    <div class="ways" role="radiogroup" [attr.aria-label]="'What to give' | t">
      <button
        role="radio"
        [attr.aria-checked]="assign.way() === 'keys'"
        (click)="assign.choose('keys')"
      >
        {{ 'Keyboard' | t }}
      </button>
      <button
        role="radio"
        [attr.aria-checked]="assign.way() === 'controller'"
        [disabled]="!assign.canLearn()"
        [title]="assign.canLearn() ? '' : ('Plug in a controller to give its controls' | t)"
        (click)="assign.choose('controller')"
      >
        {{ 'Controller' | t }}
      </button>
    </div>
    <span class="what">{{ what() }}</span>
    <span class="end">
      <kbd>{{ 'Esc' | t }}</kbd>
      <button class="done" (click)="assign.stop()">{{ 'Done' | t }}</button>
    </span>
  `,
  styles: `
    :host {
      display: flex;
      align-items: center;
      gap: 14px;
      min-height: 46px;
      padding: 0 12px 0 16px;
      background: var(--pr-light);
      color: var(--pr-on-light);
      animation: pr-fade-in 160ms ease-out;
    }
    .name {
      font: 700 13.5px var(--pr-font);
    }
    .ways {
      display: inline-flex;
      border: 1px solid rgb(11 12 14 / 0.22);
      border-radius: var(--pr-radius);
      overflow: hidden;
      button {
        padding: 7px 12px;
        border: none;
        background: transparent;
        color: rgb(11 12 14 / 0.7);
        font: 600 11px / 1 var(--pr-font);
        letter-spacing: 0.06em;
        text-transform: uppercase;
        cursor: pointer;
        &[aria-checked='true'] {
          background: var(--pr-on-light);
          color: var(--pr-light);
        }
        &:disabled {
          opacity: 0.4;
          cursor: default;
        }
      }
    }
    .what {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      color: rgb(11 12 14 / 0.78);
    }
    .end {
      margin-left: auto;
      display: inline-flex;
      align-items: center;
      gap: 10px;
    }
    kbd {
      font: 600 11px / 1 var(--pr-font-mono);
      padding: 3px 6px;
      border: 1px solid rgb(11 12 14 / 0.22);
      border-radius: var(--pr-radius-sm);
      color: rgb(11 12 14 / 0.75);
    }
    .done {
      height: 30px;
      padding: 0 14px;
      border: none;
      border-radius: var(--pr-radius);
      background: var(--pr-on-light);
      color: var(--pr-light);
      font: 600 11px / 1 var(--pr-font);
      letter-spacing: 0.06em;
      text-transform: uppercase;
      cursor: pointer;
    }
  `,
  host: { 'data-assign-ui': '' },
  imports: [TPipe],
})
export class AssignBar {
  protected readonly assign = inject(AssignService);
  protected readonly what = computed(() =>
    this.assign.way() === 'keys'
      ? t('Click an outlined button, then press the key to give it.')
      : t('Click an outlined button, then move the control on the controller to give it.'),
  );
}

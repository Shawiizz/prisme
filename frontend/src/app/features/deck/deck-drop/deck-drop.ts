import { Component, computed, inject, input } from '@angular/core';
import { TPipe } from '../../../core/i18n/t.pipe';
import { EngineService } from '../../../core/services/engine.service';
import { FileDropService } from '../../../core/services/file-drop.service';
import { deckLetter } from '../../../shared/time';

/**
 * What a track dragged from the file manager does on a deck, told over the whole deck while it
 * is dragged, lit under the pointer. Its host is where the deck takes the drop
 * (`data-file-drop`).
 */
@Component({
  selector: 'app-deck-drop',
  template: `
    @if (drops.dragged()?.audio) {
      <div class="target" [class.over]="over()" animate.enter="pr-fade-in">
        <div class="what">
          <span class="load">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M12 4v11M7 10l5 5 5-5M5 19h14" />
            </svg>
            {{ 'Load on {deck}' | t: { deck: letter() } }}
          </span>
          @if (title(); as title) {
            <small>{{ 'replaces {title}' | t: { title } }}</small>
          }
        </div>
      </div>
    }
  `,
  styles: `
    .target {
      position: absolute;
      inset: 0;
      z-index: 1;
      container-type: size;
      display: grid;
      place-items: center;
      padding: 0 var(--pr-space-3);
      border: 1.5px dashed var(--pr-line-2);
      border-radius: var(--pr-radius);
      background: rgb(10 11 13 / 0.72);
      color: var(--pr-text-2);
      font-weight: 600;
      letter-spacing: 0.02em;
      &.over {
        border-color: var(--pr-light);
        background-image: linear-gradient(var(--pr-light-veil) 0 0);
        color: var(--pr-text);
      }
    }
    .what {
      display: grid;
      justify-items: center;
      gap: 4px;
      min-width: 0;
      max-width: 100%;
    }
    .load {
      display: inline-flex;
      align-items: center;
      gap: 8px;
    }
    svg {
      width: 18px;
      height: 18px;
      fill: none;
      stroke: currentColor;
      stroke-width: 1.6;
      stroke-linecap: round;
      stroke-linejoin: round;
    }
    small {
      max-width: 100%;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
      font-weight: 400;
      font-size: var(--pr-size-small);
      color: var(--pr-text-3);
    }
    // a deck one line high: what is replaced beside what is done
    @container (height < 64px) {
      .what {
        grid-auto-flow: column;
        align-items: baseline;
        gap: 10px;
      }
    }
  `,
  imports: [TPipe],
})
export class DeckDrop {
  readonly deck = input.required<number>();
  protected readonly drops = inject(FileDropService);
  private readonly engine = inject(EngineService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly title = computed(() => this.engine.tracks()[this.deck()]?.title ?? null);
  protected readonly over = computed(() => this.drops.isOver({ kind: 'deck', deck: this.deck() }));
}

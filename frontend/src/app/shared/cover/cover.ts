import { Component, computed, input, linkedSignal } from '@angular/core';
import { convertFileSrc } from '@tauri-apps/api/core';

/** A track's cover thumbnail (served by the app's `cover://` protocol); empty if none. */
@Component({
  selector: 'app-cover',
  template: `
    @if (!missing()) {
      <img [src]="src()" alt="" loading="lazy" decoding="async" (error)="missing.set(true)" />
    }
  `,
  styles: `
    :host {
      display: block;
      overflow: hidden;
      border-radius: 2px;
      background: var(--pr-raise);
    }
    img {
      display: block;
      width: 100%;
      height: 100%;
      object-fit: cover;
    }
  `,
})
export class Cover {
  readonly path = input.required<string>();
  protected readonly src = computed(() => convertFileSrc(this.path(), 'cover'));
  protected readonly missing = linkedSignal({ source: this.path, computation: () => false });
}

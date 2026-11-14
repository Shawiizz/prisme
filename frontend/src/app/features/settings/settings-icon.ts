import { Component, input } from '@angular/core';
import { SettingsCategory } from '../../core/services/navigation.service';

/** The sign of a page of the settings. */
@Component({
  selector: 'app-settings-icon',
  template: `
    <svg viewBox="0 0 24 24" aria-hidden="true">
      @switch (page()) {
        @case ('audio') {
          <path d="M4 9h3l5-4v14l-5-4H4zM16 9.5a3.5 3.5 0 0 1 0 5M18.5 7a7 7 0 0 1 0 10" />
        }
        @case ('turntables') {
          <circle cx="12" cy="12" r="8.5" />
          <circle cx="12" cy="12" r="2.5" />
        }
        @case ('mixing') {
          <path d="M7 4v16M17 4v16M4 9h6M14 15h6" />
        }
        @case ('dj') {
          <path d="M4 17l4-6 4 3 4-7 4 5" />
        }
        @case ('library') {
          <path d="M5 4v16M10 4v16M15 5l4 15" />
        }
        @case ('controllers') {
          <rect x="3" y="7" width="18" height="10" rx="1.5" />
          <path d="M7 11v2M11 11v2M15 11h2" />
        }
        @case ('shortcuts') {
          <rect x="3" y="6" width="18" height="12" rx="1.5" />
          <path d="M7 10h1M11 10h1M15 10h2M8 14h8" />
        }
        @case ('interface') {
          <circle cx="12" cy="12" r="8.5" />
          <path d="M3.5 12h17M12 3.5c3 3 3 14 0 17M12 3.5c-3 3-3 14 0 17" />
        }
        @case ('about') {
          <circle cx="12" cy="12" r="8.5" />
          <path d="M12 11v5M12 8h.01" />
        }
      }
    </svg>
  `,
  styles: `
    :host {
      flex: none;
      display: block;
      width: var(--icon, 17px);
      height: var(--icon, 17px);
    }
    svg {
      display: block;
      width: 100%;
      height: 100%;
      fill: none;
      stroke: currentColor;
      stroke-width: 1.5;
      stroke-linecap: round;
      stroke-linejoin: round;
    }
  `,
})
export class SettingsIcon {
  readonly page = input.required<SettingsCategory>();
}

import { Component, inject } from '@angular/core';
import { closedByBack } from '../../core/back';
import { NavigationService } from '../../core/services/navigation.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { SETTINGS_PAGES } from '../settings/settings-catalog';
import { SettingsDialog } from '../settings/settings-dialog';
import { SettingsIcon } from '../settings/settings-icon';

/**
 * The settings on a phone, as a phone lays its own out: the pages as a list, each with
 * what it holds; a page touched takes the screen, and the back arrow or gesture returns to
 * the list.
 */
@Component({
  selector: 'app-phone-settings',
  template: `
    @if (navigation.settings()) {
      <app-settings-dialog />
    } @else {
      <h2>{{ 'Settings' | t }}</h2>
      <div class="pages">
        @for (page of pages; track page.id) {
          <button (click)="navigation.openSettings(page.id)">
            <app-settings-icon [page]="page.id" />
            <span class="names">
              <span>{{ page.label }}</span>
              <span>{{ page.description }}</span>
            </span>
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3l5 5-5 5" /></svg>
          </button>
        }
      </div>
    }
  `,
  styleUrl: './phone-settings.scss',
  imports: [SettingsDialog, SettingsIcon, TPipe],
})
export class PhoneSettings {
  protected readonly navigation = inject(NavigationService);
  protected readonly pages = SETTINGS_PAGES;

  constructor() {
    closedByBack(
      () => this.navigation.settings() !== null,
      () => this.navigation.closeSettings(),
    );
  }
}

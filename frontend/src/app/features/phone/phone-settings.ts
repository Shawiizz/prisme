import { Component, computed, inject, signal } from '@angular/core';
import { closedByBack } from '../../core/back';
import { NavigationService } from '../../core/services/navigation.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { SETTINGS, SETTINGS_PAGES, matches, normalize } from '../settings/settings-catalog';
import { SettingsDialog } from '../settings/settings-dialog';
import { SettingsIcon } from '../settings/settings-icon';

/**
 * The settings on a phone, as a phone lays its own out: a search over every setting, then
 * the pages as a list, each with what it holds; a page or a setting touched takes the
 * screen, and the back arrow or gesture returns to the list.
 */
@Component({
  selector: 'app-phone-settings',
  template: `
    @if (navigation.settings()) {
      <app-settings-dialog animate.enter="pr-push-in" />
    } @else {
      <h2>{{ 'Settings' | t }}</h2>
      <label class="search">
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <circle cx="7" cy="7" r="5" />
          <path d="M11 11l4 4" />
        </svg>
        <input
          type="search"
          [placeholder]="'Search the settings' | t"
          [value]="query()"
          (input)="query.set($any($event.target).value)"
          [attr.aria-label]="'Search the settings' | t"
        />
      </label>
      @if (found(); as settings) {
        @if (settings.length) {
          <div class="pages">
            @for (s of settings; track s.id) {
              <button (click)="navigation.openSettings(s.page, s.id)">
                <app-settings-icon [page]="s.page" />
                <span class="names">
                  <span>{{ s.label }}</span>
                  <span>{{ s.pageLabel }}</span>
                </span>
                <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3l5 5-5 5" /></svg>
              </button>
            }
          </div>
        } @else {
          <p class="none">{{ 'Nothing matches “{q}”' | t: { q: query() } }}</p>
        }
      } @else {
        <div class="pages" animate.enter="pr-back-in">
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
    }
  `,
  styleUrl: './phone-settings.scss',
  imports: [SettingsDialog, SettingsIcon, TPipe],
})
export class PhoneSettings {
  protected readonly navigation = inject(NavigationService);
  protected readonly pages = SETTINGS_PAGES;
  protected readonly query = signal('');
  /** The settings the search finds, by their name, what they do or their page; `null`
   * without a search. */
  protected readonly found = computed(() => {
    const words = normalize(this.query()).split(/\s+/).filter(Boolean);
    if (!words.length) return null;
    return SETTINGS.flatMap((s) => {
      const page = this.pages.find((p) => p.id === s.page);
      if (!page) return [];
      const text = `${s.label} ${s.description} ${page.label} ${page.keywords}`;
      return matches(words, text) ? [{ ...s, pageLabel: page.label }] : [];
    });
  });

  constructor() {
    closedByBack(
      () => this.navigation.settings() !== null,
      () => this.navigation.closeSettings(),
    );
  }
}

import { Component, Directive, computed, inject, input } from '@angular/core';
import { SETTING_BY_ID } from './settings-catalog';
import { SettingsSearch } from './settings-search';

/**
 * A row of the settings: the setting's name and what it does, from the catalog, then its
 * control. Hidden while the search does not find it, lit once when pointed out. A name or
 * a help of its own replaces the catalog's; a `.pr-row__help` given adds a line under it.
 */
@Component({
  selector: 'app-setting-row',
  template: `
    <div>
      <ng-content select=".pr-row__name">
        <span class="pr-row__name">{{ label() }}</span>
      </ng-content>
      <span class="pr-row__help">{{ help() ?? entry().description() }}</span>
      <ng-content select=".pr-row__help" />
    </div>
    <ng-content />
  `,
  styles: `
    :host-context(.phone) {
      .pr-row__name {
        font-size: 16px;
      }
      .pr-row__help {
        font-size: 13px;
      }
    }
  `,
  host: {
    '[class.pr-row]': 'shown()',
    '[hidden]': '!shown()',
    '[attr.data-setting]': 'id()',
    '[class.is-pointed]': 'search.pointed() === id()',
  },
})
export class SettingRow {
  readonly id = input.required<string>();
  /** The name, when the setting is not quite the catalog's here. */
  readonly name = input<string | null>(null);
  /** What it does, when the catalog's does not hold here. */
  readonly help = input<string | null>(null);

  protected readonly search = inject(SettingsSearch);
  protected readonly entry = computed(() => SETTING_BY_ID.get(this.id())!);
  /** The name shown: its control is called the same. */
  readonly label = computed(() => this.name() ?? this.entry().label());
  protected readonly shown = computed(() => this.search.finds(this.id()));
}

/** The control of a setting's row, called by the row's name. */
@Directive({
  selector: '[settingControl]',
  host: { '[attr.aria-label]': 'row.label()' },
})
export class SettingControl {
  protected readonly row = inject(SettingRow);
}

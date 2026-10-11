import { Component, computed, inject } from '@angular/core';
import { ControlService } from '../../core/control/control.service';
import { KeyboardService } from '../../core/keyboard/keyboard.service';

/** Settings page: the keyboard shortcuts, by group, with the keys as printed on the user's keyboard. */
@Component({
  selector: 'app-shortcuts-settings',
  templateUrl: './shortcuts-settings.html',
  styleUrl: './shortcuts-settings.scss',
})
export class ShortcutsSettings {
  protected readonly keyboard = inject(KeyboardService);
  private readonly controls = inject(ControlService);

  protected readonly groups = computed(() => {
    const groups = new Map<string, { key: string; label: string }[]>();
    for (const s of this.keyboard.shortcuts) {
      const control = this.controls.find(s.control);
      if (!control) continue;
      const line = { key: this.keyboard.label(s), label: control.label };
      groups.set(control.group, [...(groups.get(control.group) ?? []), line]);
    }
    return [...groups.entries()].map(([name, lines]) => ({ name, lines }));
  });
}

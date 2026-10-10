import { Component, computed, inject } from '@angular/core';
import { ControlService } from '../../core/control/control.service';
import { KeyboardService } from '../../core/keyboard/keyboard.service';
import { TPipe } from '../../core/i18n/t.pipe';

/** The keyboard shortcuts, by group, with the keys as printed on the user's keyboard. */
@Component({
  selector: 'app-shortcuts-dialog',
  templateUrl: './shortcuts-dialog.html',
  styleUrl: './shortcuts-dialog.scss',
  imports: [TPipe],
})
export class ShortcutsDialog {
  protected readonly keyboard = inject(KeyboardService);
  protected readonly controls = inject(ControlService);

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

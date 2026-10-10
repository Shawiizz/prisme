import { Component, computed, inject } from '@angular/core';
import { KeyboardService } from '../../core/keyboard/keyboard.service';
import { Shortcut } from '../../core/keyboard/shortcuts';

/** The keyboard shortcuts, by group, with the keys as printed on the user's keyboard. */
@Component({
  selector: 'app-shortcuts-dialog',
  templateUrl: './shortcuts-dialog.html',
  styleUrl: './shortcuts-dialog.scss',
})
export class ShortcutsDialog {
  protected readonly keyboard = inject(KeyboardService);

  protected readonly groups = computed(() => {
    const groups = new Map<string, Shortcut[]>();
    for (const s of this.keyboard.shortcuts) {
      groups.set(s.group, [...(groups.get(s.group) ?? []), s]);
    }
    return [...groups.entries()].map(([name, shortcuts]) => ({ name, shortcuts }));
  });
}

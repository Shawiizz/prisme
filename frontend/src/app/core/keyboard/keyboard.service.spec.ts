import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { Control } from '../control/controls';
import { ControlService } from '../control/control.service';
import { KeyboardService } from './keyboard.service';

describe('KeyboardService', () => {
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
    document.body.replaceChildren();
  });

  it('leaves a key pressed in a dialog to the dialog', async () => {
    mockIPC(() => undefined);
    const applied: string[] = [];
    vi.spyOn(TestBed.inject(ControlService), 'findInUse').mockImplementation((id): Control => ({
      id,
      group: '',
      label: '',
      kind: 'button',
      apply: (v) => void (v && applied.push(id)),
    }));
    await TestBed.inject(KeyboardService).start();
    document.body.innerHTML = `<button id="page"></button>
      <div role="alertdialog" aria-modal="true"><button id="no"></button></div>`;
    const enter = (on: string) =>
      document
        .getElementById(on)!
        .dispatchEvent(new KeyboardEvent('keydown', { code: 'Enter', bubbles: true }));

    enter('no');
    expect(applied).toEqual([]);
    enter('page');
    expect(applied).toEqual(['library.load']);
  });
});

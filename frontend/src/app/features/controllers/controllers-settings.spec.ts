import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { Controller } from '../../core/models/controllers';
import { ControllersSettings } from './controllers-settings';

const VCI_100: Controller = {
  name: 'VCI-100',
  hid: false,
  enabled: true,
  plugged: true,
  profile: 'Vestax VCI-100',
  profileFile: 'vestax-vci-100.json',
  offered: [
    { file: 'vestax-vci-100.json', name: 'Vestax VCI-100' },
    { file: 'vestax-vci-100-hile.json', name: 'Vestax VCI-100 (Hile Mod)' },
  ],
  verified: false,
  profileSource: 'shipped',
  replaces: null,
  exportFile: 'vestax-vci-100.json',
  listed: true,
  canExport: true,
};

describe('ControllersSettings', () => {
  afterEach(() => clearMocks());

  async function settings(controllers: Controller[], chosen: unknown[] = []) {
    mockIPC((command, args) => {
      if (command === 'list_controllers') return { controllers, canLearn: true };
      if (command === 'choose_controller_profile') chosen.push(args);
      if (['controller_mappings', 'controller_light_targets'].includes(command)) return [];
      return undefined;
    });
    const fixture = TestBed.createComponent(ControllersSettings);
    fixture.detectChanges();
    // the list asked for at the start comes
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
    return fixture.nativeElement as HTMLElement;
  }

  it('offer the other profiles of a controller, the one in use chosen', async () => {
    const chosen: unknown[] = [];
    const element = await settings([VCI_100], chosen);
    const select = element.querySelector<HTMLSelectElement>('.choice select')!;
    expect([...select.options].map((o) => o.text.trim())).toEqual([
      'Vestax VCI-100',
      'Vestax VCI-100 (Hile Mod)',
    ]);
    expect(select.value).toBe('vestax-vci-100.json');

    select.value = 'vestax-vci-100-hile.json';
    select.dispatchEvent(new Event('change'));
    await Promise.resolve();
    expect(chosen).toEqual([{ name: 'VCI-100', file: 'vestax-vci-100-hile.json' }]);
  });

  it('offer no choice to a controller its model alone fits', async () => {
    const element = await settings([{ ...VCI_100, offered: VCI_100.offered.slice(0, 1) }]);
    expect(element.querySelector('.choice')).toBeNull();
  });
});

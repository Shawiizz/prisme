import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { Update } from '@tauri-apps/plugin-updater';
import { UpdateService, isNewer } from './update.service';

describe('isNewer', () => {
  it('compares each number in turn, not the text', () => {
    expect(isNewer('0.3.10', '0.3.9')).toBe(true);
    expect(isNewer('0.4.0', '0.3.12')).toBe(true);
    expect(isNewer('1.0.0', '0.9.9')).toBe(true);
  });

  it('is false for the same version or an older one', () => {
    expect(isNewer('0.3.6', '0.3.6')).toBe(false);
    expect(isNewer('0.3.5', '0.3.6')).toBe(false);
  });
});

describe('UpdateService', () => {
  afterEach(() => clearMocks());

  it('has Prisme install the update, so that it closes first', async () => {
    const commands: string[] = [];
    mockIPC((command) => {
      commands.push(command);
    });
    const updates = TestBed.inject(UpdateService);
    const update = { downloadAndInstall: vi.fn() } as unknown as Update;
    updates.available.set({ version: '9.0.0', update, apk: null });
    await updates.install();
    expect(commands[0]).toBe('install_update');
    expect(update.downloadAndInstall).not.toHaveBeenCalled();
  });
});

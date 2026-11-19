import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { Update } from '@tauri-apps/plugin-updater';
import { EngineStatus } from '../models/engine';
import { EngineService } from './engine.service';
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

  it('has Prisme download, then install once the decks are stopped, even when one started during the download', async () => {
    const commands: string[] = [];
    const engine = TestBed.inject(EngineService);
    const updates = TestBed.inject(UpdateService);
    const playing = (on: boolean) =>
      engine.status.set({ decks: [{ playing: on, speed: on ? 1 : 0 }] } as EngineStatus);
    mockIPC((command) => {
      commands.push(command);
      if (command === 'download_update') playing(true);
    });
    const update = { downloadAndInstall: vi.fn() } as unknown as Update;
    playing(false);
    updates.available.set({ version: '9.9.9', update, apk: null });

    await updates.install();
    expect(commands).toEqual(['download_update']);
    expect(updates.installing()).toBe(false);
    expect(updates.waitsForDecks()).toBe(true);

    playing(false);
    await updates.install();
    expect(commands).toEqual(['download_update', 'install_update', 'plugin:process|restart']);
    expect(update.downloadAndInstall).not.toHaveBeenCalled();
  });

  it('hands the APK downloaded to Android, once the DJ let Prisme install applications', async () => {
    const commands: [string, unknown][] = [];
    const engine = TestBed.inject(EngineService);
    const updates = TestBed.inject(UpdateService);
    let allowed = false;
    mockIPC((command, args) => {
      commands.push([command, args]);
      if (command === 'install_apk') return allowed ? 'installer' : 'notAllowed';
      return undefined;
    });
    engine.status.set({ decks: [{ playing: false, speed: 0 }] } as EngineStatus);
    const apk = { url: 'https://github.com/x.apk', size: 42 };
    updates.available.set({ version: '9.9.9', update: null, apk });

    await updates.install();
    expect(commands.map(([c]) => c)).toEqual(['download_apk', 'install_apk']);
    expect(commands[0][1]).toEqual(expect.objectContaining({ url: apk.url, size: 42 }));
    expect(commands[1][1]).toEqual({ url: apk.url });
    expect(updates.needsLeave()).toBe(true);
    expect(updates.installing()).toBe(false);

    allowed = true;
    await updates.install();
    expect(updates.needsLeave()).toBe(false);
  });

  it('downloads the APK but keeps it while a deck plays', async () => {
    const commands: string[] = [];
    const engine = TestBed.inject(EngineService);
    const updates = TestBed.inject(UpdateService);
    mockIPC((command) => {
      commands.push(command);
      if (command === 'download_apk') {
        engine.status.set({ decks: [{ playing: true, speed: 1 }] } as EngineStatus);
      }
    });
    engine.status.set({ decks: [{ playing: false, speed: 0 }] } as EngineStatus);
    updates.available.set({ version: '9.9.9', update: null, apk: { url: 'x.apk', size: 1 } });

    await updates.install();
    expect(commands).toEqual(['download_apk']);
    expect(updates.waitsForDecks()).toBe(true);
    await updates.install();
    expect(commands).toEqual(['download_apk']);
  });
});

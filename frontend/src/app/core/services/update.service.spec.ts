import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { EngineStatus } from '../models/engine';
import { EngineService } from './engine.service';
import { UpdateService } from './update.service';

/** The backend's refusal while a deck plays. */
const DECKS_PLAY = { code: 'decksPlay', params: {}, message: 'a deck plays' };

describe('UpdateService', () => {
  afterEach(() => clearMocks());

  it('has Prisme download, then install once the backend lets it, the download kept meanwhile', async () => {
    const commands: string[] = [];
    const engine = TestBed.inject(EngineService);
    const updates = TestBed.inject(UpdateService);
    let playing = true;
    mockIPC((command) => {
      commands.push(command);
      if (command === 'install_update' && playing) throw DECKS_PLAY;
      return undefined;
    });
    engine.status.set({ decks: [{ plays: false }], plays: false } as EngineStatus);
    updates.available.set({ version: '9.9.9', apk: null });

    await updates.install();
    expect(commands).toEqual(['download_update', 'install_update']);
    expect(updates.installing()).toBe(false);
    expect(engine.error()).toBeNull();

    playing = false;
    await updates.install();
    expect(commands).toEqual([
      'download_update',
      'install_update',
      'install_update',
      'plugin:process|restart',
    ]);
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
    engine.status.set({ decks: [{ plays: false }], plays: false } as EngineStatus);
    const apk = { url: 'https://github.com/x.apk', size: 42 };
    updates.available.set({ version: '9.9.9', apk });

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

  it('waits while a deck plays', async () => {
    const commands: string[] = [];
    const engine = TestBed.inject(EngineService);
    const updates = TestBed.inject(UpdateService);
    mockIPC((command) => {
      commands.push(command);
    });
    engine.status.set({ decks: [{ plays: true }], plays: true } as EngineStatus);
    updates.available.set({ version: '9.9.9', apk: { url: 'x.apk', size: 1 } });

    expect(updates.waitsForDecks()).toBe(true);
    await updates.install();
    expect(commands).toEqual([]);
  });
});

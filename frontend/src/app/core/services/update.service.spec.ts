import { TestBed } from '@angular/core/testing';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
import { EngineService } from './engine.service';
import { UpdateService } from './update.service';
import { tellPreferences } from './preferences.service';
import type { EngineStatus, Preferences, UpdateState } from '../models/backend';

/** The page's tasks run, the backend's answers and events with them. */
const settle = () => new Promise((done) => setTimeout(done));

/** The preferences told, automatic updates on or off. */
function automatic(on: boolean) {
  tellPreferences({ ui: { language: null }, updates: { automatic: on } } as Preferences);
}

function playing(engine: EngineService, plays: boolean) {
  engine.status.set({ decks: [{ plays }], plays } as EngineStatus);
}

describe('UpdateService', () => {
  afterEach(() => clearMocks());

  describe('on a computer', () => {
    const commands: string[] = [];
    let told: UpdateState;

    beforeEach(() => {
      commands.length = 0;
      told = { stage: 'unknown' };
      mockIPC(
        (command) => {
          commands.push(command);
          return command === 'update_state' ? told : undefined;
        },
        { shouldMockEvents: true },
      );
      automatic(true);
    });

    it('shows the state the backend tells, first asked, then as it changes', async () => {
      told = { stage: 'latest' };
      const updates = TestBed.inject(UpdateService);
      await settle();
      expect(updates.state()).toEqual({ stage: 'latest' });
      expect(updates.offer()).toBeNull();

      await emit('update-state', { stage: 'ready', version: '9.9.9' });
      expect(updates.offer()).toEqual({ kind: 'restart', version: '9.9.9' });
    });

    it('offers a download in the title bar only where Prisme puts it in place itself', async () => {
      const updates = TestBed.inject(UpdateService);
      await settle();
      updates.state.set({ stage: 'available', version: '9.9.9', installsItself: true });
      expect(updates.offer()).toEqual({ kind: 'download', version: '9.9.9' });
      updates.state.set({ stage: 'available', version: '9.9.9', installsItself: false });
      expect(updates.offer()).toBeNull();
    });

    it('shows a download in the title bar only when the DJ asked for it', async () => {
      const updates = TestBed.inject(UpdateService);
      await settle();
      updates.state.set({ stage: 'downloading', version: '9.9.9', progress: 0.5 });
      expect(updates.offer()).toBeNull();
      automatic(false);
      expect(updates.offer()).toEqual({ kind: 'downloading', version: '9.9.9', progress: 0.5 });
    });

    it('restarts into the new version once the decks are stopped', async () => {
      const engine = TestBed.inject(EngineService);
      const updates = TestBed.inject(UpdateService);
      await settle();
      playing(engine, true);
      await updates.restart();
      expect(commands).not.toContain('restart_to_update');
      playing(engine, false);
      await updates.restart();
      expect(commands).toContain('restart_to_update');
      expect(engine.error()).toBeNull();
    });
  });

  it('hands the APK downloaded to Android, once the DJ let Prisme install applications', async () => {
    const commands: [string, unknown][] = [];
    let allowed = false;
    mockIPC((command, args) => {
      commands.push([command, args]);
      if (command === 'install_apk') return allowed ? 'installer' : 'notAllowed';
      return undefined;
    });
    const engine = TestBed.inject(EngineService);
    const updates = TestBed.inject(UpdateService);
    await settle();
    commands.length = 0;
    playing(engine, false);
    const apk = { url: 'https://github.com/x.apk', size: 42 };
    updates.apk.set({ version: '9.9.9', apk });

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

  it('keeps the APK for once the decks stop', async () => {
    const commands: string[] = [];
    mockIPC((command) => {
      commands.push(command);
    });
    const engine = TestBed.inject(EngineService);
    const updates = TestBed.inject(UpdateService);
    await settle();
    commands.length = 0;
    playing(engine, true);
    updates.apk.set({ version: '9.9.9', apk: { url: 'x.apk', size: 1 } });

    expect(updates.waitsForDecks()).toBe(true);
    await updates.install();
    expect(commands).toEqual([]);
  });
});

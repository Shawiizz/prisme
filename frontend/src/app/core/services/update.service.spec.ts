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

  it('installs once the decks are stopped, even when one started during the download', async () => {
    const commands: string[] = [];
    mockIPC((command) => void commands.push(command));
    const engine = TestBed.inject(EngineService);
    const updates = TestBed.inject(UpdateService);
    const playing = (on: boolean) =>
      engine.status.set({ decks: [{ playing: on, speed: on ? 1 : 0 }] } as EngineStatus);
    const update = {
      download: vi.fn(async () => playing(true)),
      install: vi.fn(async () => undefined),
    };
    playing(false);
    updates.available.set({ version: '9.9.9', update: update as unknown as Update, apk: null });

    await updates.install();
    expect(update.install).not.toHaveBeenCalled();
    expect(updates.installing()).toBe(false);
    expect(updates.waitsForDecks()).toBe(true);
    expect(commands).not.toContain('plugin:process|restart');

    playing(false);
    await updates.install();
    expect(update.download).toHaveBeenCalledOnce();
    expect(update.install).toHaveBeenCalledOnce();
    expect(commands).toContain('plugin:process|restart');
  });
});

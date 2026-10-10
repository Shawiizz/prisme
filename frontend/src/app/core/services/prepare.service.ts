import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { DirEntry } from '../models/library';

/**
 * The prepare list: tracks lined up for the next minutes of the set. The app takes a
 * track out once it has been played (`prepare-changed`).
 */
@Injectable({ providedIn: 'root' })
export class PrepareService {
  readonly list = signal<DirEntry[]>([]);

  async init(): Promise<void> {
    await listen('prepare-changed', () => this.refresh());
    await this.refresh();
  }

  async refresh(): Promise<void> {
    this.list.set(await invoke<DirEntry[]>('prepare_list'));
  }

  async add(paths: string[]): Promise<void> {
    await invoke('add_to_prepare', { paths });
    await this.refresh();
  }

  async remove(paths: string[]): Promise<void> {
    await invoke('remove_from_prepare', { paths });
    await this.refresh();
  }

  async move(path: string, index: number): Promise<void> {
    await invoke('move_in_prepare', { path, index });
    await this.refresh();
  }
}

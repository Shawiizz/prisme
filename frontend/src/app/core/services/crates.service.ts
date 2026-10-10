import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { Crate, DirEntry } from '../models/library';

/** The crates of the library (named sets of tracks) and their management. */
@Injectable({ providedIn: 'root' })
export class CratesService {
  readonly crates = signal<Crate[]>([]);

  async refresh(): Promise<void> {
    this.crates.set(await invoke<Crate[]>('list_crates'));
  }

  async create(name: string): Promise<number> {
    const id = await invoke<number>('create_crate', { name });
    await this.refresh();
    return id;
  }

  async rename(id: number, name: string): Promise<void> {
    await invoke('rename_crate', { id, name });
    await this.refresh();
  }

  async delete(id: number): Promise<void> {
    await invoke('delete_crate', { id });
    await this.refresh();
  }

  /** Returns how many of the tracks were not in the crate yet. */
  async add(id: number, paths: string[]): Promise<number> {
    const added = await invoke<number>('add_to_crate', { id, paths });
    await this.refresh();
    return added;
  }

  async remove(id: number, paths: string[]): Promise<void> {
    await invoke('remove_from_crate', { id, paths });
    await this.refresh();
  }

  tracks(id: number) {
    return invoke<DirEntry[]>('crate_tracks', { id });
  }

  name(id: number): string | undefined {
    return this.crates().find((c) => c.id === id)?.name;
  }
}

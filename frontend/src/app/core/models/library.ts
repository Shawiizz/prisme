export interface DirEntry {
  name: string;
  path: string;
  sizeBytes: number;
}

export interface DirListing {
  path: string;
  parent: string | null;
  folders: DirEntry[];
  tracks: DirEntry[];
}

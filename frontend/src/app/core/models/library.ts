import { BackendError } from '../errors';
import { MusicalKey } from './engine';
import { RowStems } from './stems';

/** The list browsed (see `SourceDto`). */
export type TrackSource =
  | { kind: 'folder'; path: string }
  | { kind: 'prepare' }
  | { kind: 'duplicates' }
  | { kind: 'elsewhere' }
  | { kind: 'crate'; id: number }
  | { kind: 'history'; id: number };

/** Where the search looks: the list browsed, or every analysed track. */
export type SearchScope = 'list' | 'library';

/** `order`: the order of the list itself (play order of a set); `mix`: how well the tracks
 * follow the deck they are compared with. */
export type SortColumn =
  | 'order'
  | 'title'
  | 'artist'
  | 'album'
  | 'genre'
  | 'year'
  | 'bpm'
  | 'key'
  | 'rating'
  | 'plays'
  | 'mix';

/** How a row is picked: alone, added or taken out (Ctrl), up to it (Shift), or alone unless it
 * is in the selection (a right click, a drag). */
export type Pick = 'only' | 'toggle' | 'range' | 'context';

/** A folder by its name. */
export interface Folder {
  name: string;
  path: string;
}

/** The folder shown: the music folder it is in, the folder above inside it, the folders in it. */
export interface FolderView {
  path: string;
  root: string | null;
  up: string | null;
  folders: Folder[];
}

/** A row of the library as the backend computes it (see `LibraryRowDto`). */
export interface LibraryRow {
  path: string;
  /** When it was played, in a set: a track played twice is two rows. */
  playedAtMs: number | null;
  /** Its place in its list, from 1, in a list with an order of its own. */
  position: number | null;
  title: string;
  artist: string | null;
  album: string | null;
  genre: string | null;
  year: number | null;
  bpm: number | null;
  key: MusicalKey | null;
  /** 0xRRGGBB */
  color: number | null;
  /** 0 (not rated) to 5. */
  rating: number;
  plays: number;
  /** Its analysis is known. */
  analysed: boolean;
  /** The file could not be decoded. */
  failed: boolean;
  stems: RowStems;
  selected: boolean;
  /** Played in the set going on. */
  played: boolean;
  /** Under none of the music folders. */
  elsewhere: boolean;
  previewing: boolean;
  /** The decks holding it. */
  decks: { deck: number; playing: boolean }[];
  /** Its key mixes with the one heard on the deck the tracks are compared with. */
  fits: boolean;
  /** The pitch that brings it to that deck's tempo, in percent. */
  pitch: number | null;
  /** Whether the tempo fader reaches it. */
  reachable: boolean;
}

/** The rows the table draws, from its `first`. */
export interface LibraryWindow {
  /** The latest sent has the highest. */
  seq: number;
  first: number;
  rows: LibraryRow[];
}

/** The library as the backend browses it (see `LibraryViewDto`). */
export interface LibraryView {
  seq: number;
  source: TrackSource;
  folder: FolderView | null;
  scope: SearchScope;
  /** The search applied. */
  filter: string;
  sort: { column: SortColumn; descending: boolean };
  ordered: boolean;
  removable: boolean;
  reorderable: boolean;
  /** How many rows are shown. */
  count: number;
  /** Where the cursor is among the rows shown. */
  cursor: number | null;
  /** Moved each time the cursor goes to another row (not when rows before it move). */
  cursorMoves: number;
  /** How many tracks the actions on the selection take. */
  selected: number;
  focus: 'tracks' | 'lists';
  pointed: { kind: 'prepare' } | { kind: 'crate'; id: number };
  /** The deck the tracks are compared with. */
  reference: number;
  /** Why the music folder shown at start could not be read. */
  unread: BackendError | null;
  window: LibraryWindow;
}

/** A track by its path, its title, and what tells it apart in a list: its tempo, its key. */
export interface NamedTrack {
  path: string;
  title: string;
  artist: string | null;
  bpm: number | null;
  key: MusicalKey | null;
}

/** The lists the library keeps (see `LibraryListsDto`). */
export interface LibraryLists {
  crates: Crate[];
  sessions: HistorySession[];
  /** How many tracks the prepare list holds, and the first. */
  prepare: number;
  next: NamedTrack | null;
  /** How many tracks are under none of the music folders. */
  elsewhere: number;
}

/** What the menu of tracks can do with them (see `LibraryMenuDto`). */
export interface LibraryMenu {
  count: number;
  first: NamedTrack | null;
  /** They can be taken out of the list shown, this one. */
  removable: TrackSource | null;
  /** How many of them are under none of the music folders: all, or none. */
  elsewhere: number;
  /** The folder they share, all from elsewhere, and the music folder it holds if it does. */
  folder: string | null;
  holds: string | null;
  /** The genre a genre given to them starts with. */
  genre: string;
  /** How many the installed models have stems to add to. */
  separable: number;
}

export interface Crate {
  id: number;
  name: string;
  /** The rules of a smart crate. */
  rules: SmartRules | null;
  trackCount: number;
}

export type TextField = 'title' | 'artist' | 'album' | 'genre' | 'comment' | 'fileName';

/** A rule of a smart crate; bounds are inclusive, a missing one is open. */
export type Rule =
  | { kind: 'text'; field: TextField; contains: string }
  | { kind: 'bpm'; min: number | null; max: number | null }
  | { kind: 'year'; min: number | null; max: number | null }
  | { kind: 'key'; camelot: string[] };

export interface SmartRules {
  /** Every rule must match; otherwise one is enough. */
  matchAll: boolean;
  rules: Rule[];
}

/** A run of the app that played tracks. */
export interface HistorySession {
  id: number;
  startedAtMs: number;
  playCount: number;
}

/** A folder looked into: the folders in it, and how many tracks. */
export interface DirListing {
  path: string;
  /** The music folder it is or is in, the deepest. */
  within: string | null;
  parent: string | null;
  folders: Folder[];
  tracks: number;
}

/** Background analysis of the music folder (`library-progress` event). */
export interface AnalysisProgress {
  total: number;
  done: number;
  failed: number;
  running: boolean;
  /** Held back while a deck plays. */
  paused: boolean;
}

/** A folder whose tracks make the library; the main one first. */
/** The music folders, and how far their analysis is. */
export interface FoldersState {
  /** The main one first. */
  folders: MusicFolder[];
  progress: AnalysisProgress;
  /** The run of the analysis `progress` is of: each look through the folders the next. */
  scan: number;
  /** The library's database could not be opened: what changes is lost at exit. */
  inMemory: boolean;
}

export interface MusicFolder {
  path: string;
  /** The folder the library started with, which stays. */
  main: boolean;
  /** False while it cannot be found (a drive unplugged): listed, its tracks kept. */
  present: boolean;
}

/** Where a folder to add is looked for first, named by its own folder (`3A2F-1C0B`, `C:`). */
export interface StoragePlace {
  kind: 'phone' | 'removable' | 'home' | 'drive';
  name: string;
  path: string;
}

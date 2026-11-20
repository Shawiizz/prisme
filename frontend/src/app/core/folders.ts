import { t } from './i18n/i18n';
import { MusicFolder } from './models/library';

/** A folder on the way to another, as the address bar of a file manager shows it. */
export interface PathStep {
  name: string;
  path: string;
}

const SEPARATORS = /[\\/]/;

function separatorOf(path: string): string {
  return path.includes('\\') ? '\\' : '/';
}

/** The last name of a path: "C:\Music\House" → "House"; a drive by its letter, "E:\" → "E:". */
export function lastName(path: string): string {
  return path.split(SEPARATORS).filter(Boolean).pop() ?? path;
}

/** The folder a file is in: "C:\Music\a.mp3" → "C:\Music"; at the top of a drive, "E:\". */
export function parentOf(path: string): string {
  const at = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  const parent = path.slice(0, at);
  return parent === '' || /^[A-Za-z]:$/.test(parent) ? path.slice(0, at + 1) : parent;
}

/** What a music folder is called: its name, the system's music folder in the UI's language. */
export function folderName(folder: MusicFolder): string {
  const name = lastName(folder.path);
  return folder.main && name === 'Music' ? t('Music') : name;
}

/** The folders from `root` down to `path`, which is in it. */
export function stepsFrom(root: string, path: string): PathStep[] {
  const separator = separatorOf(root);
  const steps = [{ name: lastName(root), path: root }];
  let at = root;
  for (const name of path.slice(root.length).split(SEPARATORS).filter(Boolean)) {
    // a root at the top of a drive (D:\) ends with the separator already
    at = at.endsWith(separator) ? at + name : at + separator + name;
    steps.push({ name, path: at });
  }
  return steps;
}

/** A path made short, the user's own folder left out: "C:\Users\…\Music", "~/Music". */
export function shortPath(path: string): string {
  return path
    .replace(/^([A-Za-z]:\\Users\\)[^\\]+/, '$1…')
    .replace(/^\/(?:home|Users)\/[^/]+/, '~');
}

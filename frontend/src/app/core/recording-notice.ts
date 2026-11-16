import { t } from './i18n/i18n';
import { Recording } from './models/engine';
import { formatClock } from '../shared/time';

/** What tells a recording ended where it went: its file, how long it lasts, in how many files,
 * its tracklist, what could not be written, and on a `phone`, which shows no folders, its
 * folder. */
export function recordingNotice(
  recording: Recording,
  phone: boolean,
): { text: string; detail: string } {
  const file = fileName(recording.path);
  const parts = recording.files.length;
  const detail = [
    formatClock(recording.seconds),
    ...(parts > 1 ? [t('{n} files', { n: parts })] : []),
    recording.tracklist
      ? t('tracklist {file}', { file: fileName(recording.tracklist) })
      : t('the tracklist could not be written'),
    ...(recording.failure ? [recording.failure] : []),
    ...(phone ? [folderOf(recording.path)] : []),
  ].join(' · ');
  const text = recording.failure
    ? t('Mix cut short, saved up to there: {file}', { file })
    : t('Mix saved: {file}', { file });
  return { text, detail };
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function folderOf(path: string): string {
  return path.slice(0, Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')));
}

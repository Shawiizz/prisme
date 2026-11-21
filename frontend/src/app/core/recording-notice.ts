import { Text, t } from './i18n/i18n';
import { formatClock } from '../shared/time';
import type { Recording, RecordingFailure } from './models/backend';

const FAILURE: Record<RecordingFailure, Text> = {
  diskFull: () => t('the disk is full'),
  notWritten: () => t('the file could not be written'),
  stopped: () => t('the recording stopped unexpectedly'),
};

/** What tells a recording ended where it went: its file, how long it lasts, in how many files,
 * its tracklist, what could not be written, and on a `phone`, which shows no folders, its
 * folder. */
export function recordingNotice(
  recording: Recording,
  phone: boolean,
): { text: string; detail: string } {
  const file = recording.fileName;
  const parts = recording.parts;
  const detail = [
    formatClock(recording.seconds),
    ...(parts > 1 ? [t('{n} files', { n: parts })] : []),
    recording.tracklist
      ? t('tracklist {file}', { file: recording.tracklist })
      : t('the tracklist could not be written'),
    ...(recording.failure ? [FAILURE[recording.failure]()] : []),
    ...(phone ? [recording.folder] : []),
  ].join(' · ');
  const text = recording.failure
    ? t('Mix cut short, saved up to there: {file}', { file })
    : t('Mix saved: {file}', { file });
  return { text, detail };
}

import { describe, expect, it } from 'vitest';
import { Recording } from './models/engine';
import { recordingNotice } from './recording-notice';

const FIRST = 'C:\\Users\\dj\\Documents\\Prisme Recordings\\Prisme 2026-11-16 21h30m05.wav';
const PHONE_FIRST =
  '/storage/emulated/0/Documents/Prisme Recordings/Prisme 2026-11-16 21h30m05.wav';
const SET: Recording = {
  path: FIRST,
  files: [FIRST],
  seconds: 3725,
  tracks: 21,
  tracklist: FIRST.replace('.wav', '.txt'),
  failure: null,
};

describe('recordingNotice', () => {
  it('names the file, its length and its tracklist', () => {
    expect(recordingNotice(SET, false)).toEqual({
      text: 'Mix saved: Prisme 2026-11-16 21h30m05.wav',
      detail: '1:02:05 · tracklist Prisme 2026-11-16 21h30m05.txt',
    });
  });

  it('says what could not be written, in how many files, and where on a phone', () => {
    const cut = {
      ...SET,
      path: PHONE_FIRST,
      files: [PHONE_FIRST, PHONE_FIRST.replace('.wav', ' 2.wav')],
      tracklist: null,
      failure: 'No space left on device',
    };
    const notice = recordingNotice(cut, true);
    expect(notice.text).toBe('Mix cut short, saved up to there: Prisme 2026-11-16 21h30m05.wav');
    expect(notice.detail).toBe(
      '1:02:05 · 2 files · the tracklist could not be written · No space left on device · /storage/emulated/0/Documents/Prisme Recordings',
    );
  });
});

import { describe, expect, it } from 'vitest';
import { Recording } from './models/engine';
import { recordingNotice } from './recording-notice';

const FIRST = 'C:\\Users\\dj\\Documents\\Prisme Recordings\\Prisme 2026-11-16 21h30m05.wav';
const PHONE_FIRST =
  '/storage/emulated/0/Documents/Prisme Recordings/Prisme 2026-11-16 21h30m05.wav';
const SET: Recording = {
  path: FIRST,
  fileName: 'Prisme 2026-11-16 21h30m05.wav',
  folder: 'C:\\Users\\dj\\Documents\\Prisme Recordings',
  parts: 1,
  seconds: 3725,
  tracks: 21,
  tracklist: 'Prisme 2026-11-16 21h30m05.txt',
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
    const cut: Recording = {
      ...SET,
      path: PHONE_FIRST,
      folder: '/storage/emulated/0/Documents/Prisme Recordings',
      parts: 2,
      tracklist: null,
      failure: 'diskFull',
    };
    const notice = recordingNotice(cut, true);
    expect(notice.text).toBe('Mix cut short, saved up to there: Prisme 2026-11-16 21h30m05.wav');
    expect(notice.detail).toBe(
      '1:02:05 · 2 files · the tracklist could not be written · the disk is full · /storage/emulated/0/Documents/Prisme Recordings',
    );
  });
});

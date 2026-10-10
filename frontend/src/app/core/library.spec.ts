import { DirEntry, TrackTags } from './models/library';
import { matchesSearch, trackTitle } from './services/library.service';

function entry(name: string, tags: Partial<TrackTags> | null): DirEntry {
  const details = tags && {
    bpm: null,
    key: null,
    failed: false,
    tags: { title: null, artist: null, album: null, genre: null, year: null, ...tags },
  };
  return { name, path: 'C:/Music/' + name, details };
}

describe('trackTitle', () => {
  it('prefers the title tag', () => {
    expect(trackTitle(entry('01 - track.mp3', { title: 'Strobe' }))).toBe('Strobe');
  });

  it('falls back to the file name without extension', () => {
    expect(trackTitle(entry('01 - track.mp3', null))).toBe('01 - track');
    expect(trackTitle(entry('untagged.flac', {}))).toBe('untagged');
  });
});

describe('matchesSearch', () => {
  const strobe = entry('track01.mp3', {
    title: 'Strobe',
    artist: 'deadmau5',
    genre: 'Progressive House',
  });

  it('needs every word, in any field', () => {
    expect(matchesSearch(strobe, ['deadmau5', 'strobe'])).toBe(true);
    expect(matchesSearch(strobe, ['progressive'])).toBe(true);
    expect(matchesSearch(strobe, ['strobe', 'techno'])).toBe(false);
  });

  it('looks into the file name too', () => {
    expect(matchesSearch(strobe, ['track01'])).toBe(true);
    expect(matchesSearch(entry('Mi Casa.mp3', null), ['casa'])).toBe(true);
  });

  it('matches everything when the search is empty', () => {
    expect(matchesSearch(strobe, [])).toBe(true);
  });
});

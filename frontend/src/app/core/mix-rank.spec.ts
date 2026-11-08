import { DirEntry } from './models/library';
import { MixContext, mixRank } from './mix-rank';

function track(name: string, bpm: number | null, camelot: string | null): DirEntry {
  return {
    name,
    path: 'C:/Music/' + name,
    details: {
      bpm,
      key: camelot ? { camelot, name: '' } : null,
      tags: { title: null, artist: null, album: null, genre: null, year: null },
      color: null,
      rating: 0,
      playCount: 0,
      failed: false,
      vocals: false,
    },
  };
}

const context: MixContext = {
  reference: { bpm: 128, camelot: '8A', path: 'C:/Music/playing' },
  pitchRange: 0.08,
  keyLock: true,
  playedTonight: new Set(['C:/Music/played']),
};

function order(tracks: DirEntry[], c = context): string[] {
  const rank = (t: DirEntry) => mixRank(t, c);
  const compare = (a: number[], b: number[]) => a.map((v, i) => v - b[i]).find((d) => d !== 0) ?? 0;
  return [...tracks].sort((a, b) => compare(rank(a), rank(b))).map((t) => t.name);
}

describe('mixRank', () => {
  it('puts harmony first, then the smallest reachable pitch change', () => {
    const tracks = [
      track('clash', 128, '3B'),
      track('boost', 128, '10A'),
      track('far', 140, '9A'),
      track('near', 126, '9A'),
      track('relative', 64.5, '8B'),
      track('unknown', null, null),
    ];
    expect(order(tracks)).toEqual(['relative', 'near', 'far', 'boost', 'clash', 'unknown']);
  });

  it('puts what played tonight or plays now last', () => {
    const tracks = [
      track('played', 128, '8A'),
      track('playing', 128, '8A'),
      track('fresh', 128, '3B'),
    ];
    expect(order(tracks)).toEqual(['fresh', 'played', 'playing']);
  });

  it('hears the key moved by the pitch without key lock', () => {
    // 121 BPM pitched up 5.8 % sounds a semitone higher: 1A becomes 8A
    const tracks = [track('moved', 121, '1A'), track('stays', 121, '8B')];
    expect(order(tracks, { ...context, keyLock: false })[0]).toBe('moved');
    expect(order(tracks)[0]).toBe('stays');
  });
});

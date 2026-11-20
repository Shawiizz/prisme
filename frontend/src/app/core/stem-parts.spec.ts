import { StemPart, partId } from './stem-parts';

describe('stem parts', () => {
  it('names the other instruments the melody in the actions', () => {
    expect(['vocals', 'instrumental', 'other'].map((p) => partId(p as StemPart))).toEqual([
      'vocals',
      'instrumental',
      'melody',
    ]);
  });
});

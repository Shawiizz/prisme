import { camelotCompatible, semitonesToMix, transposeCamelot } from './camelot';

describe('transposeCamelot', () => {
  it('moves 7 steps per semitone', () => {
    expect(transposeCamelot('8A', 1)).toEqual({ camelot: '3A', name: 'Bbm' });
    expect(transposeCamelot('8B', -1)).toEqual({ camelot: '1B', name: 'B' });
    expect(transposeCamelot('8A', 12)).toEqual({ camelot: '8A', name: 'Am' });
    expect(transposeCamelot('8A', 0)).toEqual({ camelot: '8A', name: 'Am' });
    expect(transposeCamelot('nope', 1)).toBeNull();
  });

  it('two semitones up is two steps clockwise', () => {
    expect(transposeCamelot('8A', 2)?.camelot).toBe('10A');
    expect(camelotCompatible('8A', transposeCamelot('8A', 7)!.camelot)).toBe(true);
  });
});

describe('semitonesToMix', () => {
  it('takes the shortest way to a compatible key', () => {
    expect(semitonesToMix('8A', '3A')).toBe(1);
    expect(semitonesToMix('8A', '1A')).toBe(-1);
    expect(semitonesToMix('8A', '10B')).toBe(2);
    expect(semitonesToMix('8A', '8B')).toBe(0);
    expect(semitonesToMix('?', '8B')).toBeNull();
  });

  it('leaves keys that already mix alone', () => {
    expect(semitonesToMix('4A', '5A')).toBe(0);
    expect(semitonesToMix('12B', '1B')).toBe(0);
  });

  it('always ends compatible', () => {
    for (const code of ['5B', '11A']) {
      for (let n = 1; n <= 12; n++) {
        for (const mode of ['A', 'B']) {
          const target = `${n}${mode}`;
          const s = semitonesToMix(code, target)!;
          expect(Math.abs(s)).toBeLessThanOrEqual(6);
          expect(camelotCompatible(transposeCamelot(code, s)!.camelot, target)).toBe(true);
        }
      }
    }
  });
});

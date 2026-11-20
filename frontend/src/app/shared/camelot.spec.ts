import { camelotCompatible, camelotEnergyBoost, transposeCamelot } from './camelot';

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

describe('camelotEnergyBoost', () => {
  it('is two steps up in the same mode, around the wheel', () => {
    expect(camelotEnergyBoost('8A', '10A')).toBe(true);
    expect(camelotEnergyBoost('11B', '1B')).toBe(true);
    expect(camelotEnergyBoost('12A', '2A')).toBe(true);
  });

  it('is not two steps down, nor the other mode', () => {
    expect(camelotEnergyBoost('10A', '8A')).toBe(false);
    expect(camelotEnergyBoost('8A', '10B')).toBe(false);
  });
});

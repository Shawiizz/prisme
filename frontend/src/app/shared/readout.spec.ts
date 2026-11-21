import { readout } from './readout';

describe('readout', () => {
  it('reads a gain in decibels with its sign, and silence as minus infinity', () => {
    expect(readout({ db: 3 })).toBe('+3.0 dB');
    expect(readout({ db: -4.25 })).toBe('−4.3 dB');
    expect(readout({ db: 0 })).toBe('0.0 dB');
    expect(readout({ db: null })).toBe('−∞ dB');
  });

  it('reads a share, the balance by its side and the filter by its kind', () => {
    expect(readout({ percent: 42 })).toBe('42 %');
    expect(readout({ balance: 0 })).toBe('L = R');
    expect(readout({ balance: -20 })).toBe('L 20 %');
    expect(readout({ balance: 35 })).toBe('R 35 %');
    expect(readout({ filter: 0 })).toBe('0');
    expect(readout({ filter: -35 })).toBe('LPF 35 %');
    expect(readout({ filter: 60 })).toBe('HPF 60 %');
  });
});

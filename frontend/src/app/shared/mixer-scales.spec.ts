import {
  balanceAmount,
  balanceKnob,
  bandKnob,
  eqGain,
  eqKnob,
  filterAmount,
  filterKnob,
  liveGain,
  liveKnob,
  readouts,
  trimGain,
  trimKnob,
} from './mixer-scales';

describe('mixer scales', () => {
  it('EQ: off, flat, boosted', () => {
    expect(eqGain(0)).toBe(0);
    expect(eqGain(0.5)).toBeCloseTo(1);
    expect(eqGain(1)).toBeCloseTo(10 ** (6 / 20));
    expect(eqGain(0.25)).toBeCloseTo(10 ** (-20 / 20));
  });

  it('EQ knob and gain round-trip', () => {
    for (const knob of [0.1, 0.3, 0.5, 0.75, 1]) {
      expect(eqKnob(eqGain(knob))).toBeCloseTo(knob);
    }
    expect(eqKnob(0)).toBe(0);
  });

  it('EQ knob of a band on a channel', () => {
    const strip = { trim: 1, eq: [0, 1, eqGain(0.75)], kills: [false, false, false], filter: 0 };
    expect(bandKnob(strip, 'low')).toBe(0);
    expect(bandKnob(strip, 'mid')).toBeCloseTo(0.5);
    expect(bandKnob(strip, 'high')).toBeCloseTo(0.75);
  });

  it('filter: low-pass left, open at the centre, high-pass right', () => {
    expect(filterAmount(0)).toBe(-1);
    expect(filterAmount(0.5)).toBe(0);
    expect(filterAmount(1)).toBe(1);
    expect(filterKnob(filterAmount(0.3))).toBeCloseTo(0.3);
  });

  it('trim: ±12 dB around flat', () => {
    expect(trimGain(0.5)).toBeCloseTo(1);
    expect(trimGain(1)).toBeCloseTo(10 ** (12 / 20));
    expect(trimKnob(trimGain(0.2))).toBeCloseTo(0.2);
  });

  it('live input: silent, 0 dB at three quarters, +12 dB at the end', () => {
    expect(liveGain(0)).toBe(0);
    expect(liveGain(0.75)).toBeCloseTo(1);
    expect(liveGain(1)).toBeCloseTo(10 ** (12 / 20));
    expect(liveKnob(liveGain(0.4))).toBeCloseTo(0.4);
    expect(liveKnob(0)).toBe(0);
  });

  it('balance: left only, both sides, right only', () => {
    expect([0, 0.5, 1].map(balanceAmount)).toEqual([-1, 0, 1]);
    expect(balanceKnob(balanceAmount(0.3))).toBeCloseTo(0.3);
  });

  it('reads the knobs in decibels and percents', () => {
    expect(readouts.live(0.75)).toBe('0.0 dB');
    expect(readouts.balance(0.5)).toBe('L = R');
    expect(readouts.balance(0.3)).toBe('L 40 %');
    expect(readouts.eq(0.5)).toBe('0.0 dB');
    expect(readouts.eq(0)).toBe('−∞ dB');
    expect(readouts.trim(1)).toBe('+12.0 dB');
    expect(readouts.filter(0.5)).toBe('0');
    expect(readouts.filter(0.25)).toBe('LPF 50 %');
    expect(readouts.percent(0.42)).toBe('42 %');
  });
});

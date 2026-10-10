import { eqGain, eqKnob, trimGain, trimKnob } from './mixer-scales';

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

  it('trim: ±12 dB around flat', () => {
    expect(trimGain(0.5)).toBeCloseTo(1);
    expect(trimGain(1)).toBeCloseTo(10 ** (12 / 20));
    expect(trimKnob(trimGain(0.2))).toBeCloseTo(0.2);
  });
});

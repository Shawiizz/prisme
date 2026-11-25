import { beatsAt, bpmAt, timeOf } from './beat-grid';
import type { Tempo } from './models/backend';

const STEADY: Tempo = { bpm: 120, firstBeatS: 0.25, beatsPerBar: 4, anchors: [], bpmAfter: 120 };

/** 120 BPM from beat 0 at 1 s to beat 8 at 5 s, then 96 BPM to beat 16 at 10 s, 100 after. */
const DRIFTING: Tempo = {
  bpm: 106.67,
  firstBeatS: 1,
  beatsPerBar: 4,
  anchors: [
    { beat: 0, timeS: 1 },
    { beat: 8, timeS: 5 },
    { beat: 16, timeS: 10 },
  ],
  bpmAfter: 100,
};

describe('beat grid', () => {
  it('counts the beats of a steady grid', () => {
    expect(beatsAt(STEADY, 1.25)).toBeCloseTo(2, 9);
    expect(timeOf(STEADY, -1)).toBeCloseTo(-0.25, 9);
    expect(bpmAt(STEADY, 50)).toBe(120);
  });

  it('follows a grid whose tempo changes, as the backend does', () => {
    expect(beatsAt(DRIFTING, 0)).toBeCloseTo(-2, 9);
    expect(beatsAt(DRIFTING, 3)).toBeCloseTo(4, 9);
    expect(beatsAt(DRIFTING, 7.5)).toBeCloseTo(12, 9);
    expect(beatsAt(DRIFTING, 10.6)).toBeCloseTo(17, 9);
    for (const beats of [-2, 4, 8, 12, 16, 17]) {
      expect(beatsAt(DRIFTING, timeOf(DRIFTING, beats))).toBeCloseTo(beats, 9);
    }
    expect(bpmAt(DRIFTING, 3)).toBeCloseTo(120, 9);
    expect(bpmAt(DRIFTING, 6)).toBeCloseTo(96, 9);
    expect(bpmAt(DRIFTING, 20)).toBe(100);
  });
});

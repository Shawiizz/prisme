import { Waveform } from '../../../core/models/engine';
import { envelopeHeights } from './envelope';

const SPS = 150;

/** `seconds` of four-to-the-floor at 150 BPM: a kick of 60 ms on each beat over a steady
 * mid and high band, every slice peaking at `peak`. */
function track(seconds: number, level: (s: number) => [number, number, number], peak = 250) {
  const n = seconds * SPS;
  const w: Waveform = {
    slicesPerSecond: SPS,
    peak: new Uint8Array(n).fill(peak),
    low: new Uint8Array(n),
    mid: new Uint8Array(n),
    high: new Uint8Array(n),
  };
  for (let i = 0; i < n; i++) {
    [w.low[i], w.mid[i], w.high[i]] = level(i / SPS);
  }
  return w;
}

const onKick = (s: number) => s % 0.4 < 0.06;
const kickSlice = (beat: number) => Math.round((beat * 0.4 + 0.03) * SPS);
const offbeatSlice = (beat: number) => Math.round((beat * 0.4 + 0.2) * SPS);

describe('envelopeHeights', () => {
  it('unfolds a limited master into its kicks', () => {
    const wall = track(60, (s) => [onKick(s) ? 240 : 150, 170, 110]);
    const h = envelopeHeights(wall);
    for (const beat of [10, 50, 120]) {
      expect(h[kickSlice(beat)]).toBeGreaterThan(0.95);
      expect(h[offbeatSlice(beat)]).toBeLessThan(0.45);
    }
  });

  it('sets the contrast by the track, not by its level', () => {
    const groove = (gain: number) => (s: number) =>
      [onKick(s) ? 200 : 60, 90, 40].map((v) => Math.round(v * gain)) as [number, number, number];
    const loud = envelopeHeights(track(30, groove(1)));
    const quiet = envelopeHeights(track(30, groove(0.5), 120));
    for (const i of [kickSlice(20), offbeatSlice(20)]) {
      expect(quiet[i]).toBeCloseTo(loud[i], 1);
    }
  });

  it('keeps a breakdown below the body of the track, and silence flat', () => {
    const h = envelopeHeights(
      track(60, (s) =>
        s < 1 ? [0, 0, 0] : s < 40 ? [onKick(s) ? 220 : 90, 120, 70] : [5, 40, 30],
      ),
    );
    expect(h[Math.round(0.5 * SPS)]).toBe(0);
    expect(h[Math.round(50 * SPS)]).toBeLessThan(0.3);
    expect(h[kickSlice(30)]).toBeGreaterThan(0.95);
    expect(envelopeHeights(track(2, () => [0, 0, 0])).every((v) => v === 0)).toBe(true);
  });
});

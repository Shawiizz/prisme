import { DeckStatus } from './models/engine';
import { octaveRatio, phaseOffset, pitchToMatch } from './sync';

function deck(bpm: number | null, beatPhase: number | null): DeckStatus {
  return {
    loaded: true,
    playing: true,
    positionS: 0,
    durationS: 300,
    speed: 1,
    bpm,
    beatPhase,
    sync: { locked: false, leader: false, rateOffset: 1 },
    looping: { startS: null, endS: null, active: false },
    slipMode: false,
    slipPositionS: null,
    vinyl: { enabled: false, mode: 'relative', signal: 'none', recordSpeed: 0, recordPositionS: null },
  };
}

describe('octaveRatio', () => {
  it('relates tempos an octave apart', () => {
    expect(octaveRatio(70, 140)).toBe(0.5);
    expect(octaveRatio(174, 87)).toBe(2);
    expect(octaveRatio(125, 128)).toBe(1);
  });
});

describe('phaseOffset', () => {
  it('is the nearest beat difference', () => {
    expect(phaseOffset(deck(128, 0.3), deck(128, 0.1))).toBeCloseTo(0.2);
    expect(phaseOffset(deck(128, 0.05), deck(128, 0.95))).toBeCloseTo(0.1);
    expect(phaseOffset(deck(128, 0.9), deck(128, 0.1))).toBeCloseTo(-0.2);
  });

  it('compares a double-time deck with twice the other phase', () => {
    expect(phaseOffset(deck(140, 0.5), deck(70, 0.25))).toBeCloseTo(0);
  });

  it('needs both beat grids', () => {
    expect(phaseOffset(deck(null, null), deck(128, 0.1))).toBeNull();
  });
});

describe('pitchToMatch', () => {
  it('is the speed change towards the other tempo', () => {
    expect(pitchToMatch(deck(125, 0), deck(128, 0))).toBeCloseTo(0.024);
    expect(pitchToMatch(deck(128, 0), deck(64, 0))).toBeCloseTo(0);
    expect(pitchToMatch(deck(null, 0), deck(128, 0))).toBeNull();
  });
});

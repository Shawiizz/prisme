import { PlayerShowing, crossfaderGain, heardDeck, sameShowing, stillTrue } from './lock-player';

describe('crossfaderGain', () => {
  it('lets a deck through on its side and cuts it on the other', () => {
    expect(crossfaderGain(0, -1, 'smooth')).toBeCloseTo(1);
    expect(crossfaderGain(0, 1, 'smooth')).toBeCloseTo(0);
    expect(crossfaderGain(1, -1, 'sharp')).toBeCloseTo(0);
    expect(crossfaderGain(3, 1, 'sharp')).toBeCloseTo(1);
  });

  it('keeps both sides at the middle', () => {
    expect(crossfaderGain(0, 0, 'smooth')).toBeCloseTo(Math.SQRT1_2);
    expect(crossfaderGain(1, 0, 'sharp')).toBe(1);
  });
});

describe('heardDeck', () => {
  const started = [10, 20, 0, 0];

  it('is none while no deck moves', () => {
    expect(heardDeck([], started, null)).toBeNull();
  });

  it('is the last started when a mixer of its own mixes them', () => {
    expect(heardDeck([0, 1], started, null)).toBe(1);
    expect(heardDeck([0, 1], [30, 20], null)).toBe(0);
  });

  it('is the loudest through the software mixer', () => {
    const levels = [0.9, 0.2];
    expect(heardDeck([0, 1], started, (d) => levels[d])).toBe(0);
  });

  it('is the last started of two as loud', () => {
    expect(heardDeck([0, 1], started, () => 0.7)).toBe(1);
  });
});

describe('sameShowing', () => {
  const showing: PlayerShowing = {
    decks: [{ deck: 0, title: 'One', artist: null, playing: true }],
    shown: 0,
    path: 'one.mp3',
    durationS: 300,
    controls: true,
  };

  it('is the same for a copy', () => {
    expect(sameShowing(showing, structuredClone(showing))).toBe(true);
  });

  it('differs when a deck stops or the controls go', () => {
    const stopped = { ...showing, decks: [{ ...showing.decks[0], playing: false }] };
    expect(sameShowing(showing, stopped)).toBe(false);
    expect(sameShowing(showing, { ...showing, controls: false })).toBe(false);
  });
});

describe('stillTrue', () => {
  const told = { showing: {} as PlayerShowing, positionS: 10, speed: 1, at: 0 };

  it('holds while the deck goes on as Android moves it', () => {
    expect(stillTrue(told, 12.2, 1, 2000)).toBe(true);
  });

  it('fails past a drift or a change of speed', () => {
    expect(stillTrue(told, 30, 1, 2000)).toBe(false);
    expect(stillTrue(told, 12, 0, 2000)).toBe(false);
  });
});

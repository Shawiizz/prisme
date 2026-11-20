import { PlayerShowing, sameShowing, stillTrue } from './lock-player';

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

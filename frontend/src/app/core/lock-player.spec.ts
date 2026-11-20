import { EngineStatus, TrackInfo } from './models/engine';
import { PlayerShowing, playerShowing, sameShowing, stillTrue } from './lock-player';

describe('playerShowing', () => {
  const track = { path: 'one.mp3', title: 'One', artist: null } as TrackInfo;
  const status = (player: number | null) =>
    ({ player, decks: [{ heard: true, durationS: 300 }, { heard: false }] }) as EngineStatus;

  it('shows the deck heard with its track', () => {
    const showing = playerShowing([track, null], status(0), [0, 1], true);
    expect([showing.shown, showing.path, showing.durationS]).toEqual([0, 'one.mp3', 300]);
    expect(showing.decks).toEqual([{ deck: 0, title: 'One', artist: null, playing: true }]);
  });

  it('waits for the track of the deck heard to reach the page', () => {
    const showing = playerShowing([track, null], status(1), [0, 1], true);
    expect([showing.shown, showing.path, showing.durationS]).toEqual([null, null, 0]);
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

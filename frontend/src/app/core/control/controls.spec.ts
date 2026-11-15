import { buildControls, Control, ControlActions, measuredControl } from './controls';

/** Controls whose actions record their calls; queries answer `true`. */
function recorder() {
  const calls: string[] = [];
  const actions = new Proxy(
    {},
    {
      get:
        (_, name: string) =>
        (...args: unknown[]) => {
          calls.push(`${name}(${args.join(',')})`);
          return true;
        },
    },
  ) as ControlActions;
  const controls = buildControls(actions);
  const find = (id: string): Control => controls.find((c) => c.id === id)!;
  return { calls, controls, find, actions };
}

describe('controls', () => {
  it('have unique ids', () => {
    const { controls } = recorder();
    expect(new Set(controls.map((c) => c.id)).size).toBe(controls.length);
  });

  it('give both decks the same controls', () => {
    const { controls } = recorder();
    const of = (deck: number) =>
      controls.filter((c) => c.group === `Deck ${deck}`).map((c) => c.id.slice(6));
    expect(of(1)).toEqual(of(2));
  });

  it('buttons act when pressed, held ones also when released', () => {
    const { calls, find } = recorder();
    find('deck1.play').apply(1);
    find('deck1.play').apply(0);
    find('deck2.cue').apply(1);
    find('deck2.cue').apply(0);
    find('deck1.hotcue3').apply(1);
    find('deck2.bend.up').apply(1);
    find('deck2.bend.up').apply(0);
    expect(calls).toEqual([
      'togglePlay(0)',
      'cue(1,true)',
      'cue(1,false)',
      'hotCue(0,2)',
      'bend(1,1)',
      'bend(1,0)',
    ]);
  });

  it('a pad hit softly is pressed all the same', () => {
    const { calls, find } = recorder();
    const soft = 20 / 127;
    find('deck1.hotcue3').apply(soft);
    find('deck2.cue').apply(soft);
    find('deck2.cue').apply(0);
    expect(calls).toEqual(['hotCue(0,2)', 'cue(1,true)', 'cue(1,false)']);
  });

  it('faders and encoders pass their value on', () => {
    const { calls, find } = recorder();
    find('deck2.eq.low').apply(0.25);
    find('mixer.crossfader').apply(1);
    find('library.browse').apply(-3);
    expect(calls).toEqual(['setEq(1,low,0.25)', 'setCrossfader(1)', 'browse(-3)']);
  });

  it('drive the master, the live sources and passthrough', () => {
    const { calls, find } = recorder();
    find('mixer.balance').apply(0.25);
    find('mixer.booth').apply(0.8);
    find('mixer.aux').apply(0.5);
    find('mixer.mic.on').apply(1);
    find('mixer.mic.on').apply(0);
    find('mixer.talkover').apply(1);
    find('deck2.passthrough').apply(1);
    expect(find('mixer.mic.cue').lit!()).toBe(true);
    expect(calls).toEqual([
      'setBalance(0.25)',
      'setBoothLevel(0.8)',
      'setLiveLevel(aux,0.5)',
      'toggleLive(mic)',
      'toggleTalkover()',
      'togglePassthrough(1)',
      'liveCued(mic)',
    ]);
  });

  it('light what is on', () => {
    const { calls, find } = recorder();
    expect(find('deck1.sample4').lit!()).toBe(true);
    expect(calls).toEqual(['samplePlaying(0,3)']);
    expect(find('deck1.loop.halve').lit).toBeUndefined();
  });

  it('read a jump or a loop of so many beats from its id', () => {
    const { calls, actions } = recorder();
    measuredControl(actions, 'deck2.jump.-8')!.apply(1);
    measuredControl(actions, 'deck1.loop.0.5')!.apply(1);
    expect(calls).toEqual(['beatJump(1,-8)', 'beatLoop(0,0.5)']);
    expect(measuredControl(actions, 'deck1.loop')).toBeUndefined();
    expect(measuredControl(actions, 'deck1.loop.-1')).toBeUndefined();
    expect(measuredControl(actions, 'deck9.jump.4')).toBeUndefined();
  });
});

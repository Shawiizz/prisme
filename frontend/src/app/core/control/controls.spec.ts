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

  it('light what is on', () => {
    const { calls, find } = recorder();
    expect(find('deck1.sample4').lit!()).toBe(true);
    expect(calls).toEqual(['samplePlaying(0,3)']);
    expect(find('deck1.loop.halve').lit).toBeUndefined();
  });

  it('give each effect slot its controls, the first under the ids of a single effect', () => {
    const { calls, find } = recorder();
    find('deck1.fx').apply(1);
    find('deck2.fx3.on').apply(1);
    find('deck1.fx2.reverb').apply(1);
    find('deck1.fx.beats').apply(-1);
    find('deck2.fx2.beats').apply(2);
    find('deck2.fx2.mix').apply(0.5);
    find('deck1.fx.part').apply(1);
    expect(find('deck1.fx3.gate').lit!()).toBe(true);
    expect(calls).toEqual([
      'toggleFx(0,0)',
      'setFxOn(1,2,true)',
      'pressFx(0,1,reverb)',
      'scaleFxBeats(0,0,0.5)',
      'scaleFxBeats(1,1,4)',
      'setFxMix(1,1,0.5)',
      'nextFxPart(0)',
      'fxHolds(0,2,gate)',
    ]);
    expect(find('deck1.fx2.part')).toBeUndefined();
  });

  it('preview the selected track, move through it, stop it, or load and play', () => {
    const { calls, find } = recorder();
    find('library.preview').apply(1);
    find('library.preview').apply(0);
    find('library.preview.seek').apply(-2);
    find('library.preview.stop').apply(1);
    find('library.load2.play').apply(1);
    expect(find('library.preview.seek').kind).toBe('encoder');
    expect(find('library.preview').lit!()).toBe(true);
    expect(calls).toEqual([
      'togglePreview()',
      'seekPreview(-2)',
      'stopPreview()',
      'loadSelectedAndPlay(1)',
      'previewing()',
    ]);
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

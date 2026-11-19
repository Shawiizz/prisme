import { buildControls, Control, ControlActions, measuredControl } from './controls';
import { Pickup } from './pickup';
import { Band, CrossfaderCurve, LiveSource } from '../models/engine';
import { nextTempoRange } from '../models/preferences';
import { eqGain, eqKnob, liveGain, liveKnob } from '../../shared/mixer-scales';

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
    expect(calls).toEqual([
      'setEq(1,low,0.25)',
      'eqPosition(1,low)',
      'setCrossfader(1)',
      'browse(-3)',
    ]);
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
    find('mixer.headphones.split').apply(1);
    find('deck2.headphones.solo').apply(1);
    find('deck1.mute').apply(1);
    expect(find('deck2.mute').lit!()).toBe(true);
    expect(find('mixer.mic.cue').lit!()).toBe(true);
    expect(calls).toEqual([
      'setBalance(0.25)',
      'setBoothLevel(0.8)',
      'setLiveLevel(aux,0.5)',
      'liveLevelPosition(aux)',
      'toggleLive(mic)',
      'talkover()',
      'setTalkover(false)',
      'passthrough(1)',
      'setPassthrough(1,false)',
      'splitCue()',
      'setSplitCue(false)',
      'soloHeadphones(1)',
      'muted(0)',
      'setMute(0,false)',
      'muted(1)',
      'liveCued(mic)',
    ]);
  });

  it('play sixteen hot cues and sixteen sounds', () => {
    const { calls, find } = recorder();
    find('deck1.hotcue16').apply(1);
    find('deck2.sample9').apply(1);
    expect(find('deck1.hotcue17')).toBeUndefined();
    expect(find('deck1.sample17')).toBeUndefined();
    expect(calls).toEqual(['hotCue(0,15)', 'triggerSample(1,8)']);
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

  it('give each slot a second knob, its period and its effect along a knob, focus and a hold', () => {
    const { calls, find } = recorder();
    find('deck1.fx2.param2').apply(0.3);
    find('deck2.fx3.held').apply(1);
    find('deck2.fx3.held').apply(0);
    find('deck1.fx.focus').apply(1);
    expect(calls).toEqual([
      'setFxParam2(0,1,0.3)',
      'holdFx(1,2,true)',
      'holdFx(1,2,false)',
      'toggleFxFocus(0,0)',
    ]);
    expect(find('deck1.fx.held').label).toBe('Effect 1 on while held');
  });

  /** Effect controls over a deck's chain as `fx` holds it. */
  function chainOf(fx: { kind: string; amount: number; param2: number; beats: number }[]) {
    const focus = { slot: null as number | null };
    const chain = { on: true };
    const actions = {
      fxFocused: () => focus.slot,
      toggleFxFocus: (_: number, slot: number) => {
        focus.slot = focus.slot === slot ? null : slot;
      },
      fxAmountPosition: (_: number, slot: number) => fx[slot].amount,
      setFxAmount: (_: number, slot: number, v: number) => (fx[slot].amount = v),
      setFxAmounts: (_: number, v: number) => fx.forEach((f) => (f.amount = v)),
      fxParam2Position: (_: number, slot: number) => fx[slot].param2,
      setFxParam2: (_: number, slot: number, v: number) => (fx[slot].param2 = v),
      fxBeats: (_: number, slot: number) => fx[slot].beats,
      setFxBeats: (_: number, slot: number, beats: number) => (fx[slot].beats = beats),
      fxKind: (_: number, slot: number) => fx[slot].kind,
      setFxKind: (_: number, slot: number, kind: string) => (fx[slot].kind = kind),
      fxChainOn: () => chain.on,
      setFxChainOn: (_: number, on: boolean) => (chain.on = on),
    } as unknown as ControlActions;
    const controls = buildControls(actions);
    return { find: (id: string) => controls.find((c) => c.id === id)!, chain };
  }

  const slots = () =>
    [0.1, 0.2, 0.3].map((amount) => ({ kind: 'echo', amount, param2: 0.5, beats: 1 }));

  it('turn the three effect knobs on the amounts, or on the focused effect', () => {
    const fx = slots();
    const { find } = chainOf(fx);
    const knobs = [1, 2, 3].map((k) => find(`deck1.fx.knob${k}`));
    expect(knobs.map((k) => k.position!())).toEqual([0.1, 0.2, 0.3]);
    knobs[2].apply(0.9);
    expect(fx[2].amount).toBe(0.9);

    find('deck1.fx2.focus').apply(1);
    expect(find('deck1.fx2.focus').lit!()).toBe(true);
    expect(find('deck1.fx.focus').lit!()).toBe(false);
    knobs[0].apply(0.6);
    knobs[1].apply(0.8);
    knobs[2].apply(0);
    expect(fx[1]).toEqual({ kind: 'echo', amount: 0.6, param2: 0.8, beats: 1 / 8 });
    expect(fx[0].amount).toBe(0.1);
    expect(knobs[2].position!()).toBe(0);

    find('deck1.fx2.focus').apply(1);
    expect(knobs[1].position!()).toBe(0.6);
  });

  it('move the period and the effect along a knob, keeping a controller within a step', () => {
    const fx = slots();
    const { find } = chainOf(fx);
    const period = find('deck1.fx3.period');
    const kind = find('deck1.fx3.kind');
    expect(period.position!()).toBeCloseTo(3.5 / 7);
    period.apply(1);
    expect(fx[2].beats).toBe(8);
    period.apply(0.2);
    expect(fx[2].beats).toBe(1 / 4);
    expect(period.position!()).toBe(0.2);
    fx[2].beats = 2;
    expect(period.position!()).toBeCloseTo(4.5 / 7);

    kind.apply(0.99);
    expect(fx[2].kind).toBe('bitcrusher');
    kind.apply(0.4);
    expect(fx[2].kind).toBe('flanger');
    expect(kind.position!()).toBe(0.4);
  });

  it('move the three amounts together, and switch the whole chain', () => {
    const fx = slots();
    const { find, chain } = chainOf(fx);
    expect(find('deck1.fx.super').position!()).toBeCloseTo(0.2);
    find('deck1.fx.super').apply(0.7);
    expect(fx.map((f) => f.amount)).toEqual([0.7, 0.7, 0.7]);
    find('deck1.fx.chain').apply(1);
    expect(chain.on).toBe(false);
    expect(find('deck1.fx.chain.off').lit!()).toBe(true);
    find('deck1.fx.chain.on').apply(1);
    expect(find('deck1.fx.chain').lit!()).toBe(true);
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

  it('turn the Auto DJ on and off, fade now, skip', () => {
    const { calls, find } = recorder();
    find('autodj.toggle').apply(1);
    find('autodj.toggle').apply(0);
    find('autodj.fade').apply(1);
    find('autodj.skip').apply(1);
    expect(find('autodj.toggle').lit!()).toBe(true);
    expect(find('autodj.toggle').kind).toBe('button');
    expect(calls).toEqual(['toggleAutoDj()', 'autoDjFade()', 'autoDjSkip()', 'autoDjOn()']);
  });

  it('brake, spin back while held, repeat, fader start', () => {
    const { calls, find } = recorder();
    find('deck1.brake').apply(1);
    find('deck1.brake').apply(0);
    find('deck2.spinback').apply(1);
    find('deck2.spinback').apply(0);
    find('deck1.repeat').apply(1);
    expect(find('deck2.repeat').lit!()).toBe(true);
    find('deck2.faderStart').apply(1);
    expect(calls).toEqual([
      'brake(0)',
      'spinBack(1,true)',
      'spinBack(1,false)',
      'repeating(0)',
      'setRepeat(0,false)',
      'repeating(1)',
      'faderStart(1)',
      'setFaderStart(1,false)',
    ]);
  });

  it('reverse, switched or held, lit while the track plays backwards', () => {
    const { calls, find } = recorder();
    find('deck1.reverse').apply(1);
    find('deck2.reverse.held').apply(1);
    find('deck2.reverse.held').apply(0);
    expect(find('deck2.reverse.held').lit!()).toBe(true);
    expect(calls).toEqual([
      'reversing(0)',
      'setReverse(0,false)',
      'holdReverse(1,true)',
      'holdReverse(1,false)',
      'backwards(1)',
    ]);
  });

  it('go to the start, stopped, playing or as it was, and near the end', () => {
    const { calls, find } = recorder();
    find('deck1.start').apply(1);
    find('deck2.start.play').apply(1);
    find('deck1.start.keep').apply(1);
    find('deck2.end').apply(1);
    expect(calls).toEqual(['toStart(0,stop)', 'toStart(1,play)', 'toStart(0,goto)', 'toEnd(1)']);
  });

  it('jump and loop by the size of the deck, which a step halves or doubles', () => {
    const calls: string[] = [];
    const actions = new Proxy(
      {},
      {
        get:
          (_, name: string) =>
          (...args: unknown[]) => {
            calls.push(`${name}(${args.join(',')})`);
            return name === 'size' || name === 'loopBeats' ? 8 : true;
          },
      },
    ) as ControlActions;
    const find = (id: string) => buildControls(actions).find((c) => c.id === id)!;
    find('deck2.jump.back').apply(1);
    find('deck1.jump.forward').apply(1);
    find('deck1.loop.size').apply(1);
    find('deck1.size.halve').apply(1);
    find('deck2.size.double').apply(1);
    find('deck2.size').apply(-2);
    expect(find('deck1.size').kind).toBe('encoder');
    expect(find('deck1.loop.size').lit!()).toBe(true);
    expect(calls).toEqual([
      'size(1)',
      'beatJump(1,-8)',
      'size(0)',
      'beatJump(0,8)',
      'sizedLoop(0)',
      'scaleSize(0,0.5)',
      'scaleSize(1,2)',
      'scaleSize(1,0.25)',
      'loopBeats(0)',
      'size(0)',
    ]);
  });

  it('play a slice while held, lit on the slice playing', () => {
    const { calls, find } = recorder();
    find('deck2.slice3').apply(1);
    find('deck2.slice3').apply(0);
    expect(find('deck1.slice8').lit!()).toBe(false);
    expect(find('deck1.slice9')).toBeUndefined();
    expect(calls).toEqual(['slice(1,2,true)', 'slice(1,2,false)', 'slicePlaying(0)']);
  });

  it('cut the deck on the beats while held, from a sixteenth to a half', () => {
    const { calls, find } = recorder();
    find('deck1.cut1').apply(1);
    find('deck1.cut1').apply(0);
    find('deck2.cut4').apply(1);
    expect(find('deck2.cut4').lit!()).toBe(false);
    expect(find('deck1.cut5')).toBeUndefined();
    expect(find('deck1.cut1').label).toBe('Fader cut on 1/16 while held');
    expect(calls).toEqual(['cut(0,0,true)', 'cut(0,0,false)', 'cut(1,3,true)', 'cutPlaying(1)']);
  });

  it('move the range of a tempo fader, or of them all, either way', () => {
    const { calls, find } = recorder();
    find('deck2.tempoRange').apply(1);
    find('deck1.tempoRange.previous').apply(1);
    find('general.tempoRange').apply(1);
    find('general.tempoRange.previous').apply(1);
    expect(calls).toEqual([
      'cycleDeckTempoRange(1,1)',
      'cycleDeckTempoRange(0,-1)',
      'cycleTempoRange(1)',
      'cycleTempoRange(-1)',
    ]);
    expect(nextTempoRange(0.08, 1)).toBe(0.16);
    expect(nextTempoRange(0.5, 1)).toBe(0.08);
    expect(nextTempoRange(0.08, -1)).toBe(0.5);
    expect(nextTempoRange(0.1, 1)).toBe(0.08);
  });

  it('give each switch a key putting it on and one putting it off, whatever it is', () => {
    const { calls, controls, find } = recorder();
    find('deck1.sync.on').apply(1);
    find('deck2.keylock.off').apply(1);
    find('general.quantize.on').apply(1);
    find('mixer.crossfader.reverse.off').apply(1);
    find('deck1.vinyl.on').apply(0);
    expect(find('deck2.mute.on').lit!()).toBe(true);
    expect(find('deck2.mute.off').lit!()).toBe(false);
    expect(calls).toEqual([
      'setSync(0,true)',
      'setKeyLock(1,false)',
      'setQuantize(true)',
      'setCrossfaderReverse(false)',
      'muted(1)',
      'muted(1)',
    ]);
    const switches = [
      'deck1.sync',
      'deck1.keylock',
      'general.quantize',
      'deck1.slip',
      'deck1.vinyl',
      'deck1.passthrough',
      'mixer.crossfader.reverse',
      'mixer.headphones.split',
      'deck1.headphones',
      'deck1.repeat',
      'deck1.faderStart',
      'deck1.mute',
      'deck1.reverse',
      'mixer.talkover',
      'deck1.jog.scratch',
    ];
    const ids = new Set(controls.map((c) => c.id));
    expect(
      switches.flatMap((s) => [s, `${s}.on`, `${s}.off`]).filter((id) => !ids.has(id)),
    ).toEqual([]);
    expect(find('deck1.keylock.on').label).toBe('Key lock: on');
  });

  it('open the next or previous list, go into a list or load a track, move across', () => {
    const { calls, find } = recorder();
    find('library.list.next').apply(1);
    find('library.list.previous').apply(1);
    find('library.go').apply(1);
    find('library.left').apply(1);
    find('library.right').apply(1);
    expect(calls).toEqual([
      'openNextList(1)',
      'openNextList(-1)',
      'go()',
      'setFocus(lists)',
      'setFocus(tracks)',
    ]);
  });

  it('a smart sample pad loads an empty slot, plays a loaded one, stops a playing one', () => {
    const slots = { loaded: false, playing: false };
    const calls: string[] = [];
    const actions = new Proxy(
      {},
      {
        get:
          (_, name: string) =>
          (...args: unknown[]) => {
            if (name === 'sampleLoaded') return slots.loaded;
            if (name === 'samplePlaying') return slots.playing;
            calls.push(`${name}(${args.join(',')})`);
            return true;
          },
      },
    ) as ControlActions;
    const pad = buildControls(actions).find((c) => c.id === 'deck2.sample3.smart')!;
    const press = () => {
      pad.apply(1);
      pad.apply(0);
      return pad.state!();
    };
    expect(press()).toBe('off');
    slots.loaded = true;
    expect(press()).toBe('loaded');
    slots.playing = true;
    expect(press()).toBe('playing');
    expect(pad.lit!()).toBe(true);
    expect(calls).toEqual(['loadSample(1,2)', 'triggerSample(1,2)', 'stopSample(1,2)']);
  });

  it('light a loop playing, and the beats in phase, ahead or behind', () => {
    const calls: string[] = [];
    const actions = new Proxy(
      {},
      {
        get:
          (_, name: string) =>
          (...args: unknown[]) => {
            calls.push(`${name}(${args.join(',')})`);
            return name === 'phase' ? 'ahead' : true;
          },
      },
    ) as ControlActions;
    const find = (id: string) => buildControls(actions).find((c) => c.id === id)!;
    expect(find('deck1.loop.any').state!()).toBe('on');
    expect(find('deck2.phase.ahead').lit!()).toBe(true);
    expect(find('deck2.phase.behind').state!()).toBe('off');
    expect(find('deck2.phase').lit!()).toBe(false);
    expect(find('deck1.phase.ahead').kind).toBe('light');
    expect(calls).toEqual(['looping(0)', 'phase(1)', 'phase(1)', 'phase(1)']);
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

  it('play a sample as hard as its pad is hit, the sampler level left alone', () => {
    const { calls, actions } = recorder();
    const pad = measuredControl(actions, 'deck2.sample3.velocity')!;
    pad.apply(0.25);
    pad.apply(0);
    expect(calls).toEqual(['triggerSample(1,2,0.25)']);
  });

  /** A controller's fader sending each of `values` in turn, taken over as `ControllerService` does. */
  function turn(control: Control, values: number[]) {
    const pickup = new Pickup();
    for (const v of values) if (pickup.takes(v, control.position!())) control.apply(v);
  }

  /** A 7-bit knob from full left to `to`. */
  const knob = (to: number) => Array.from({ length: Math.round(to * 127) + 1 }, (_, i) => i / 127);

  it('bring an EQ band and a live input back from their silent end', () => {
    const levels = { eq: 0, mic: 0 };
    const controls = buildControls({
      setEq: (_: number, __: Band, v: number) => (levels.eq = eqGain(v)),
      eqPosition: () => eqKnob(levels.eq),
      setLiveLevel: (_: LiveSource, v: number) => (levels.mic = liveGain(v)),
      liveLevelPosition: () => liveKnob(levels.mic),
    } as unknown as ControlActions);
    const find = (id: string) => controls.find((c) => c.id === id)!;
    turn(find('deck1.eq.low'), knob(0.5));
    turn(find('mixer.mic'), knob(0.75));
    expect(levels.eq).toBeCloseTo(1, 1);
    expect(levels.mic).toBeCloseTo(1, 1);

    levels.eq = 0;
    const low = find('deck1.eq.low');
    for (let step = 0; step < 4; step++) low.apply(low.position!() + 1 / 64);
    expect(levels.eq).toBeGreaterThan(0);
  });

  it('switch the crossfader curve along a knob', () => {
    let curve: CrossfaderCurve = 'smooth';
    const controls = buildControls({
      crossfaderCurve: () => curve,
      setCrossfaderCurve: (c: CrossfaderCurve) => (curve = c),
    } as unknown as ControlActions);
    const fader = controls.find((c) => c.id === 'mixer.crossfaderCurve')!;
    turn(fader, knob(1));
    expect(curve).toBe('sharp');
    turn(fader, knob(1).reverse());
    expect(curve).toBe('smooth');
  });
});

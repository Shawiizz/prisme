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
      'perform(deck1.play,1,false)',
      'perform(deck1.play,0,false)',
      'perform(deck2.cue,1,false)',
      'perform(deck2.cue,0,false)',
      'perform(deck1.hotcue3,1,false)',
      'perform(deck2.bend.up,1,false)',
      'perform(deck2.bend.up,0,false)',
    ]);
  });

  it('faders and encoders pass their value on', () => {
    const { calls, find } = recorder();
    find('deck2.eq.low').apply(0.25);
    find('mixer.crossfader').apply(1);
    find('library.browse').apply(-3);
    expect(calls).toEqual([
      'perform(deck2.eq.low,0.25,false)',
      'perform(mixer.crossfader,1,false)',
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
    expect(calls).toEqual([
      'perform(mixer.balance,0.25,false)',
      'perform(mixer.booth,0.8,false)',
      'perform(mixer.aux,0.5,false)',
      'perform(mixer.mic.on,1,false)',
      'perform(mixer.mic.on,0,false)',
      'perform(mixer.talkover,1,false)',
      'perform(deck2.passthrough,1,false)',
      'perform(mixer.headphones.split,1,false)',
      'perform(deck2.headphones.solo,1,false)',
      'perform(deck1.mute,1,false)',
    ]);
  });

  it('play sixteen hot cues and sixteen sounds', () => {
    const { calls, find } = recorder();
    find('deck1.hotcue16').apply(1);
    find('deck2.sample9').apply(1);
    expect(find('deck1.hotcue17')).toBeUndefined();
    expect(find('deck1.sample17')).toBeUndefined();
    expect(calls).toEqual(['perform(deck1.hotcue16,1,false)', 'perform(deck2.sample9,1,false)']);
  });

  it('light only what the app keeps, the backend lighting the rest', () => {
    const { calls, find } = recorder();
    expect(find('general.library').lit!()).toBe(true);
    expect(calls).toEqual(['libraryOnly()']);
    expect(find('deck1.sample4').lit).toBeUndefined();
    expect(find('deck1.loop.halve').lit).toBeUndefined();
    expect(find('deck1.phase.ahead').kind).toBe('light');
    expect(find('deck1.phase.ahead').lit).toBeUndefined();
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
    expect(calls).toEqual([
      'perform(deck1.fx,1,false)',
      'perform(deck2.fx3.on,1,false)',
      'pressFx(0,1,reverb)',
      'perform(deck1.fx.beats,-1,true)',
      'perform(deck2.fx2.beats,2,true)',
      'perform(deck2.fx2.mix,0.5,false)',
      'nextFxPart(0)',
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
      'perform(deck1.fx2.param2,0.3,false)',
      'perform(deck2.fx3.held,1,false)',
      'perform(deck2.fx3.held,0,false)',
      'toggleFxFocus(0,0)',
    ]);
    expect(find('deck1.fx.held').label).toBe('Effect 1 on while held');
  });

  /** Effect controls over a deck's chain as `fx` holds it, the backend performing their
   * amounts, second knobs and periods (along a knob of 7 steps, from 1/8 beat). */
  function chainOf(fx: { kind: string; amount: number; param2: number; beats: number }[]) {
    const focus = { slot: null as number | null };
    const actions = {
      perform: (id: string, v: number) => {
        const [, n, name] = /^deck1\.fx(\d?)\.(\w+)$/.exec(id)!;
        const f = fx[n ? Number(n) - 1 : 0];
        if (name === 'amount') f.amount = v;
        if (name === 'param2') f.param2 = v;
        if (name === 'period') f.beats = 2 ** (Math.min(Math.floor(v * 7), 6) - 3);
      },
      fxFocused: () => focus.slot,
      toggleFxFocus: (_: number, slot: number) => {
        focus.slot = focus.slot === slot ? null : slot;
      },
      fxAmountPosition: (_: number, slot: number) => fx[slot].amount,
      fxParam2Position: (_: number, slot: number) => fx[slot].param2,
      fxBeats: (_: number, slot: number) => fx[slot].beats,
    } as unknown as ControlActions;
    const controls = buildControls(actions);
    return { find: (id: string) => controls.find((c) => c.id === id)! };
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

    find('deck1.fx3.focus').apply(1);
    knobs[2].apply(0.2);
    expect(fx[2].beats).toBe(1 / 4);
    expect(knobs[2].position!()).toBe(0.2);
    fx[2].beats = 2;
    expect(knobs[2].position!()).toBeCloseTo(4.5 / 7);
  });

  it('preview the selected track, move through it, stop it, or load and play', () => {
    const { calls, find } = recorder();
    find('library.preview').apply(1);
    find('library.preview').apply(0);
    find('library.preview.seek').apply(-2);
    find('library.preview.stop').apply(1);
    find('library.load2.play').apply(1);
    expect(find('library.preview.seek').kind).toBe('encoder');
    expect(calls).toEqual([
      'togglePreview()',
      'seekPreview(-2)',
      'stopPreview()',
      'loadSelectedAndPlay(1)',
    ]);
  });

  it('turn the Auto DJ on and off, fade now, skip', () => {
    const { calls, find } = recorder();
    find('autodj.toggle').apply(1);
    find('autodj.toggle').apply(0);
    find('autodj.fade').apply(1);
    find('autodj.skip').apply(1);
    expect(find('autodj.toggle').kind).toBe('button');
    expect(calls).toEqual([
      'perform(autodj.toggle,1,false)',
      'perform(autodj.toggle,0,false)',
      'perform(autodj.fade,1,false)',
      'perform(autodj.skip,1,false)',
    ]);
  });

  it('have the backend perform what it performs, given what they are given', () => {
    const { calls, find } = recorder();
    find('deck1.grid.move').apply(-2);
    find('deck2.pitch').apply(0.25);
    find('deck1.jog.touch').apply(1);
    expect(calls).toEqual([
      'perform(deck1.grid.move,-2,true)',
      'perform(deck2.pitch,0.25,false)',
      'perform(deck1.jog.touch,1,false)',
    ]);
    expect(find('shift').apply(1)).toBeUndefined();
  });

  it('brake, spin back while held, repeat, fader start', () => {
    const { calls, find } = recorder();
    find('deck1.brake').apply(1);
    find('deck1.brake').apply(0);
    find('deck2.spinback').apply(1);
    find('deck2.spinback').apply(0);
    find('deck1.repeat').apply(1);
    find('deck2.faderStart').apply(1);
    expect(calls).toEqual([
      'perform(deck1.brake,1,false)',
      'perform(deck1.brake,0,false)',
      'perform(deck2.spinback,1,false)',
      'perform(deck2.spinback,0,false)',
      'perform(deck1.repeat,1,false)',
      'perform(deck2.faderStart,1,false)',
    ]);
  });

  it('reverse, switched or held', () => {
    const { calls, find } = recorder();
    find('deck1.reverse').apply(1);
    find('deck2.reverse.held').apply(1);
    find('deck2.reverse.held').apply(0);
    expect(calls).toEqual([
      'perform(deck1.reverse,1,false)',
      'perform(deck2.reverse.held,1,false)',
      'perform(deck2.reverse.held,0,false)',
    ]);
  });

  it('jump and loop by the size of the deck, which a step halves or doubles', () => {
    const { calls, find } = recorder();
    find('deck2.jump.back').apply(1);
    find('deck1.jump.forward').apply(1);
    find('deck1.loop.size').apply(1);
    find('deck1.size.halve').apply(1);
    find('deck2.size.double').apply(1);
    find('deck2.size').apply(-2);
    expect(find('deck1.size').kind).toBe('encoder');
    expect(calls).toEqual([
      'perform(deck2.jump.back,1,false)',
      'perform(deck1.jump.forward,1,false)',
      'perform(deck1.loop.size,1,false)',
      'perform(deck1.size.halve,1,false)',
      'perform(deck2.size.double,1,false)',
      'perform(deck2.size,-2,true)',
    ]);
  });

  it('roll, slice and cut while held, eight rolls, eight slices, four cuts', () => {
    const { calls, find, actions } = recorder();
    find('deck2.slice3').apply(1);
    find('deck2.slice3').apply(0);
    find('deck1.cut1').apply(1);
    find('deck1.roll8').apply(1);
    measuredControl(actions, 'deck2.roll.0.5')!.apply(0);
    expect(find('deck1.slice9')).toBeUndefined();
    expect(find('deck1.cut5')).toBeUndefined();
    expect(find('deck1.roll9')).toBeUndefined();
    expect(find('deck1.cut1').label).toBe('Fader cut on 1/16 while held');
    expect(find('deck1.slice8').lit).toBeUndefined();
    expect(calls).toEqual([
      'perform(deck2.slice3,1,false)',
      'perform(deck2.slice3,0,false)',
      'perform(deck1.cut1,1,false)',
      'perform(deck1.roll8,1,false)',
      'perform(deck2.roll.0.5,0,false)',
    ]);
  });

  it('move the range of a tempo fader, or of them all, either way', () => {
    const { calls, find } = recorder();
    find('deck2.tempoRange').apply(1);
    find('deck1.tempoRange.previous').apply(1);
    find('general.tempoRange').apply(1);
    find('general.tempoRange.previous').apply(1);
    expect(calls).toEqual([
      'perform(deck2.tempoRange,1,false)',
      'perform(deck1.tempoRange.previous,1,false)',
      'perform(general.tempoRange,1,false)',
      'perform(general.tempoRange.previous,1,false)',
    ]);
  });

  it('give each switch a key putting it on and one putting it off, whatever it is', () => {
    const { calls, controls, find } = recorder();
    find('deck1.sync.on').apply(1);
    find('deck2.keylock.off').apply(1);
    find('general.quantize.on').apply(1);
    find('mixer.crossfader.reverse.off').apply(1);
    find('deck1.vinyl.on').apply(0);
    expect(calls).toEqual([
      'perform(deck1.sync.on,1,false)',
      'perform(deck2.keylock.off,1,false)',
      'perform(general.quantize.on,1,false)',
      'perform(mixer.crossfader.reverse.off,1,false)',
      'perform(deck1.vinyl.on,0,false)',
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
    };
    press();
    slots.loaded = true;
    press();
    slots.playing = true;
    press();
    expect(calls).toEqual([
      'loadSample(1,2)',
      'perform(deck2.sample3,1,false)',
      'perform(deck2.sample3.stop,1,false)',
    ]);
  });

  it('read a jump or a loop of so many beats from its id', () => {
    const { calls, actions } = recorder();
    measuredControl(actions, 'deck2.jump.-8')!.apply(1);
    measuredControl(actions, 'deck1.loop.0.5')!.apply(1);
    expect(calls).toEqual(['perform(deck2.jump.-8,1,false)', 'perform(deck1.loop.0.5,1,false)']);
    expect(measuredControl(actions, 'deck1.loop')).toBeUndefined();
    expect(measuredControl(actions, 'deck1.loop.-1')).toBeUndefined();
    expect(measuredControl(actions, 'deck9.jump.4')).toBeUndefined();
  });

  it('play a sample as hard as its pad is hit, the sampler level left alone', () => {
    const { calls, actions } = recorder();
    const pad = measuredControl(actions, 'deck2.sample3.velocity')!;
    pad.apply(0.25);
    pad.apply(0);
    expect(calls).toEqual([
      'perform(deck2.sample3.velocity,0.25,false)',
      'perform(deck2.sample3.velocity,0,false)',
    ]);
  });
});

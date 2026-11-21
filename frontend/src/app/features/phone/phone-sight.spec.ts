import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { PhoneSight, Setting, View, settingOf, shows } from './phone-sight';

describe('settingOf', () => {
  it('reads the setting a deck’s knob, fader, switch or key moves', () => {
    const read = (id: string) => {
      const setting = settingOf(id);
      return setting && `${setting.deck}:${Object.values(setting).slice(1).join(':')}`;
    };
    const settings = {
      'deck1.eq.low': '0:eq:low',
      'deck2.eq.high.kill': '1:eq:high',
      'deck1.eq.mid.kill.toggle': '0:eq:mid',
      'deck1.trim': '0:trim',
      'deck1.filter.toggle': '0:filter',
      'deck4.volume': '3:volume',
      'deck1.pitch': '0:tempo',
      'deck1.tempo.down': '0:tempo',
      'deck1.key.+2': '0:key',
      'deck1.key.up': '0:key',
      'deck1.keylock.on': '0:keyLock',
      'deck1.headphones': '0:cue',
      'deck1.headphones.solo': '0:cue',
      'deck2.mute.off': '1:mute',
      'deck1.vocals': '0:stem:vocals',
      'deck1.drums.level': '0:stem:drums',
      'deck1.sampler.gain': '0:samplerGain',
      'deck1.fx.chain': '0:fxChain',
      'deck1.fx': '0:fx:0:on',
      'deck1.fx2.held': '0:fx:1:on',
      'deck1.fx3.mix': '0:fx:2:mix',
      'deck1.fx.param2': '0:fx:0:param2',
      'deck1.fx2.period': '0:fx:1:beats',
      'deck1.fx.pingPong': '0:fx:0:kind',
      'deck1.fx.next': '0:fx:0:kind',
      'deck1.fx.super': '0:fx::amount',
      'mixer.crossfader': 'null:crossfader',
      'mixer.cueMix': 'null:cueMix',
      'mixer.mic.on': 'null:mic',
      'sampler.gain': 'null:samplers',
    };
    expect(Object.fromEntries(Object.keys(settings).map((id) => [id, read(id)]))).toEqual(settings);
  });

  it('reads none in what plays, browses or lays the screen out', () => {
    const others = [
      'deck1.play',
      'deck1.jog',
      'deck1.position',
      'deck1.hotcue3',
      'deck1.loop.4',
      'deck1.sync',
      'deck1.fx.focus',
      'deck1.fx.knob1',
      'deck1.sample2',
      'library.browse',
      'general.library',
      'mixer.talkover',
      'mode.deck.b',
    ];
    expect(others.map(settingOf)).toEqual(others.map(() => null));
  });
});

describe('shows', () => {
  const on = (view: View, ids: string[]) =>
    ids.filter((id) => shows(view, settingOf(id) as Setting));

  it('a deck’s readouts show its tempo and its key, its mute where there is room', () => {
    const ids = ['deck1.pitch', 'deck1.key.up', 'deck1.mute', 'deck1.volume', 'deck2.pitch'];
    expect(on({ kind: 'deck', deck: 0, compact: false }, ids)).toEqual(ids.slice(0, 3));
    expect(on({ kind: 'deck', deck: 0, compact: true }, ids)).toEqual(ids.slice(0, 2));
  });

  it('a channel of the table shows its EQ, filter, fader and headphones, not its trim', () => {
    const ids = ['deck1.eq.low.kill', 'deck1.filter', 'deck1.volume', 'deck1.headphones'];
    const view: View = { kind: 'channel', deck: 0 };
    expect(on(view, [...ids, 'deck1.trim', 'deck2.volume', 'deck1.vocals'])).toEqual(ids);
  });

  it('a deck’s panel shows its pitch, key lock and mute, and its channel off the table', () => {
    const ids = ['deck2.tempo', 'deck2.keylock', 'deck2.mute', 'deck2.eq.mid', 'deck2.volume'];
    expect(on({ kind: 'panel', deck: 1, mixer: true }, ids)).toEqual(ids);
    expect(on({ kind: 'panel', deck: 1, mixer: false }, ids)).toEqual(ids.slice(0, 3));
  });

  it('what floats over the table shows its deck’s pitch, stems, effect slot or sampler', () => {
    expect(on({ kind: 'pitch', deck: 0 }, ['deck1.pitch', 'deck1.keylock', 'deck1.key'])).toEqual([
      'deck1.pitch',
      'deck1.keylock',
    ]);
    expect(
      on({ kind: 'stems', deck: 0 }, ['deck1.vocals', 'deck1.bass.level', 'deck2.bass']),
    ).toEqual(['deck1.vocals', 'deck1.bass.level']);
    const fx = [
      'deck1.fx2.mix',
      'deck1.fx2.echo',
      'deck1.fx3.on',
      'deck1.fx.chain',
      'deck1.fx3.mix',
    ];
    const pads = (mode: 'fx' | 'sampler' | 'cues'): View => ({
      kind: 'pads',
      deck: 0,
      mode,
      fxSlot: 1,
    });
    expect(on(pads('fx'), [...fx, 'deck1.sampler.gain'])).toEqual(fx.slice(0, 4));
    expect(on(pads('sampler'), [...fx, 'deck1.sampler.gain', 'sampler.gain'])).toEqual([
      'deck1.sampler.gain',
      'sampler.gain',
    ]);
    expect(on(pads('cues'), [...fx, 'deck1.sampler.gain'])).toEqual([]);
  });

  it('the crossfader and the mixer’s sheet show what the mixer sets', () => {
    const ids = ['mixer.crossfader', 'mixer.headphones', 'mixer.main', 'mixer.mic', 'deck1.volume'];
    expect(on({ kind: 'crossfader' }, ids)).toEqual(['mixer.crossfader']);
    expect(on({ kind: 'mixer' }, ids)).toEqual(ids.slice(1, 4));
  });
});

describe('PhoneSight', () => {
  it('shows a setting while a part of the screen showing it is there', () => {
    TestBed.configureTestingModule({ providers: [PhoneSight] });
    const sight = TestBed.inject(PhoneSight);
    const volume = settingOf('deck1.volume')!;
    expect(sight.showing(volume)).toBe(false);
    const gone = sight.show(signal({ kind: 'deck', deck: 0, compact: true }));
    sight.show(signal({ kind: 'channel', deck: 1 }));
    expect(sight.showing(volume)).toBe(false);
    const channel = signal<View | null>({ kind: 'channel', deck: 0 });
    const left = sight.show(channel);
    expect(sight.showing(volume)).toBe(true);
    channel.set(null);
    expect(sight.showing(volume)).toBe(false);
    channel.set({ kind: 'channel', deck: 0 });
    left();
    gone();
    expect(sight.showing(volume)).toBe(false);
  });
});

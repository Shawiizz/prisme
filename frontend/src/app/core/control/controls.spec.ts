import { buildControls, Control, measuredControl } from './controls';

const controls = buildControls();
const find = (id: string): Control | undefined => controls.find((c) => c.id === id);

describe('controls', () => {
  it('have unique ids', () => {
    expect(new Set(controls.map((c) => c.id)).size).toBe(controls.length);
  });

  it('give both decks the same controls', () => {
    const of = (deck: number) =>
      controls.filter((c) => c.group === `Deck ${deck}`).map((c) => c.id.slice(6));
    expect(of(1)).toEqual(of(2));
  });

  it('name sixteen hot cues and sixteen sounds, eight rolls, eight slices, four cuts', () => {
    expect(find('deck1.hotcue16')).toBeDefined();
    expect(find('deck2.sample16.smart')).toBeDefined();
    expect(find('deck1.hotcue17')).toBeUndefined();
    expect(find('deck1.sample17')).toBeUndefined();
    expect(find('deck1.roll8')).toBeDefined();
    expect(find('deck1.roll9')).toBeUndefined();
    expect(find('deck1.slice9')).toBeUndefined();
    expect(find('deck1.cut5')).toBeUndefined();
    expect(find('deck1.cut1')!.label).toBe('Fader cut on 1/16 while held');
  });

  it('say how each is moved: a key, a fader, an encoder, or a light nothing moves', () => {
    const kinds = Object.fromEntries(controls.map((c) => [c.id, c.kind]));
    expect(kinds['deck1.play']).toBe('button');
    expect(kinds['deck2.eq.low']).toBe('fader');
    expect(kinds['deck1.fx.knob1']).toBe('fader');
    expect(kinds['deck1.fx.beats']).toBe('encoder');
    expect(kinds['deck1.size']).toBe('encoder');
    expect(kinds['library.preview.seek']).toBe('encoder');
    expect(kinds['autodj.toggle']).toBe('button');
    expect(kinds['deck1.phase.ahead']).toBe('light');
    expect(kinds['main.clip']).toBe('light');
  });

  it('give the controls of a deck their deck, the loads theirs', () => {
    expect(find('deck3.play')!.deck).toBe(2);
    expect(find('library.load4')!.deck).toBe(3);
    expect(find('library.double2')!.deck).toBe(1);
    expect(find('mixer.crossfader')!.deck).toBeUndefined();
  });

  it('give each effect slot its controls, the first under the ids of a single effect', () => {
    for (const id of ['deck1.fx', 'deck2.fx3.on', 'deck1.fx2.reverb', 'deck2.fx2.mix'])
      expect(find(id)).toBeDefined();
    expect(find('deck1.fx.part')).toBeDefined();
    expect(find('deck1.fx2.part')).toBeUndefined();
    expect(find('deck1.fx.held')!.label).toBe('Effect 1 on while held');
  });

  it('give each switch a key putting it on and one putting it off, whatever it is', () => {
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
    expect(find('deck1.keylock.on')!.label).toBe('Key lock: on');
  });

  it('read a jump, a loop, a roll or a key shift of so many from its id', () => {
    expect(measuredControl('deck2.jump.-8')).toMatchObject({ deck: 1, kind: 'button' });
    expect(measuredControl('deck1.loop.0.5')!.label).toBe('Loop 0.5 beats');
    expect(measuredControl('deck2.roll.0.5')).toBeDefined();
    expect(measuredControl('deck1.key.+2')).toBeDefined();
    expect(measuredControl('deck1.key.1.5')).toBeUndefined();
    expect(measuredControl('deck1.loop')).toBeUndefined();
    expect(measuredControl('deck1.loop.-1')).toBeUndefined();
    expect(measuredControl('deck1.jump.0')).toBeUndefined();
    expect(measuredControl('deck9.jump.4')).toBeUndefined();
  });

  it('play a sample as hard as its pad is hit', () => {
    expect(measuredControl('deck2.sample3.velocity')).toMatchObject({ deck: 1, kind: 'button' });
    expect(measuredControl('deck2.sample17.velocity')).toBeUndefined();
  });
});

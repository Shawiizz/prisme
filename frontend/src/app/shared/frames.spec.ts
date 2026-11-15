import { Showing } from './frames';

describe('Showing', () => {
  const track = { title: 'a' };

  it('is new the first time, then only when a value changes', () => {
    const showing = new Showing();
    expect(showing.start().with(track).with(1.5).isNew()).toBe(true);
    expect(showing.start().with(track).with(1.5).isNew()).toBe(false);
    expect(showing.start().with(track).with(1.6).isNew()).toBe(true);
    expect(
      showing
        .start()
        .with({ ...track })
        .with(1.6)
        .isNew(),
    ).toBe(true);
  });

  it('is new with fewer or more values', () => {
    const showing = new Showing();
    showing.start().with(1).with(undefined).isNew();
    expect(showing.start().with(1).isNew()).toBe(true);
    expect(showing.start().with(1).with(undefined).isNew()).toBe(true);
  });

  it('is new after being forgotten', () => {
    const showing = new Showing();
    showing.start().with(1).isNew();
    showing.forget();
    expect(showing.start().with(1).isNew()).toBe(true);
  });
});

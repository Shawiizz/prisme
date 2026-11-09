import { Pickup } from './pickup';

describe('Pickup', () => {
  it('waits until a fader away from the setting reaches it', () => {
    const pickup = new Pickup();
    expect(pickup.takes(0.2, 0.5)).toBe(false);
    expect(pickup.takes(0.4, 0.5)).toBe(false);
    expect(pickup.takes(0.6, 0.5)).toBe(true);
    expect(pickup.takes(0.7, 0.6)).toBe(true);
  });

  it('takes a setting at once from a fader on it', () => {
    expect(new Pickup().takes(0.51, 0.5)).toBe(true);
  });

  it('picks a setting changed elsewhere up again', () => {
    const pickup = new Pickup();
    expect(pickup.takes(0.5, 0.5)).toBe(true);
    expect(pickup.takes(0.6, 0.5)).toBe(true);
    expect(pickup.takes(0.62, 0.9)).toBe(false);
    expect(pickup.takes(0.95, 0.9)).toBe(true);
  });
});

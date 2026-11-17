import { isNewer } from './update.service';

describe('isNewer', () => {
  it('compares each number in turn, not the text', () => {
    expect(isNewer('0.3.10', '0.3.9')).toBe(true);
    expect(isNewer('0.4.0', '0.3.12')).toBe(true);
    expect(isNewer('1.0.0', '0.9.9')).toBe(true);
  });

  it('is false for the same version or an older one', () => {
    expect(isNewer('0.3.6', '0.3.6')).toBe(false);
    expect(isNewer('0.3.5', '0.3.6')).toBe(false);
  });
});

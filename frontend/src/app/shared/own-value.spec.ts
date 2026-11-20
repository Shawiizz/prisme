import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { SETTLE_MS, ownValue } from './own-value';

describe('ownValue', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function made(held = signal(false)) {
    const value = signal(0.5);
    const own = TestBed.runInInjectionContext(() => ownValue(value, held));
    return { value, own, held };
  }

  it('follows the backend while nothing moves it', () => {
    const { value, own } = made();
    value.set(0.7);
    expect(own.current()).toBe(0.7);
  });

  it('keeps its own value through older echoes of quick steps, then the backend again', () => {
    const { value, own } = made();
    own.current.set(0.52);
    own.moved();
    own.current.set(0.54);
    own.moved();
    value.set(0.52);
    expect(own.current()).toBe(0.54);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(own.current()).toBe(0.52);
    value.set(0.54);
    expect(own.current()).toBe(0.54);
  });

  it('keeps the pointer value while held, and a moment after it is let go', () => {
    const { value, own, held } = made();
    held.set(true);
    own.current.set(0.9);
    own.moved();
    vi.advanceTimersByTime(SETTLE_MS * 2);
    value.set(0.6);
    expect(own.current(), 'held').toBe(0.9);
    held.set(false);
    own.moved();
    value.set(0.8);
    expect(own.current(), 'just let go').toBe(0.9);
    value.set(0.9);
    vi.advanceTimersByTime(SETTLE_MS);
    expect(own.current()).toBe(0.9);
  });
});

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { PageErrors } from './page-errors';

describe('PageErrors', () => {
  let told: string[];

  beforeEach(() => {
    told = [];
    mockIPC((command, args) => {
      if (command === 'page_error') told.push((args as { message: string }).message);
    });
    vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
  });

  it('tells the backend each error once', async () => {
    const errors = new PageErrors();
    const error = new Error('deck undefined');
    errors.handleError(error);
    errors.handleError(error);
    errors.handleError('a rejection');
    await Promise.resolve();
    expect(told).toEqual([error.stack, 'a rejection']);
  });

  it('tells twenty errors a minute at most, and one told again ten minutes on', async () => {
    vi.useFakeTimers();
    const errors = new PageErrors();
    for (let i = 0; i < 30; i++) errors.handleError(`error ${i}`);
    await vi.advanceTimersByTimeAsync(60_000);
    errors.handleError('error 0');
    errors.handleError('error 25');
    await vi.advanceTimersByTimeAsync(9 * 60_000);
    errors.handleError('error 0');
    await Promise.resolve();
    expect(told).toEqual([
      ...Array.from({ length: 20 }, (_, i) => `error ${i}`),
      'error 25',
      'error 0',
    ]);
    vi.useRealTimers();
  });
});

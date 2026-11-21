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

  it('stops telling after twenty different errors', async () => {
    const errors = new PageErrors();
    for (let i = 0; i < 30; i++) errors.handleError(`error ${i}`);
    await Promise.resolve();
    expect(told).toHaveLength(20);
  });
});

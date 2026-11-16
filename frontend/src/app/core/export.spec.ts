import { fileName, setName } from './services/export.service';

describe('setName', () => {
  it('names a set by when it started, sortable', () => {
    expect(setName(new Date(2026, 10, 4, 9, 5).getTime())).toBe('2026-11-04 09:05');
  });
});

describe('fileName', () => {
  it('keeps what a file name can hold', () => {
    expect(fileName('Festivals / Main stage')).toBe('Festivals - Main stage');
    expect(fileName('Tech: "house"?')).toBe('Tech- -house--');
    expect(fileName('Warm-up (2026)')).toBe('Warm-up (2026)');
  });
});

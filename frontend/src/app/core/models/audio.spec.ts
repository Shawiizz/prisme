import { TimecodeFormat, timecodeMakers } from './audio';

const format = (id: string, maker: string, record: string, side: string | null) =>
  ({ id, maker, record, side, leadInS: 0 }) as TimecodeFormat;

describe('timecodeMakers', () => {
  it('groups the formats by maker, then record, its sides in order', () => {
    const a = format('a', 'Serato', 'Control vinyl CV02', 'A');
    const b = format('b', 'Serato', 'Control vinyl CV02', 'B');
    const cd = format('cd', 'Serato', 'Control CD', null);
    const other = format('o', 'Other', 'Control CD', null);
    expect(timecodeMakers([a, b, cd, other])).toEqual([
      {
        name: 'Serato',
        records: [
          { name: 'Control vinyl CV02', sides: [a, b] },
          { name: 'Control CD', sides: [cd] },
        ],
      },
      { name: 'Other', records: [{ name: 'Control CD', sides: [other] }] },
    ]);
  });
});

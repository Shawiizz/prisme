import { describeControl, shortName } from './controllers';

describe('controllers', () => {
  it('describe a MIDI control by its message and a HID one by its place in a report', () => {
    expect(describeControl({ kind: 'controlChange', channel: 0, number: 7 })).toBe('CC 7 · ch 1');
    expect(describeControl({ kind: 'hid', report: 1, byte: 14, bit: 1 })).toBe(
      'Report 1 · byte 14 · bit 1',
    );
    expect(describeControl({ kind: 'hid', report: 2, byte: 9, bits: 12 })).toBe(
      'Report 2 · byte 9 · 12 bits',
    );
    expect(describeControl({ kind: 'hid', byte: 3 })).toBe('Report 0 · byte 3 · bit 0');
  });

  it('name a HID device without its ids where it is told', () => {
    expect(shortName('Traktor Kontrol S4 MK2 (HID 17cc:1310)')).toBe('Traktor Kontrol S4 MK2');
    expect(shortName('2- Traktor Kontrol S4 MK2 (HID 17cc:1310)')).toBe(
      '2- Traktor Kontrol S4 MK2',
    );
    expect(shortName('DDJ-400')).toBe('DDJ-400');
  });
});

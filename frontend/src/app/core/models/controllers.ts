/** How the controllers' controls and mappings are named in the interface. */

import { t } from '../i18n/i18n';
import type { ControllerMapping, Encoding, PhysicalControl } from './backend';

/** How an encoder sends its steps. */
export type StepEncoding = Exclude<Encoding, 'absolute'>;

export function describeControl(c: PhysicalControl): string {
  if (c.kind === 'hid') {
    const bits = c.bits ?? 1;
    const where = `${t('Report {n}', { n: c.report ?? 0 })} · ${t('byte {n}', { n: c.byte })}`;
    return bits === 1
      ? `${where} · ${t('bit {n}', { n: c.bit ?? 0 })}`
      : `${where} · ${t('{n} bits', { n: bits })}`;
  }
  const what =
    c.kind === 'note'
      ? t('Note {n}', { n: c.number })
      : c.kind === 'controlChange'
        ? `CC ${c.number}`
        : t('Pitch bend');
  return `${what} · ${t('ch {n}', { n: c.channel + 1 })}`;
}

/** A controller's name, but a HID device's ids (`Traktor Kontrol S4 MK2 (HID 17cc:1310)`). */
export function shortName(device: string): string {
  return device.replace(/ \(HID [0-9a-f]{4}:[0-9a-f]{4}\)/i, '');
}

/** "Shift + CC 0 · ch 1 · 14 bits" */
export function describeMapping(m: ControllerMapping): string {
  const fine = m.fine !== undefined ? ` · ${t('14 bits')}` : '';
  const shift = m.shift === true ? t('Shift') : m.shift ? `${t('Shift')} ${m.shift}` : null;
  return `${shift ? shift + ' + ' : ''}${describeControl(m.control)}${fine}`;
}

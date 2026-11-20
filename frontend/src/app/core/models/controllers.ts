/** Shapes exchanged with the Rust side about controllers, MIDI and HID (see `controllers.rs`). */

import { Target } from '../control/controls';
import { t } from '../i18n/i18n';

/** A controller plugged in while Prisme runs: opened if Prisme knows its model (or, held by
 * another program, waited for), else offered. */
export interface PluggedIn {
  kind: 'opened' | 'offered' | 'waiting';
  /** The MIDI port name. */
  device: string;
  model: string;
}

export interface Controller {
  /** The MIDI port name, or the name Prisme gives a HID device. */
  name: string;
  enabled: boolean;
  plugged: boolean;
  /** The model Prisme knows it as: its controls come mapped. */
  profile: string | null;
  /** The file of its profile. */
  profileFile: string | null;
  /** The profiles the DJ may choose for it: its model's, then the variants of it (a firmware
   * or an edition of its own). */
  offered: { file: string; name: string }[];
  /** Its profile was tried on the controller itself, not only read from another program's. */
  verified: boolean;
  /** Where its profile comes from. */
  profileSource: ProfileSource | null;
  /** Prisme's model that the DJ's own profile takes the place of on it. */
  replaces: string | null;
  /** The file its profile is exported as, by default. */
  exportFile: string;
  /** Listed with the controllers: a MIDI port, a HID device of a model Prisme knows, or one in
   * use (most other HID devices are no controllers). */
  listed: boolean;
  /** What it drives can be written as a profile: it is in use and plugged in. */
  canExport: boolean;
}

/** Shipped with Prisme, published since and fetched, or the DJ's own. */
export type ProfileSource = 'shipped' | 'updated' | 'own';

/** The controller models Prisme knows, and where the DJ's own profiles go. */
export interface ControllerProfiles {
  profiles: {
    file: string;
    name: string;
    source: ProfileSource;
    verified: boolean;
    /** Prisme's model whose profile of the same file name this one, the DJ's own, replaces. */
    replaces: string | null;
  }[];
  /** The models known (their variants aside): all of them, those updated since, the DJ's own. */
  counts: { all: number; updated: number; own: number };
  /** The DJ's own profiles that take the place of Prisme's of the same file name (`name`: the
   * model replaced). */
  replacing: { file: string; name: string }[];
  /** The DJ's files that could not be read: `file: why`. */
  unreadable: string[];
  folder: string | null;
}

/** A physical control: what one of its MIDI messages addresses, or a field of a HID report. */
export type PhysicalControl = MidiControl | HidControl;

export interface MidiControl {
  kind: 'note' | 'controlChange' | 'pitchBend';
  /** 0 to 15. */
  channel: number;
  number: number;
}

/** A field of a HID report: its first byte, its lowest bit there and how many bits it has. */
export interface HidControl {
  kind: 'hid';
  /** Left out: 0, a device that does not number its reports. */
  report?: number;
  byte: number;
  bit?: number;
  /** Left out: 1, a key. */
  bits?: number;
  bigEndian?: boolean;
}

/** How an encoder sends its steps: two's complement, around 64 (with no zero: 64 one step
 * left), or with a sign bit. */
export type StepEncoding =
  'relative' | 'relativeOffset' | 'relativeOffsetNoZero' | 'relativeSigned';

/** A control of a controller driving an action of the app (`target`). */
export interface ControllerMapping {
  device: string;
  control: PhysicalControl;
  target: string;
  /** How its values are read; left out: positions (`absolute`). */
  encoding?: 'absolute' | StepEncoding;
  /** The controller sending the low half of a 14-bit fader. */
  fine?: number;
  /** In the second layer: while the controller's shift is held (`true`), or a named
   * shift (`"left"`). */
  shift?: boolean | string;
}

/** An action the controllers in use drive, and what drives it on each. */
export interface Mapped {
  target: Target;
  devices: { device: string; mappings: ControllerMapping[] }[];
}

/** An action the app performs that a controller drove: a key's value. */
/** The controllers as the backend shows them. */
export interface ControllersState {
  /** Those plugged in, and those in use that are not. */
  controllers: Controller[];
  /** A controller is open: controls can be learnt. */
  canLearn: boolean;
  /** The controller models Prisme knows. */
  profiles: ControllerProfiles;
  /** What drives what, by action and by controller. */
  mapped: Mapped[];
  /** Where the MIDI clock goes. */
  clockOutput: string | null;
  /** The action the next control moved will drive. */
  learning: string | null;
}

/** A controller in use opened again. */
export interface Opened {
  device: string;
  model: string;
}

export interface UiAction {
  target: string;
  value: number;
}

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

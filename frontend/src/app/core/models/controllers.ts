/** Shapes exchanged with the Rust side about MIDI controllers (see `controllers.rs`). */

import { t } from '../i18n/i18n';

export interface Controller {
  /** The MIDI port name. */
  name: string;
  enabled: boolean;
  plugged: boolean;
  /** The model Prisme knows it as: its controls come mapped. */
  profile: string | null;
  /** Its profile was tried on the controller itself, not only read from another program's. */
  verified: boolean;
  /** Where its profile comes from. */
  profileSource: ProfileSource | null;
}

/** Shipped with Prisme, published since and fetched, or the DJ's own. */
export type ProfileSource = 'shipped' | 'updated' | 'own';

/** The controller models Prisme knows, and where the DJ's own profiles go. */
export interface ControllerProfiles {
  profiles: { file: string; name: string; source: ProfileSource; verified: boolean }[];
  /** The DJ's files that could not be read: `file: why`. */
  unreadable: string[];
  folder: string | null;
}

export interface MidiControl {
  kind: 'note' | 'controlChange' | 'pitchBend';
  /** 0 to 15. */
  channel: number;
  number: number;
}

/** How an encoder sends its steps: two's complement, around 64, or with a sign bit. */
export type StepEncoding = 'relative' | 'relativeOffset' | 'relativeSigned';

/** A control of a controller driving a control of the app (`target`, see `buildControls`). */
export interface ControllerMapping {
  device: string;
  control: MidiControl;
  target: string;
  /** How its values are read; left out: positions (`absolute`). */
  encoding?: 'absolute' | StepEncoding;
  /** The controller sending the low half of a 14-bit fader. */
  fine?: number;
  /** In the second layer: while the controller's shift is held. */
  shift?: boolean;
}

/** A mapped control moved. */
export interface ControllerInput {
  target: string;
  value: number;
  /** `value` is steps of an encoder: a fader's control moves by them. */
  steps: boolean;
}

export function describeControl(c: MidiControl): string {
  const what =
    c.kind === 'note'
      ? t('Note {n}', { n: c.number })
      : c.kind === 'controlChange'
        ? `CC ${c.number}`
        : t('Pitch bend');
  return `${what} · ${t('ch {n}', { n: c.channel + 1 })}`;
}

/** "Shift + CC 0 · ch 1 · 14 bits" */
export function describeMapping(m: ControllerMapping): string {
  const fine = m.fine !== undefined ? ` · ${t('14 bits')}` : '';
  return `${m.shift ? t('Shift') + ' + ' : ''}${describeControl(m.control)}${fine}`;
}

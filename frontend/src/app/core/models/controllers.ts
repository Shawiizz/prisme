/** Shapes exchanged with the Rust side about MIDI controllers (see `controllers.rs`). */

import { t } from '../i18n/i18n';

export interface Controller {
  /** The MIDI port name. */
  name: string;
  enabled: boolean;
  plugged: boolean;
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
  encoding: 'absolute' | StepEncoding;
  /** The controller sending the low half of a 14-bit fader. */
  fine?: number;
  /** In the second layer: while the controller's shift is held. */
  shift?: boolean;
}

/** A mapped control moved. */
export interface ControllerInput {
  target: string;
  value: number;
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

/** Shapes exchanged with the Rust side about MIDI controllers (see `controllers.rs`). */

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

/** A control of a controller driving a control of the app (`target`, see `buildControls`). */
export interface ControllerMapping {
  device: string;
  control: MidiControl;
  target: string;
  encoding: 'absolute' | 'relative';
}

/** A mapped control moved. */
export interface ControllerInput {
  target: string;
  value: number;
}

export function describeControl(c: MidiControl): string {
  const what =
    c.kind === 'note'
      ? `Note ${c.number}`
      : c.kind === 'controlChange'
        ? `CC ${c.number}`
        : 'Pitch bend';
  return `${what} · ch ${c.channel + 1}`;
}

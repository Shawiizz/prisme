import { Label } from '../i18n/i18n';

/** The physical control an action suits. A light shows a state only: nothing drives it (the
 * beat, a clip, the end of a track). */
export type ControlKind = 'button' | 'fader' | 'encoder' | 'light';

/** An action a key or a controller can do, as the backend lists it (`list_targets`). */
export interface Target {
  /** Stable: the controller mappings refer to it. */
  id: string;
  /** Button: 1 pressed, 0 released. Fader: 0 to 1. Encoder: steps, negative to the left. */
  kind: ControlKind;
  /** The deck it plays, for the controls of a deck and the loads. */
  deck: number | null;
  group: Label;
  label: Label;
  /** Of a deck in use, or of none. */
  inUse: boolean;
}

/** An action named in the UI's language: for the mappings, the keys, the command palette. */
export interface Control {
  id: string;
  group: string;
  label: string;
  kind: ControlKind;
  inUse: boolean;
}

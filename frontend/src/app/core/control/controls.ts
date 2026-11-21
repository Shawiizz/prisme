import type { ControlKind } from '../models/backend';

/** An action named in the UI's language: for the mappings, the keys, the command palette. */
export interface Control {
  id: string;
  group: string;
  label: string;
  kind: ControlKind;
  inUse: boolean;
}

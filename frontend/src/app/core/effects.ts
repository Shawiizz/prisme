import { Text, t } from './i18n/i18n';
import type { EffectKind } from './models/backend';

/** The name of each beat effect, in full. */
export const EFFECT_NAMES: Record<EffectKind, Text> = {
  echo: () => t('Echo'),
  pingPong: () => t('Ping-pong echo'),
  reverb: () => t('Reverb'),
  flanger: () => t('Flanger'),
  phaser: () => t('Phaser'),
  filter: () => t('Filter'),
  gate: () => t('Gate'),
  bitcrusher: () => t('Bitcrusher'),
};

/** What the second knob of each effect does: its label on the knob, and its name in full. */
export const PARAM2: Record<EffectKind, { label: string; name: Text }> = {
  echo: { label: 'TONE', name: () => t('Tone of the repeats') },
  pingPong: { label: 'TONE', name: () => t('Tone of the repeats') },
  reverb: { label: 'DAMP', name: () => t('Damping of the room') },
  flanger: { label: 'WIDTH', name: () => t('Width of the sweep') },
  phaser: { label: 'DEPTH', name: () => t('Depth of the sweep') },
  filter: { label: 'RESO', name: () => t('Resonance') },
  gate: { label: 'LENGTH', name: () => t('How long the gate stays open') },
  bitcrusher: { label: 'HOLD', name: () => t('Downsampling') },
};

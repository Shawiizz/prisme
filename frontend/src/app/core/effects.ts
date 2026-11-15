import { Text, t } from './i18n/i18n';
import { EffectKind } from './models/engine';

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

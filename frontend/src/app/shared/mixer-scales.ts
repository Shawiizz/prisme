import { BANDS, Band, ChannelStrip } from '../core/models/engine';
import { decimal } from './time';

/** Knob positions (0 = full left, 0.5 = centre, 1 = full right) and the gains they set. */

/** Below this the EQ knob removes its band, like an isolator. */
const EQ_OFF_BELOW = 0.02;
const EQ_CUT_DB = 40;
const EQ_BOOST_DB = 6;
const TRIM_RANGE_DB = 12;
/** A live input's knob: 0 dB three quarters of the way, +12 dB at the end. */
const LIVE_UNITY = 0.75;
const LIVE_BOOST_DB = 12;

const toGain = (db: number) => 10 ** (db / 20);
const toDb = (gain: number) => 20 * Math.log10(gain);

/** EQ: full left removes the band, centre is flat, full right +6 dB. */
export function eqGain(knob: number): number {
  if (knob <= EQ_OFF_BELOW) return 0;
  const db = knob < 0.5 ? ((knob - 0.5) / 0.5) * EQ_CUT_DB : ((knob - 0.5) / 0.5) * EQ_BOOST_DB;
  return toGain(db);
}

export function eqKnob(gain: number): number {
  if (gain <= 0) return 0;
  const db = toDb(gain);
  const knob = db < 0 ? 0.5 + (db / EQ_CUT_DB) * 0.5 : 0.5 + (db / EQ_BOOST_DB) * 0.5;
  return Math.min(Math.max(knob, EQ_OFF_BELOW + 1e-6), 1);
}

/** Where the EQ knob of `band` stands on a channel. */
export function bandKnob(strip: ChannelStrip, band: Band): number {
  return eqKnob(strip.eq[BANDS.indexOf(band)]);
}

/** Filter: full left closes the low-pass (-1), centre is open (0), full right closes the
 * high-pass (1). */
export function filterAmount(knob: number): number {
  return knob * 2 - 1;
}

export function filterKnob(amount: number): number {
  return (amount + 1) / 2;
}

/** Trim: -12 dB to +12 dB, flat at the centre. */
export function trimGain(knob: number): number {
  return toGain((knob - 0.5) * 2 * TRIM_RANGE_DB);
}

export function trimKnob(gain: number): number {
  return Math.min(Math.max(0.5 + toDb(gain) / (2 * TRIM_RANGE_DB), 0), 1);
}

/** A live input's level: silent full left, 0 dB at three quarters, +12 dB full right. */
export function liveGain(knob: number): number {
  if (knob <= EQ_OFF_BELOW) return 0;
  return toGain(((knob - LIVE_UNITY) / (1 - LIVE_UNITY)) * LIVE_BOOST_DB);
}

export function liveKnob(gain: number): number {
  if (gain <= 0) return 0;
  const knob = LIVE_UNITY + (toDb(gain) / LIVE_BOOST_DB) * (1 - LIVE_UNITY);
  return Math.min(Math.max(knob, EQ_OFF_BELOW + 1e-6), 1);
}

/** Balance: full left plays the master on the left only (-1), full right on the right (1). */
export function balanceAmount(knob: number): number {
  return knob * 2 - 1;
}

export function balanceKnob(amount: number): number {
  return (amount + 1) / 2;
}

/** What a knob reads while it is touched. */
/** The headphone volume knob goes up to +3.5 dB. */
export const MAX_HEADPHONE_GAIN = 1.5;

export const readouts = {
  /** "+3.0 dB", "−∞ dB" when the band is removed. */
  eq: (knob: number) => decibels(eqGain(knob)),
  trim: (knob: number) => decibels(trimGain(knob)),
  live: (knob: number) => decibels(liveGain(knob)),
  /** The side the master leans to, and how far the other is turned down. */
  balance: (knob: number) => {
    const amount = Math.round(balanceAmount(knob) * 100);
    if (amount === 0) return 'L = R';
    return `${amount < 0 ? 'L' : 'R'} ${Math.abs(amount)} %`;
  },
  /** Left of the centre a low-pass, right of it a high-pass. */
  filter: (knob: number) => {
    const amount = Math.round(Math.abs(knob - 0.5) * 200);
    if (amount < 2) return '0';
    return `${knob < 0.5 ? 'LPF' : 'HPF'} ${amount} %`;
  },
  percent: (knob: number) => `${Math.round(knob * 100)} %`,
  gain: (gain: number) => decibels(gain),
};

function decibels(gain: number): string {
  if (gain <= 0) return '−∞ dB';
  const db = Math.round(toDb(gain) * 10) / 10 || 0;
  return `${db > 0 ? '+' : db < 0 ? '−' : ''}${decimal(Math.abs(db), 1)} dB`;
}

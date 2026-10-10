/** Knob positions (0 = full left, 0.5 = centre, 1 = full right) and the gains they set. */

/** Below this the EQ knob removes its band, like an isolator. */
const EQ_OFF_BELOW = 0.02;
const EQ_CUT_DB = 40;
const EQ_BOOST_DB = 6;
const TRIM_RANGE_DB = 12;

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

/** Trim: -12 dB to +12 dB, flat at the centre. */
export function trimGain(knob: number): number {
  return toGain((knob - 0.5) * 2 * TRIM_RANGE_DB);
}

export function trimKnob(gain: number): number {
  return Math.min(Math.max(0.5 + toDb(gain) / (2 * TRIM_RANGE_DB), 0), 1);
}

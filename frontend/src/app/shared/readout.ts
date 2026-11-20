import { decimal } from './time';

/** What a knob reads under it, as the backend reads its setting (`scales::Readout`). */
export type Readout =
  { db: number | null } | { percent: number } | { balance: number } | { filter: number };

/** A knob of the screen as the backend shows it (`knobs::Knob`). */
export interface Knob {
  /** Where it stands, from 0 to 1. */
  at: number;
  reads: Readout;
  /** Where a double click puts it back. */
  home: number;
}

/** "+3.0 dB", "−∞ dB", "42 %", "L = R", "R 20 %", "LPF 35 %". */
export function readout(reads: Readout): string {
  if ('db' in reads) {
    const db = reads.db;
    if (db === null) return '−∞ dB';
    return `${db > 0 ? '+' : db < 0 ? '−' : ''}${decimal(Math.abs(db), 1)} dB`;
  }
  if ('percent' in reads) return `${reads.percent} %`;
  if ('balance' in reads) {
    const side = reads.balance;
    return side === 0 ? 'L = R' : `${side < 0 ? 'L' : 'R'} ${Math.abs(side)} %`;
  }
  const closed = reads.filter;
  return closed === 0 ? '0' : `${closed < 0 ? 'LPF' : 'HPF'} ${Math.abs(closed)} %`;
}

/** `0.42` → `42 %` */
export function percent(share: number): string {
  return `${Math.round(share * 100)} %`;
}

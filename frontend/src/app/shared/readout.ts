import { decimal } from './time';
import type { Readout } from '../core/models/backend';

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

/** Bytes in megabytes, rounded: what is left to download. */
export function megabytes(bytes: number | null): number | null {
  return bytes === null ? null : Math.round(bytes / 1e6);
}

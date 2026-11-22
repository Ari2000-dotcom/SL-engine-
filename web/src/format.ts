import type { Broker, LegStatus } from './types';

const inr = new Intl.NumberFormat('en-IN', { maximumFractionDigits: 0 });
const price = new Intl.NumberFormat('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

export const fmtInt = (n: number): string => inr.format(n);
export const fmtPrice = (n: number | null | undefined): string => (n == null ? '-' : price.format(n));
export const fmtRupee = (n: number | null | undefined): string => (n == null ? '-' : `₹${inr.format(Math.round(n))}`);
export const fmtPct = (n: number | null | undefined): string => (n == null ? '-' : `${n.toFixed(1)}%`);

export function fmtDuration(totalSecs: number): string {
  const s = Math.max(0, Math.floor(totalSecs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return `${h}h ${String(m).padStart(2, '0')}m`;
  if (m > 0) return `${m}m ${String(sec).padStart(2, '0')}s`;
  return `${sec}s`;
}

export function fmtClock(iso: string | null | undefined): string {
  if (!iso) return '-';
  return new Date(iso).toLocaleTimeString('en-IN', { hour12: false, timeZone: 'Asia/Kolkata' });
}

export function fmtAgo(iso: string | null | undefined, now: number): string {
  if (!iso) return 'never';
  return `${fmtDuration((now - new Date(iso).getTime()) / 1000)} ago`;
}

export const BROKER_NAME: Record<Broker, string> = { KOTAK: 'Kotak Neo', ZERODHA: 'Zerodha Kite', ANGEL: 'Angel One' };

export const STATUS_LABEL: Record<LegStatus, string> = {
  GRACE: 'Just opened',
  PROTECTED: 'Protected',
  NO_SL: 'No stop-loss',
  SL_REJECTED: 'SL rejected',
  SL_CANCELLED: 'SL cancelled',
  PARTIAL_SL: 'Partial SL',
  OVER_COVERED: 'SL too large',
  WRONG_SIDE: 'Wrong side',
  TOO_FAR: 'SL too far',
  UNVERIFIED: 'Unverified',
  STALE_DATA: 'Stale data',
};

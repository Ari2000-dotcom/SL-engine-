import { fmtRupee } from '../format';
import { SideBuy, SideSell, StateAmo, StateCancelled, StateDone, StateOpen, StatePart, StatePending, StateRejected, StateTrigger } from '../icons';
import type { Leg } from '../types';

export const OPEN_STATES = new Set(['PENDING_NEW', 'OPEN', 'TRIGGER_PENDING', 'MODIFY_PENDING', 'CANCEL_PENDING', 'PARTIAL', 'AMO']);
export const STATE_LABEL: Record<string, string> = {
  PENDING_NEW: 'Pending',
  OPEN: 'Open',
  TRIGGER_PENDING: 'Trigger',
  MODIFY_PENDING: 'Modifying',
  CANCEL_PENDING: 'Cancelling',
  PARTIAL: 'Part filled',
  COMPLETE: 'Complete',
  CANCELLED: 'Cancelled',
  REJECTED: 'Rejected',
  AMO: 'After market',
  UNKNOWN: 'Unknown',
};
export const KIND_LABEL: Record<string, string> = {
  MARKET: 'Market',
  LIMIT: 'Limit',
  SL_LIMIT: 'SL limit',
  SL_MARKET: 'SL market',
  GTT_SL: 'GTT stop',
};

/** Open profit or loss from quantity and price. Contract multipliers are not applied, so treat it as an estimate. */
export const legPnl = (l: Leg): number | null => (l.ltp == null || !(l.avg_price > 0) ? null : (l.ltp - l.avg_price) * l.net_qty);
export const fmtSigned = (n: number): string => `${n >= 0 ? '+' : '-'}${fmtRupee(Math.abs(n))}`;

export const initials = (name: string): string =>
  name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0])
    .join('')
    .toUpperCase();

export const Side = ({ side }: { side: 'BUY' | 'SELL' }) => (
  <span className={`side side--${side.toLowerCase()}`}>
    {side === 'BUY' ? <SideBuy size={16} aria-hidden="true" /> : <SideSell size={16} aria-hidden="true" />}
    {side === 'BUY' ? 'Buy' : 'Sell'}
  </span>
);

export const STATE_ICON: Record<string, typeof StateOpen> = {
  PENDING_NEW: StatePending,
  OPEN: StateOpen,
  TRIGGER_PENDING: StateTrigger,
  MODIFY_PENDING: StatePending,
  CANCEL_PENDING: StatePending,
  PARTIAL: StatePart,
  COMPLETE: StateDone,
  CANCELLED: StateCancelled,
  REJECTED: StateRejected,
  AMO: StateAmo,
};

/** Order state as an icon plus a word, so it never relies on colour alone. */
export const StateChip = ({ state }: { state: string }) => {
  const Icon = STATE_ICON[state] ?? StateOpen;
  return (
    <span className={`state state--${state.toLowerCase()}`}>
      <Icon size={16} aria-hidden="true" />
      {STATE_LABEL[state] ?? state}
    </span>
  );
};


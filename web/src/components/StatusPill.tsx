import { CheckmarkFilled, CircleDash, ErrorFilled, WarningAltFilled } from '../icons';
import { STATUS_LABEL } from '../format';
import type { LegStatus, Severity } from '../types';

const SEVERITY: Record<LegStatus, Severity> = {
  GRACE: 'GREY',
  PROTECTED: 'GREEN',
  NO_SL: 'RED',
  SL_REJECTED: 'RED',
  SL_CANCELLED: 'RED',
  WRONG_SIDE: 'RED',
  PARTIAL_SL: 'AMBER',
  OVER_COVERED: 'AMBER',
  TOO_FAR: 'AMBER',
  UNVERIFIED: 'AMBER',
  STALE_DATA: 'GREY',
};

export const severityOf = (s: LegStatus): Severity => SEVERITY[s];
export const coverageTone: Record<Severity, string> = { RED: 'r', AMBER: 'a', GREEN: 'g', GREY: 'n' };

const ICON = {
  RED: ErrorFilled,
  AMBER: WarningAltFilled,
  GREEN: CheckmarkFilled,
  GREY: CircleDash,
} as const;

/** State is shown with an icon and a label as well as colour, so it reads without colour vision. */
export function StatusPill({ status }: { status: LegStatus }) {
  const sev = SEVERITY[status];
  const Icon = ICON[sev];
  return (
    <span className={`pill pill--${sev.toLowerCase()}`}>
      <Icon size={16} aria-hidden="true" />
      {STATUS_LABEL[status]}
    </span>
  );
}

const CONN = {
  OK: { cls: 'green', label: 'Connected', Icon: CheckmarkFilled },
  FAILED: { cls: 'red', label: 'Login or read failed', Icon: ErrorFilled },
  CONNECTING: { cls: 'grey', label: 'Connecting', Icon: CircleDash },
} as const;

export function ConnectionPill({ status }: { status: 'OK' | 'FAILED' | 'CONNECTING' }) {
  const { cls, label, Icon } = CONN[status];
  return (
    <span className={`pill pill--${cls}`}>
      <Icon size={16} aria-hidden="true" />
      {label}
    </span>
  );
}

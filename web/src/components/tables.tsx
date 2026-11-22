import { CheckmarkFilled } from '../icons';
import { useEffect, useMemo, useRef, useState } from 'react';
import { BROKER_NAME, fmtAgo, fmtClock, fmtDuration, fmtInt, fmtPct, fmtPrice, fmtRupee } from '../format';
import type { Account, ClientOverview, Incident, Leg } from '../types';
import { ConnectionPill, StatusPill, severityOf } from './StatusPill';

export function Empty({ title, children, ok }: { title: string; children: React.ReactNode; ok?: boolean }) {
  return (
    <div className="empty">
      {ok && <CheckmarkFilled size={24} aria-hidden="true" />}
      <h3>{title}</h3>
      <p>{children}</p>
    </div>
  );
}

const sevClass = (s: ReturnType<typeof severityOf> | string) => (s === 'RED' ? 'sev-red' : s === 'AMBER' ? 'sev-amber' : '');


/** A numeric cell that flashes green or red for a moment when its value moves, like a quote screen. */
export function FlashCell({ value, className, children }: { value: number | null | undefined; className?: string; children: React.ReactNode }) {
  const prev = useRef(value);
  const [dir, setDir] = useState<'up' | 'down' | null>(null);
  useEffect(() => {
    const before = prev.current;
    prev.current = value;
    if (value == null || before == null || value === before) return;
    setDir(value > before ? 'up' : 'down');
    const id = window.setTimeout(() => setDir(null), 900);
    return () => window.clearTimeout(id);
  }, [value]);
  return <td className={`${className ?? ''} ${dir ? `flash-${dir}` : ''}`}>{children}</td>;
}

/** Keyboard for a table of clickable rows: arrows move between rows, Enter or Space opens one. */
export function rowKeys(open: () => void) {
  return {
    tabIndex: 0,
    onKeyDown: (e: React.KeyboardEvent<HTMLTableRowElement>) => {
      const row = e.currentTarget;
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const next = (e.key === 'ArrowDown' ? row.nextElementSibling : row.previousElementSibling) as HTMLElement | null;
        next?.focus();
        next?.scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        open();
      }
    },
  };
}

export type OpenLeg = (legKey: string, leg?: Leg) => void;

export interface Lookups {
  clientName: (id: string) => string;
  brokerOf: (accountId: string) => string;
}

export function makeLookups(overview: ClientOverview[], accounts: Account[]): Lookups {
  const names = new Map(overview.map((c) => [c.client_id, c.name]));
  const brokers = new Map(accounts.map((a) => [a.account_id, BROKER_NAME[a.broker]]));
  return {
    clientName: (id) => names.get(id) ?? id,
    brokerOf: (id) => brokers.get(id) ?? id,
  };
}

// ---------------- Sorting ----------------

type Dir = 'asc' | 'desc';

function useSort<T>(rows: T[], by: Record<string, (r: T) => string | number>) {
  const [sort, setSort] = useState<{ key: string; dir: Dir } | null>(null);
  const sorted = useMemo(() => {
    if (!sort) return rows;
    const get = by[sort.key];
    const k = sort.dir === 'asc' ? 1 : -1;
    return [...rows].sort((a, b) => {
      const x = get(a);
      const y = get(b);
      return (typeof x === 'number' && typeof y === 'number' ? x - y : String(x).localeCompare(String(y))) * k;
    });
    // `by` is a stable literal per table, so it is left out on purpose.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows, sort]);
  const toggle = (key: string) =>
    setSort((s) => (s?.key !== key ? { key, dir: 'asc' } : s.dir === 'asc' ? { key, dir: 'desc' } : null));
  const th = (key: string, label: string, num = false) => (
    <th
      className={`sortable ${num ? 'num' : ''}`}
      onClick={() => toggle(key)}
      aria-sort={sort?.key === key ? (sort.dir === 'asc' ? 'ascending' : 'descending') : 'none'}
    >
      {label}
      <span className="arrow" aria-hidden="true">
        {sort?.key === key ? (sort.dir === 'asc' ? '↑' : '↓') : ''}
      </span>
    </th>
  );
  return { sorted, th };
}

const RANK = { RED: 0, AMBER: 1, GREY: 2, GREEN: 3 } as const;

// ---------------- Incidents ----------------

const RESOLUTION = { SL_PLACED: 'Stop-loss placed', POSITION_CLOSED: 'Position closed' } as const;

export function IncidentsTable(props: {
  incidents: Incident[];
  now: number;
  look: Lookups;
  showClient: boolean;
  onAck: (id: number) => void;
  ackBusy: number | null;
  onOpen: OpenLeg;
  selected: string | null;
}) {
  const { incidents, now, look, showClient } = props;
  const { sorted, th } = useSort(incidents, {
    status: (i) => RANK[severityOf(i.current_status)],
    client: (i) => look.clientName(i.client_id),
    broker: (i) => look.brokerOf(i.account_id),
    symbol: (i) => i.symbol,
    opened: (i) => new Date(i.opened_at).getTime(),
    dur: (i) => (i.resolved_at ? i.unprotected_secs : (now - new Date(i.opened_at).getTime()) / 1000),
  });
  return (
    <div className="tscroll">
      <table className="dt" aria-label="Incidents">
        <thead>
          <tr>
            {th('status', 'Status')}
            {showClient && th('client', 'Client')}
            {th('broker', 'Broker')}
            {th('symbol', 'Symbol')}
            <th>What is wrong</th>
            {th('opened', 'Opened')}
            {th('dur', 'Unprotected', true)}
            <th>Action</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((i) => {
            const open = !i.resolved_at;
            const secs = open ? (now - new Date(i.opened_at).getTime()) / 1000 : i.unprotected_secs;
            return (
              <tr
                key={i.id}
                className={`click ${open ? sevClass(severityOf(i.current_status)) : 'is-done'} ${props.selected === i.leg_key ? 'is-selected' : ''}`}
                onClick={() => props.onOpen(i.leg_key)}
                {...rowKeys(() => props.onOpen(i.leg_key))}
              >
                <td>
                  <StatusPill status={open ? i.current_status : 'PROTECTED'} />
                </td>
                {showClient && <td>{look.clientName(i.client_id)}</td>}
                <td>{look.brokerOf(i.account_id)}</td>
                <td className="sym">{i.symbol}</td>
                <td className="wrap">
                  {open ? (i.detail ?? '-') : i.resolution ? RESOLUTION[i.resolution] : '-'}
                  {open && i.worst_status !== i.current_status && (
                    <span className="sub">Worst so far: {i.worst_status.replace(/_/g, ' ').toLowerCase()}</span>
                  )}
                </td>
                <td className="num" style={{ textAlign: 'left' }}>
                  {fmtClock(i.opened_at)}
                </td>
                <td className="num">
                  <b style={open ? { color: severityOf(i.current_status) === 'RED' ? 'var(--red)' : 'var(--amber)' } : undefined}>{fmtDuration(secs)}</b>
                </td>
                <td onClick={(e) => e.stopPropagation()}>
                  {!open ? (
                    <span className="muted">Closed {fmtClock(i.resolved_at)}</span>
                  ) : i.acknowledged_at ? (
                    <span className="muted">Seen by {i.acknowledged_by}</span>
                  ) : (
                    <button type="button" className="btn" disabled={props.ackBusy === i.id} onClick={() => props.onAck(i.id)}>
                      Acknowledge
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ---------------- Legs ----------------

export function LegsTable({
  legs,
  look,
  showClient,
  onOpen,
  selected,
}: {
  legs: Leg[];
  look: Lookups;
  showClient: boolean;
  onOpen: OpenLeg;
  selected: string | null;
}) {
  const { sorted, th } = useSort(legs, {
    status: (l) => RANK[l.severity],
    client: (l) => look.clientName(l.client_id),
    broker: (l) => look.brokerOf(l.account_id),
    symbol: (l) => l.symbol,
    qty: (l) => Math.abs(l.net_qty),
    avg: (l) => l.avg_price,
    ltp: (l) => l.ltp ?? 0,
    sl: (l) => l.sl_trigger ?? 0,
    slq: (l) => l.sl_qty,
    loss: (l) => (l.risk_mode === 'PCT' ? (l.dist_pct ?? 0) : (l.risk_rupee ?? 0)),
  });
  return (
    <div className="tscroll">
      <table className="dt" aria-label="Open legs">
        <thead>
          <tr>
            {th('status', 'Status')}
            {showClient && th('client', 'Client')}
            {th('broker', 'Broker')}
            {th('symbol', 'Symbol')}
            {th('qty', 'Quantity', true)}
            {th('avg', 'Avg price', true)}
            {th('ltp', 'LTP', true)}
            {th('sl', 'SL trigger', true)}
            {th('slq', 'SL qty', true)}
            {th('loss', 'Loss if hit', true)}
            <th>Note</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((l) => (
            <tr
              key={l.leg_key}
              className={`click ${sevClass(l.severity)} ${selected === l.leg_key ? 'is-selected' : ''}`}
              onClick={() => onOpen(l.leg_key, l)}
              {...rowKeys(() => onOpen(l.leg_key, l))}
            >
              <td>
                <StatusPill status={l.status} />
              </td>
              {showClient && <td>{look.clientName(l.client_id)}</td>}
              <td>{look.brokerOf(l.account_id)}</td>
              <td>
                <span className="sym">{l.symbol}</span>
                <span className="sub">
                  {l.exchange} · {l.product.toLowerCase()}
                </span>
              </td>
              <td className="num">
                {fmtInt(l.net_qty)}
                <span className="sub">{l.net_qty < 0 ? 'short' : 'long'}</span>
              </td>
              <td className="num">{fmtPrice(l.avg_price)}</td>
              <FlashCell className="num" value={l.ltp}>
                {fmtPrice(l.ltp)}
              </FlashCell>
              <td className="num">{fmtPrice(l.sl_trigger)}</td>
              <td className="num">{l.sl_qty ? fmtInt(l.sl_qty) : '-'}</td>
              <td className="num">
                {l.risk_mode === 'PCT' ? fmtPct(l.dist_pct) : fmtRupee(l.risk_rupee)}
                <span className="sub">limit {l.risk_mode === 'PCT' ? `${l.limit_value}%` : fmtRupee(l.limit_value)}</span>
              </td>
              <td className="wrap" title={l.detail ?? undefined}>{l.detail ?? ''}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ---------------- Clients ----------------

export function ClientsTable({ rows, onPick }: { rows: ClientOverview[]; onPick?: (clientId: string) => void }) {
  const { sorted, th } = useSort(rows, {
    client: (c) => c.name,
    red: (c) => c.red,
    amber: (c) => c.amber,
    green: (c) => c.green,
    down: (c) => c.accounts_failed,
  });
  return (
    <div className="tscroll">
      <table className="dt" aria-label="Clients">
        <thead>
          <tr>
            {th('client', 'Client')}
            <th>Coverage</th>
            {th('red', 'No SL', true)}
            {th('amber', 'Partial', true)}
            {th('green', 'Safe', true)}
            {th('down', 'Feeds down', true)}
          </tr>
        </thead>
        <tbody>
          {sorted.map((c) => {
            const total = Math.max(1, c.red + c.amber + c.green + c.grey);
            const w = (n: number) => `${(n / total) * 100}%`;
            return (
              <tr key={c.client_id} className={`${onPick ? 'click' : ''} ${c.red > 0 ? 'sev-red' : c.amber > 0 ? 'sev-amber' : ''}`} onClick={onPick ? () => onPick(c.client_id) : undefined}>
                <td>
                  <span className="sym">{c.name}</span>
                  <span className="sub">{c.group}</span>
                </td>
                <td>
                  <div className="bar" role="img" aria-label={`${c.red} red, ${c.amber} amber, ${c.green} protected, ${c.grey} waiting`}>
                    <i className="r" style={{ width: w(c.red) }} />
                    <i className="a" style={{ width: w(c.amber) }} />
                    <i className="g" style={{ width: w(c.green) }} />
                    <i className="n" style={{ width: w(c.grey) }} />
                  </div>
                </td>
                <td className="num" style={c.red ? { color: 'var(--red)', fontWeight: 500 } : undefined}>
                  {c.red}
                </td>
                <td className="num" style={c.amber ? { color: 'var(--amber)', fontWeight: 500 } : undefined}>
                  {c.amber}
                </td>
                <td className="num">{c.green}</td>
                <td className="num">
                  {c.accounts_failed} / {c.accounts_total}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ---------------- Accounts ----------------

export function AccountsTable({ accounts, look, now }: { accounts: Account[]; look: Lookups; now: number }) {
  const { sorted, th } = useSort(accounts, {
    status: (a) => (a.status === 'FAILED' ? 0 : a.status === 'CONNECTING' ? 1 : 2),
    client: (a) => look.clientName(a.client_id),
    broker: (a) => BROKER_NAME[a.broker],
    lat: (a) => a.latency_ms ?? 0,
    polls: (a) => a.polls,
  });
  return (
    <div className="tscroll">
      <table className="dt" aria-label="Broker accounts">
        <thead>
          <tr>
            {th('status', 'Connection')}
            {th('client', 'Client')}
            {th('broker', 'Broker')}
            <th>Account</th>
            <th>Last good read</th>
            {th('lat', 'Latency', true)}
            {th('polls', 'Polls today', true)}
            <th>Last error</th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((a) => (
            <tr key={a.account_id} className={a.status === 'FAILED' ? 'sev-red' : ''}>
              <td>
                <ConnectionPill status={a.status} />
              </td>
              <td>{look.clientName(a.client_id)}</td>
              <td>{BROKER_NAME[a.broker]}</td>
              <td className="sym">{a.account_id}</td>
              <td>{fmtAgo(a.last_ok, now)}</td>
              <td className="num">{a.latency_ms == null ? '-' : `${a.latency_ms} ms`}</td>
              <td className="num">{fmtInt(a.polls)}</td>
              <td className="wrap" title={a.last_error ?? undefined}>{a.last_error ?? ''}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

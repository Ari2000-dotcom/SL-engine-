import { Close } from '../icons';
import { useEffect } from 'react';
import { STATUS_LABEL, fmtAgo, fmtClock, fmtDuration, fmtInt, fmtPct, fmtPrice, fmtRupee } from '../format';
import type { Incident, Leg } from '../types';
import { StatusPill, coverageTone, severityOf } from './StatusPill';
import type { Lookups } from './tables';

const TONE = { RED: 'var(--red)', AMBER: 'var(--amber)', GREEN: 'var(--green)', GREY: 'var(--grey)' } as const;

interface Props {
  /** Missing for a closed incident whose leg is no longer open. */
  leg: Leg | undefined;
  incident: Incident | undefined;
  look: Lookups;
  now: number;
  onClose: () => void;
  onAck: (id: number) => void;
  ackBusy: number | null;
}

/** Everything the desk needs to ring a client about one leg, in one place. */
export function LegDrawer({ leg, incident, look, now, onClose, onAck, ackBusy }: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const abs = leg ? Math.abs(leg.net_qty) : 0;
  const cover = leg && abs ? Math.min(100, (leg.sl_qty / abs) * 100) : 0;
  const open = incident && !incident.resolved_at;
  const symbol = leg?.symbol ?? incident?.symbol ?? '';
  const status = leg?.status ?? incident?.current_status ?? 'STALE_DATA';
  const detail = leg?.detail ?? incident?.detail ?? null;
  const clientId = leg?.client_id ?? incident?.client_id ?? '';
  const accountId = leg?.account_id ?? incident?.account_id ?? '';

  return (
    <>
      <div className="scrim" onClick={onClose} />
      <aside className="drawer" role="dialog" aria-label={`Leg ${symbol}`}>
        <div className="drawer__head">
          <div>
            <StatusPill status={status} />
            <h2>{symbol}</h2>
            <p>
              {look.clientName(clientId)} · {look.brokerOf(accountId)} · {accountId}
            </p>
          </div>
          <button type="button" className="iconbtn" onClick={onClose} aria-label="Close panel">
            <Close size={20} />
          </button>
        </div>

        <div className="drawer__body">
          {detail && (
            <div>
              <div className="h3">What is wrong</div>
              <div className="note">{detail}</div>
            </div>
          )}

          {leg && (
            <>
          <dl className="facts" style={{ margin: 0 }}>
            <div>
              <dt>Position</dt>
              <dd>
                {fmtInt(leg.net_qty)}
                <small>
                  {leg.net_qty < 0 ? 'short' : 'long'} · {leg.exchange} · {leg.product.toLowerCase()}
                </small>
              </dd>
            </div>
            <div>
              <dt>Average price</dt>
              <dd>{fmtPrice(leg.avg_price)}</dd>
            </div>
            <div>
              <dt>Last price</dt>
              <dd>{fmtPrice(leg.ltp)}</dd>
            </div>
            <div>
              <dt>SL trigger</dt>
              <dd>{fmtPrice(leg.sl_trigger)}</dd>
            </div>
            <div>
              <dt>Loss if hit</dt>
              <dd>
                {leg.risk_mode === 'PCT' ? fmtPct(leg.dist_pct) : fmtRupee(leg.risk_rupee)}
                <small>limit {leg.risk_mode === 'PCT' ? `${leg.limit_value}%` : fmtRupee(leg.limit_value)}</small>
              </dd>
            </div>
            <div>
              <dt>Stop-loss cover</dt>
              <dd>
                {fmtInt(leg.sl_qty)} / {fmtInt(abs)}
                <small>{Math.round(cover)}% of the position</small>
              </dd>
            </div>
          </dl>

          <div>
            <div className="bar" style={{ height: '0.625rem' }} role="img" aria-label={`Stop-loss covers ${Math.round(cover)} percent`}>
              <i className={coverageTone[severityOf(status)]} style={{ width: `${cover}%` }} />
            </div>
          </div>

          {leg.sl_order_ids.length > 0 && (
            <div>
              <div className="h3">Stop-loss orders</div>
              <div className="note num" style={{ textAlign: 'left' }}>
                {leg.sl_order_ids.join(', ')}
              </div>
            </div>
          )}
            </>
          )}

          {incident && (
            <div>
              <div className="h3">Incident timeline</div>
              <ol className="timeline">
                {incident.history.map(([t, s], idx) => (
                  <li key={idx} style={{ ['--tl' as string]: TONE[severityOf(s)] }}>
                    <span>{STATUS_LABEL[s]}</span>
                    <span className="muted num">{fmtClock(t)}</span>
                  </li>
                ))}
              </ol>
              <p className="muted" style={{ fontSize: 'var(--t-xs)' }}>
                {open
                  ? `Unprotected for ${fmtDuration((now - new Date(incident.opened_at).getTime()) / 1000)}, opened ${fmtAgo(incident.opened_at, now)}.`
                  : `Closed after ${fmtDuration(incident.unprotected_secs)} unprotected.`}
              </p>
            </div>
          )}

          {open && !incident.acknowledged_at && (
            <button type="button" className="btn btn--primary" style={{ height: '2.5rem' }} disabled={ackBusy === incident.id} onClick={() => onAck(incident.id)}>
              Acknowledge incident
            </button>
          )}
          {open && incident.acknowledged_at && <p className="muted">Acknowledged by {incident.acknowledged_by}.</p>}
        </div>
      </aside>
    </>
  );
}

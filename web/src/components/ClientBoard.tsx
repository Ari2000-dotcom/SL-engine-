import { useState } from 'react';
import { Search } from '../icons';
import { fetchBook } from '../api';
import { BROKER_NAME, STATUS_LABEL, fmtClock, fmtInt, fmtPrice } from '../format';
import { usePolled } from '../hooks';
import { KIND_LABEL, OPEN_STATES, STATE_LABEL, Side, StateChip, fmtSigned, initials, legPnl } from './bookParts';
import { ConnectionPill, StatusPill } from './StatusPill';
import { FlashCell, rowKeys, type OpenLeg } from './tables';
import type { Account, ClientOverview, Leg, Order } from '../types';

export interface BoardAccount {
  account: Account;
  red: number;
  amber: number;
}

interface Props {
  client: ClientOverview;
  items: BoardAccount[];
  token: string;
  onAuthLost: () => void;
  onOpen: OpenLeg;
  selected: string | null;
  onPick: (accountId: string) => void;
  index: number;
}

/** Every word typed must appear somewhere in the row: a name, a side, a state, a quantity, a price or an order id. */
const matches = (q: string, parts: (string | number | null | undefined)[]): boolean => {
  const words = q.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = parts.filter((p) => p != null).join(' ').toLowerCase();
  return words.every((w) => hay.includes(w));
};
const legText = (l: Leg) => [l.symbol, l.exchange, l.product, l.net_qty, Math.abs(l.net_qty), l.net_qty < 0 ? 'short sell' : 'long buy', l.avg_price, l.ltp, l.sl_trigger, STATUS_LABEL[l.status]];
const orderText = (o: Order) => [o.symbol, o.exchange, o.product, o.side, o.side === 'BUY' ? 'buy' : 'sell', o.qty, o.filled_qty, o.price || 'market', o.trigger_price, o.broker_order_id, STATE_LABEL[o.state] ?? o.state, KIND_LABEL[o.kind] ?? o.kind, o.reject_reason];

function PositionsMini({ legs, onOpen, selected }: { legs: Leg[]; onOpen: OpenLeg; selected: string | null }) {
  return (
    <table className="dt dt--tight dt--board" aria-label="Open positions">
      <thead>
        <tr>
          <th>Time</th>
          <th>Instrument</th>
          <th className="num">Qty</th>
          <th className="num">P&amp;L</th>
          <th>Stop-loss</th>
        </tr>
      </thead>
      <tbody>
        {legs.map((l) => {
          const pnl = legPnl(l);
          return (
            <tr
              key={l.leg_key}
              className={`click ${l.severity === 'RED' ? 'sev-red' : l.severity === 'AMBER' ? 'sev-amber' : ''} ${selected === l.leg_key ? 'is-selected' : ''}`}
              onClick={() => onOpen(l.leg_key, l)}
              {...rowKeys(() => onOpen(l.leg_key, l))}
            >
              <td className="time">{l.opened_ts ? fmtClock(l.opened_ts) : '-'}</td>
              <td className="ins">
                <span className="sym">{l.symbol}</span>
                <span className="sub">
                  {l.exchange} · {l.product.toLowerCase()}
                </span>
              </td>
              <td className="num">
                {fmtInt(l.net_qty)}
                <span className="sub">{l.net_qty < 0 ? 'short' : 'long'}</span>
              </td>
              <FlashCell className={`num pnl ${pnl == null ? '' : pnl >= 0 ? 'pnl--up' : 'pnl--down'}`} value={pnl}>
                {pnl == null ? '-' : fmtSigned(pnl)}
                <span className="sub">{fmtPrice(l.avg_price)}</span>
              </FlashCell>
              <td>
                <StatusPill status={l.status} />
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function OrdersMini({ rows }: { rows: Order[] }) {
  return (
    <table className="dt dt--tight dt--board" aria-label="Open orders">
      <thead>
        <tr>
          <th>Time</th>
          <th>Instrument</th>
          <th>Side</th>
          <th className="num">Filled / qty</th>
          <th>Status</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((o) => (
          <tr key={o.broker_order_id} className={o.state === 'REJECTED' ? 'sev-red' : ''}>
            <td className="time">{fmtClock(o.order_ts)}</td>
            <td className="ins">
              <span className="sym">{o.symbol}</span>
              <span className="sub">
                {KIND_LABEL[o.kind] ?? o.kind} · {o.price ? fmtPrice(o.price) : 'market'}
                {o.trigger_price != null && ` · trigger ${fmtPrice(o.trigger_price)}`}
              </span>
            </td>
            <td>
              <Side side={o.side} />
            </td>
            <td className="num">
              {fmtInt(o.filled_qty)} / {fmtInt(o.qty)}
            </td>
            <td>
              <StateChip state={o.state} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** One broker account inside a client board: open positions on the left, open orders on the right. */
function AccountSection({ item, q, token, onAuthLost, onOpen, selected, onPick }: Omit<Props, 'client' | 'items' | 'index'> & { item: BoardAccount; q: string }) {
  const a = item.account;
  const { data, error } = usePolled((signal) => fetchBook(token, a.account_id, signal), [token, a.account_id], 5000, onAuthLost);
  const book = data && data.account_id === a.account_id ? data : null;
  const [ot, setOt] = useState<'open' | 'prev' | 'all'>('open');
  const byTime = (x: string | null, y: string | null) => (y ?? '').localeCompare(x ?? '');
  const allLegs = [...(book?.legs ?? [])].sort((x, y) => byTime(x.opened_ts, y.opened_ts));
  const legs = allLegs.filter((l) => matches(q, legText(l)));
  const allOrders = [...(book?.orders ?? [])].filter((o) => matches(q, orderText(o))).sort((x, y) => byTime(x.order_ts, y.order_ts));
  const open = allOrders.filter((o) => OPEN_STATES.has(o.state));
  const prev = allOrders.filter((o) => !OPEN_STATES.has(o.state));
  const shownOrders = ot === 'open' ? open : ot === 'prev' ? prev : allOrders;
  const bad = legs.filter((l) => l.severity === 'RED' || l.severity === 'AMBER').length;
  const pnl = legs.reduce((s, l) => s + (legPnl(l) ?? 0), 0);
  const ring = item.red ? 'red' : a.status === 'FAILED' ? 'red' : item.amber ? 'amber' : 'ok';

  return (
    <section className="acct">
      <header className="acct__head">
        <span className={`brk brk--${ring}`} aria-hidden="true">
          {BROKER_NAME[a.broker].slice(0, 1)}
        </span>
        <button type="button" className="acct__name" onClick={() => onPick(a.account_id)} title="Open this account on its own">
          <b>{BROKER_NAME[a.broker]}</b>
          <small>{a.account_id}</small>
        </button>
        <ConnectionPill status={a.status} />
        <span className="acct__figs">
          {book && (
            <>
              <span>
                {legs.length} {legs.length === 1 ? 'position' : 'positions'}
              </span>
              {bad > 0 ? <b className="t-red">{bad} without full stop</b> : legs.length > 0 ? <b className="t-green">all protected</b> : null}
              {legs.length > 0 && <b className={`num ${pnl >= 0 ? 't-green' : 't-red'}`}>{fmtSigned(pnl)}</b>}
            </>
          )}
        </span>
      </header>
      <div className="acct__cols">
        <div className="acct__col">
          <div className="acct__label">
            Open positions <i>{book ? legs.length : '–'}</i>
          </div>
          <div className="acct__scroll">
            {!book ? (
              <p className="acct__none">{error ?? 'Loading'}</p>
            ) : legs.length === 0 ? (
              <p className="acct__none">{q.trim() && allLegs.length > 0 ? 'No position matches.' : 'Flat. No open positions.'}</p>
            ) : (
              <PositionsMini legs={legs} onOpen={onOpen} selected={selected} />
            )}
          </div>
        </div>
        <div className="acct__col">
          <div className="acct__label">
            <span>Orders</span>
            <div className="acct__tabs" role="group" aria-label="Which orders">
              {(
                [
                  ['open', 'Open', open.length],
                  ['prev', 'Previous', prev.length],
                  ['all', 'All', allOrders.length],
                ] as const
              ).map(([id, label, n]) => (
                <button key={id} type="button" aria-pressed={ot === id} onClick={() => setOt(id)}>
                  {label} <i>{book ? n : '–'}</i>
                </button>
              ))}
            </div>
          </div>
          <div className="acct__scroll">
            {!book ? (
              <p className="acct__none">{error ?? 'Loading'}</p>
            ) : shownOrders.length === 0 ? (
              <p className="acct__none">{q.trim() ? 'No order matches.' : ot === 'open' ? 'No open orders.' : ot === 'prev' ? 'No earlier orders today.' : 'No orders today.'}</p>
            ) : (
              <OrdersMini rows={shownOrders} />
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

/** One large container per client. Every broker account the client has sits inside it. */
export function ClientBoard({ client, items, token, onAuthLost, onOpen, selected, onPick, index }: Props) {
  const [q, setQ] = useState('');
  const problems = items.reduce((n, i) => n + i.red + i.amber, 0);
  const down = items.filter((i) => i.account.status === 'FAILED').length;
  return (
    <article className="cboard tray--rise" style={{ ['--i' as string]: index }}>
      <header className="cboard__head">
        <span className="avatar" aria-hidden="true">
          {initials(client.name)}
        </span>
        <div className="cboard__who">
          <h2>{client.name}</h2>
          <small>
            {client.group} · {items.length} {items.length === 1 ? 'account' : 'accounts'}
          </small>
        </div>
        <div className="search cboard__search">
          <Search size={16} aria-hidden="true" />
          <input
            aria-label={`Search ${client.name}'s orders and positions`}
            placeholder="Search name or number"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') setQ('');
            }}
          />
        </div>
        {down > 0 && <span className="badge badge--down">{down} down</span>}
        {problems > 0 ? <span className="badge badge--red">{problems} to fix</span> : <span className="badge badge--ok">Protected</span>}
      </header>
      <div className="cboard__body">
        {items.map((it) => (
          <AccountSection key={it.account.account_id} item={it} q={q} token={token} onAuthLost={onAuthLost} onOpen={onOpen} selected={selected} onPick={onPick} />
        ))}
      </div>
    </article>
  );
}

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
                <span className="sym" title={l.symbol}>{l.symbol}</span>
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

function OrdersMini({ rows, label }: { rows: Order[]; label: string }) {
  return (
    <table className="dt dt--tight dt--board" aria-label={label}>
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
              <span className="sym" title={o.symbol}>{o.symbol}</span>
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
              {o.reject_reason && <span className="sub order-reason" title={o.reject_reason}>{o.reject_reason}</span>}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

const ORDER_FILTERS = { open: 'Open', prev: 'Previous', all: 'All' } as const;
type OrderFilter = keyof typeof ORDER_FILTERS;

function AccountSection({ item, q, token, onAuthLost, onOpen, selected, onPick }: Omit<Props, 'client' | 'items' | 'index'> & { item: BoardAccount; q: string }) {
  const a = item.account;
  const { data, error } = usePolled((signal) => fetchBook(token, a.account_id, signal), [token, a.account_id], 5000, onAuthLost);
  const book = data && data.account_id === a.account_id ? data : null;
  const [pickedFilter, setPickedFilter] = useState<OrderFilter | null>(null);
  const byTime = (x: string | null, y: string | null) => (y ?? '').localeCompare(x ?? '');
  const allLegs = [...(book?.legs ?? [])].sort((x, y) => byTime(x.opened_ts, y.opened_ts));
  const legs = allLegs.filter((l) => matches(q, legText(l)));
  const orders = book?.orders ?? [];
  const allOrders = orders.filter((o) => matches(q, orderText(o))).sort((x, y) => byTime(x.order_ts, y.order_ts));
  const open = allOrders.filter((o) => OPEN_STATES.has(o.state));
  const prev = allOrders.filter((o) => !OPEN_STATES.has(o.state));
  const groups = { open, prev, all: allOrders };
  const filter = pickedFilter ?? (open.length === 0 && prev.length > 0 ? 'prev' : 'open');
  const shownOrders = groups[filter];
  const bad = legs.filter((l) => l.severity === 'RED' || l.severity === 'AMBER').length;
  const waiting = legs.filter((l) => l.severity === 'GREY').length;
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
          {a.last_ok && <span title="Last successful broker read">Read {fmtClock(a.last_ok)} IST</span>}
          {book && (
            <>
              <span>
                {legs.length} {legs.length === 1 ? 'position' : 'positions'}
              </span>
              {bad > 0 && <b className="t-red">{bad} need attention</b>}
              {waiting > 0 && <span>{waiting} awaiting verification</span>}
              {a.status === 'OK' && legs.length > 0 && bad === 0 && waiting === 0 && <b className="t-green">all protected</b>}
              {legs.length > 0 && <b className={`num ${pnl >= 0 ? 't-green' : 't-red'}`}>{fmtSigned(pnl)}</b>}
            </>
          )}
        </span>
      </header>
      {a.status !== 'OK' && book && (
        <p className="acct__notice">{a.last_ok ? 'Broker data is stale. Showing the last successful read.' : 'Waiting for the first successful broker read.'}</p>
      )}
      {a.status === 'OK' && book && error && <p className="acct__notice">Could not refresh this account book. Showing the last loaded data.</p>}
      <div className={`acct__cols${book && (legs.length === 0 || orders.length === 0) ? ' acct__cols--stacked' : ''}`}>
        <div className="acct__col">
          <div className="acct__label">
            Open positions <i>{book ? legs.length : '–'}</i>
          </div>
          {book && legs.length > 0 ? (
            <div className="acct__scroll" role="region" aria-label={`Open positions for ${a.account_id}`} tabIndex={0}>
              <PositionsMini legs={legs} onOpen={onOpen} selected={selected} />
            </div>
          ) : !book ? (
            <p className="acct__none">{error ?? 'Loading'}</p>
          ) : (
            <p className="acct__none">{allLegs.length > 0 ? 'No position matches.' : a.status === 'OK' ? 'No open positions.' : 'Open positions could not be verified.'}</p>
          )}
        </div>
        <div className="acct__col">
          <div className="acct__label">
            <span>Orders</span>
            <div className="acct__tabs" role="group" aria-label="Which orders">
              {(['open', 'prev', 'all'] as const).map((id) => (
                <button key={id} type="button" aria-pressed={filter === id} onClick={() => setPickedFilter(id)}>
                  {ORDER_FILTERS[id]} <i>{book ? groups[id].length : '–'}</i>
                </button>
              ))}
            </div>
          </div>
          {!book ? (
            <p className="acct__none">{error ?? 'Loading'}</p>
          ) : shownOrders.length === 0 ? (
            <p className="acct__none">{q.trim() ? 'No order matches.' : filter === 'open' ? 'No open orders.' : filter === 'prev' ? 'No earlier orders today.' : 'No orders today.'}</p>
          ) : (
            <div className="acct__scroll" role="region" aria-label={`${ORDER_FILTERS[filter]} orders for ${a.account_id}`} tabIndex={0}>
              <OrdersMini rows={shownOrders} label={`${ORDER_FILTERS[filter]} orders`} />
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

/** One large container per client. Every broker account the client has sits inside it. */
export function ClientBoard({ client, items, token, onAuthLost, onOpen, selected, onPick, index }: Props) {
  const [q, setQ] = useState('');
  const critical = items.reduce((n, i) => n + i.red, 0);
  const problems = items.reduce((n, i) => n + i.red + i.amber, 0);
  const down = items.filter((i) => i.account.status === 'FAILED').length;
  const connecting = items.filter((i) => i.account.status === 'CONNECTING').length;
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
        {problems > 0 && <span className={`badge ${critical ? 'badge--red' : 'badge--amber'}`}>{problems} need review</span>}
        {client.grey > 0 && <span className="badge">{client.grey} unverified</span>}
        {connecting > 0 && <span className="badge">{connecting} connecting</span>}
        {problems === 0 && client.grey === 0 && down === 0 && connecting === 0 && client.green > 0 && <span className="badge badge--ok">Protected</span>}
      </header>
      <div className="cboard__body">
        {items.map((it) => (
          <AccountSection key={it.account.account_id} item={it} q={q} token={token} onAuthLost={onAuthLost} onOpen={onOpen} selected={selected} onPick={onPick} />
        ))}
      </div>
    </article>
  );
}

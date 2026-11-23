import { useEffect, useId, useMemo, useState, type ReactNode } from 'react';
import { fetchBook } from '../api';
import { ClientBoard } from '../components/ClientBoard';
import { ConnectionPill, StatusPill, coverageTone } from '../components/StatusPill';
import { Empty, FlashCell, IncidentsTable, rowKeys, type Lookups, type OpenLeg } from '../components/tables';
import { OPEN_STATES, KIND_LABEL, Side, StateChip, fmtSigned, initials, legPnl } from '../components/bookParts';
import { BROKER_NAME, fmtAgo, fmtClock, fmtInt, fmtPrice, fmtRupee } from '../format';
import { useDebounced, usePolled } from '../hooks';
import { Search } from '../icons';
import type { Account, Leg, Order, StateView, Trade } from '../types';

interface Props {
  view: StateView;
  now: number;
  look: Lookups;
  token: string;
  onAuthLost: () => void;
  onOpen: OpenLeg;
  selected: string | null;
  onAck: (id: number) => void;
  ackBusy: number | null;
}

function OrdersTable({ rows }: { rows: Order[] }) {
  return (
    <div className="tscroll tscroll--inner" role="region" aria-label="Order table scroll area" tabIndex={0}>
      <table className="dt dt--tight dt--orders" aria-label="Orders">
        <thead>
          <tr>
            <th className="dt__instrument">Instrument</th>
            <th className="dt__time">Updated (IST)</th>
            <th>Side</th>
            <th className="num">Filled / qty</th>
            <th className="num">Price</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((o) => (
            <tr key={o.broker_order_id} className={o.state === 'REJECTED' ? 'sev-red' : ''}>
              <td>
                <span className="sym" title={o.symbol}>{o.symbol}</span>
                <span className="sub">
                  {KIND_LABEL[o.kind] ?? o.kind} · {o.exchange} · {o.product.toLowerCase()}
                </span>
              </td>
              <td className="dt__time">{fmtClock(o.updated_ts)}</td>
              <td>
                <Side side={o.side} />
              </td>
              <td className="num">
                {fmtInt(o.filled_qty)} / {fmtInt(o.qty)}
              </td>
              <td className="num">
                {o.price ? fmtPrice(o.price) : 'Market'}
                {o.trigger_price != null && <span className="sub">trigger {fmtPrice(o.trigger_price)}</span>}
              </td>
              <td>
                <StateChip state={o.state} />
                {o.reject_reason && <span className="sub" title={o.reject_reason}>{o.reject_reason}</span>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TradesTable({ rows }: { rows: Trade[] }) {
  return (
    <div className="tscroll tscroll--inner">
      <table className="dt dt--tight" aria-label="Executed trades">
        <thead>
          <tr>
            <th>Time</th>
            <th>Instrument</th>
            <th>Side</th>
            <th className="num">Qty</th>
            <th className="num">Price</th>
            <th className="num">Value</th>
            <th>Trade</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((t) => (
            <tr key={t.trade_id}>
              <td className="num" style={{ textAlign: 'left' }}>
                {fmtClock(t.fill_ts)}
              </td>
              <td>
                <span className="sym">{t.symbol}</span>
                <span className="sub">
                  {t.exchange} · {t.product.toLowerCase()}
                </span>
              </td>
              <td>
                <Side side={t.side} />
              </td>
              <td className="num">{fmtInt(t.qty)}</td>
              <td className="num">{fmtPrice(t.price)}</td>
              <td className="num">{fmtRupee(t.qty * t.price)}</td>
              <td className="muted">{t.trade_id}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PositionsTable({ legs, onOpen, selected }: { legs: Leg[]; onOpen: OpenLeg; selected: string | null }) {
  return (
    <div className="tscroll tscroll--inner">
      <table className="dt dt--tight" aria-label="Open positions">
        <thead>
          <tr>
            <th>Status</th>
            <th>Instrument</th>
            <th className="num">Qty</th>
            <th className="num">Avg / LTP</th>
            <th className="num">P&amp;L</th>
            <th className="num">Stop-loss</th>
          </tr>
        </thead>
        <tbody>
          {legs.map((l) => {
            const abs = Math.abs(l.net_qty) || 1;
            const cover = Math.min(100, (l.sl_qty / abs) * 100);
            const pnl = legPnl(l);
            return (
              <tr
                key={l.leg_key}
                className={`click ${l.severity === 'RED' ? 'sev-red' : l.severity === 'AMBER' ? 'sev-amber' : ''} ${selected === l.leg_key ? 'is-selected' : ''}`}
                onClick={() => onOpen(l.leg_key, l)}
                {...rowKeys(() => onOpen(l.leg_key, l))}
              >
                <td>
                  <StatusPill status={l.status} />
                </td>
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
                <td className="num">
                  {fmtPrice(l.avg_price)}
                  <span className="sub">{fmtPrice(l.ltp)}</span>
                </td>
                <FlashCell className={`num pnl ${pnl == null ? '' : pnl >= 0 ? 'pnl--up' : 'pnl--down'}`} value={pnl}>
                  {pnl == null ? '-' : fmtSigned(pnl)}
                  <span className="sub">{pnl == null ? '' : `${pnl >= 0 ? '+' : '-'}${Math.abs((pnl / (l.avg_price * abs)) * 100).toFixed(1)}%`}</span>
                </FlashCell>
                <td className="num">
                  {l.sl_trigger == null ? <span className="nostop">No stop</span> : fmtPrice(l.sl_trigger)}
                  <span className="cover">
                    <span className="bar bar--thin">
                      <i className={coverageTone[l.severity]} style={{ width: `${cover}%` }} />
                    </span>
                    <span className="sub" style={{ marginTop: 0 }}>
                      {Math.round(cover)}% cover
                    </span>
                  </span>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function BookPanel({ title, count, actions, children }: { title: string; count?: number; actions?: ReactNode; children: ReactNode }) {
  const id = useId();
  return (
    <section className="core book-panel" aria-labelledby={id}>
      <header className="core__head">
        <h3 id={id}>{title}{count != null && <span className="count">{count}</span>}</h3>
        {actions}
      </header>
      {children}
    </section>
  );
}

type Tab = 'open' | 'fills' | 'all';

interface RailAccount {
  account: Account;
  red: number;
  amber: number;
  /** 0 red, 1 feed down, 2 amber, 3 fine. Lower is shown first. */
  rank: number;
}

export function Book({ view, now, look, token, onAuthLost, onOpen, selected, onAck, ackBusy }: Props) {
  const accounts = view.accounts;
  const [query, setQuery] = useState('');
  const [onlyProblems, setOnlyProblems] = useState(view.accounts.length > 40);
  const dq = useDebounced(query, 150).trim().toLowerCase();

  // Problem counts per account come from the live push, which only carries legs that need attention.
  const rail = useMemo(() => {
    const by = new Map<string, { red: number; amber: number }>();
    view.legs.forEach((l) => {
      const e = by.get(l.account_id) ?? { red: 0, amber: 0 };
      if (l.severity === 'RED') e.red += 1;
      else if (l.severity === 'AMBER') e.amber += 1;
      by.set(l.account_id, e);
    });
    const list: RailAccount[] = accounts.map((a) => {
      const c = by.get(a.account_id) ?? { red: 0, amber: 0 };
      const rank = c.red ? 0 : a.status === 'FAILED' ? 1 : c.amber ? 2 : 3;
      return { account: a, red: c.red, amber: c.amber, rank };
    });
    const groups = view.overview
      .map((c) => {
        const items = list.filter((r) => r.account.client_id === c.client_id).sort((a, b) => a.rank - b.rank);
        const text = `${c.name} ${c.group} ${items.map((r) => r.account.account_id).join(' ')}`.toLowerCase();
        return { client: c, items, rank: items.length ? Math.min(...items.map((r) => r.rank)) : 4, text };
      })
      .filter((g) => g.items.length > 0)
      .filter((g) => (dq ? g.text.includes(dq) : true))
      .filter((g) => (onlyProblems && !dq ? g.rank < 3 : true))
      .sort((a, b) => a.rank - b.rank || a.client.name.localeCompare(b.client.name));
    return { groups, flat: list };
  }, [accounts, view.legs, view.overview, dq, onlyProblems]);

  const firstId = useMemo(() => [...rail.flat].sort((a, b) => a.rank - b.rank)[0]?.account.account_id ?? null, [rail.flat]);
  const [sel, setSel] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab | null>(null);
  const [mode, setModeRaw] = useState<'one' | 'four'>(() => {
    try {
      return localStorage.getItem('slg.bookview') === 'one' ? 'one' : 'four';
    } catch {
      return 'four';
    }
  });
  const setMode = (m: 'one' | 'four') => {
    setModeRaw(m);
    try {
      localStorage.setItem('slg.bookview', m);
    } catch {
      /* private window: the choice just is not remembered */
    }
  };
  const [page, setPage] = useState(0);
  useEffect(() => setPage(0), [dq, onlyProblems]);
  useEffect(() => {
    if (sel === null || !accounts.some((a) => a.account_id === sel)) setSel(firstId);
  }, [sel, firstId, accounts]);

  // Another page can ask for an account (the overview grid does): honour #book/<account id>.
  useEffect(() => {
    const apply = () => {
      const m = location.hash.match(/^#book\/(.+)$/);
      const id = m ? decodeURIComponent(m[1]) : null;
      if (id && accounts.some((a) => a.account_id === id)) {
        setSel(id);
        setModeRaw('one');
      }
    };
    apply();
    window.addEventListener('hashchange', apply);
    return () => window.removeEventListener('hashchange', apply);
  }, [accounts]);

  const acct = accounts.find((a) => a.account_id === sel);
  const { data: book, loading, error } = usePolled(
    (signal) => (mode === 'one' && sel ? fetchBook(token, sel, signal) : Promise.resolve(null)),
    [token, sel, mode],
    4000,
    onAuthLost,
  );
  const fresh = book && book.account_id === sel ? book : null;
  const legs = fresh?.legs ?? [];
  const orders = fresh?.orders ?? [];
  const trades = fresh?.trades ?? [];
  const openOrders = orders.filter((o) => OPEN_STATES.has(o.state));
  const activeTab = tab ?? (openOrders.length === 0 && orders.length > 0 ? 'all' : 'open');
  const bad = legs.filter((l) => l.severity === 'RED' || l.severity === 'AMBER').length;
  const waiting = legs.filter((l) => l.severity === 'GREY').length;
  const bought = trades.filter((t) => t.side === 'BUY').reduce((s, t) => s + t.qty * t.price, 0);
  const sold = trades.filter((t) => t.side === 'SELL').reduce((s, t) => s + t.qty * t.price, 0);
  const pnls = legs.map(legPnl);
  const pnlTotal = pnls.reduce<number>((s, v) => s + (v ?? 0), 0);
  const exposure = legs.reduce((s, l) => s + Math.abs(l.net_qty) * (l.ltp ?? l.avg_price), 0);
  const acctIncidents = view.incidents.filter((i) => i.account_id === sel);

  if (accounts.length === 0) {
    return (
      <section className="core">
        <Empty title="No accounts yet">Broker accounts appear here once they are added to the desk.</Empty>
      </section>
    );
  }

  const shown = rail.groups.reduce((n, g) => n + g.items.length, 0);
  const skeleton = (
    <div className="skeleton" style={{ padding: 'var(--s4)' }} aria-busy="true">
      <div />
    </div>
  );

  const viewSwitch = (
    <div className="seg" role="group" aria-label="Layout">
      <button type="button" aria-pressed={mode === 'four'} onClick={() => setMode('four')}>
        Client boards
      </button>
      <button type="button" aria-pressed={mode === 'one'} onClick={() => setMode('one')}>
        One account
      </button>
    </div>
  );

  if (mode === 'four') {
    const PAGE = 4;
    const pages = Math.max(1, Math.ceil(rail.groups.length / PAGE));
    const pg = Math.min(page, pages - 1);
    const slice = rail.groups.slice(pg * PAGE, pg * PAGE + PAGE);
    return (
      <div className="boards">
        <div className="boards__bar">
          {viewSwitch}
          <div className="search search--sm">
            <Search size={16} aria-hidden="true" />
            <input aria-label="Find a client or account" placeholder="Find client or account" value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          <label className="check">
            <input type="checkbox" checked={onlyProblems} onChange={(e) => setOnlyProblems(e.target.checked)} />
            <span>Needs attention only</span>
          </label>
          <span className="rail-count">
            {rail.groups.length} of {view.overview.length} clients · {shown} accounts
          </span>
          {pages > 1 && <div className="boards__pager">
            <button type="button" className="btn" disabled={pg === 0} onClick={() => setPage(pg - 1)}>
              Previous
            </button>
            <span>
              {pg + 1} of {pages}
            </span>
            <button type="button" className="btn" disabled={pg >= pages - 1} onClick={() => setPage(pg + 1)}>
              Next
            </button>
          </div>}
        </div>
        {slice.length === 0 ? (
          <section className="core">
            <Empty title={dq ? 'No matches' : 'No accounts need attention'}>{dq ? 'Try another client or account name.' : 'Clear the filter to see all accounts.'}</Empty>
          </section>
        ) : (
          <div className="boards__grid">
            {slice.map((g, i) => (
              <ClientBoard
                key={g.client.client_id}
                client={g.client}
                items={g.items}
                token={token}
                onAuthLost={onAuthLost}
                onOpen={onOpen}
                selected={selected}
                onPick={(id) => {
                  setSel(id);
                  setMode('one');
                }}
                index={i}
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="book">
      <aside className="book__rail tray" aria-label="Accounts">
        <div className="core">
          <div className="rail-tools">
            {viewSwitch}
            <div className="search search--sm">
              <Search size={16} aria-hidden="true" />
              <input aria-label="Find a client or account" placeholder="Find client or account" value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
            <label className="check">
              <input type="checkbox" checked={onlyProblems} onChange={(e) => setOnlyProblems(e.target.checked)} />
              <span>Needs attention only</span>
            </label>
            <span className="rail-count">
              {rail.groups.length} of {view.overview.length} clients · {shown} accounts
            </span>
          </div>
          <div className="rail-scroll">
            {rail.groups.length === 0 && <p className="rail-empty">{dq ? 'No client or account matches.' : 'Nothing needs attention right now.'}</p>}
            {rail.groups.map((g) => (
              <div key={g.client.client_id} className="rail-group">
                <div className="rail-group__head">
                  <span className="avatar" aria-hidden="true">
                    {initials(g.client.name)}
                  </span>
                  <span className="rail-group__name">
                    <b>{g.client.name}</b>
                    <small>{g.client.group}</small>
                  </span>
                </div>
                {g.items.map((r) => {
                  const a = r.account;
                  const ring = r.rank <= 1 ? 'red' : r.rank === 2 ? 'amber' : 'ok';
                  const n = r.red + r.amber;
                  return (
                    <button key={a.account_id} type="button" className="rail-item" aria-current={sel === a.account_id ? 'true' : undefined} onClick={() => setSel(a.account_id)}>
                      <span className={`brk brk--${ring}`} aria-hidden="true">
                        {BROKER_NAME[a.broker].slice(0, 1)}
                      </span>
                      <span className="rail-item__text">
                        <b>{BROKER_NAME[a.broker]}</b>
                        <small>{a.account_id}</small>
                      </span>
                      {a.status === 'FAILED' ? (
                        <span className="badge badge--down">Down</span>
                      ) : n > 0 ? (
                        <span className={`badge ${r.red ? 'badge--red' : 'badge--amber'}`}>{n}</span>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      </aside>

      {acct && (
        <section className="book__main" key={acct.account_id}>
          <section className="core book-summary" aria-label="Account summary">
            <header className="acct-head">
              <div className="acct-head__identity">
                <h2>{BROKER_NAME[acct.broker]}</h2>
                <p className="acct-head__sub">{look.clientName(acct.client_id)} <span aria-hidden="true">·</span> {acct.account_id}</p>
              </div>
              <dl className="acct-head__meta">
                <div><dt>Connection</dt><dd><ConnectionPill status={acct.status} /></dd></div>
                <div><dt>Last good read</dt><dd>{fmtAgo(acct.last_ok, now)}</dd></div>
                <div><dt>Latency</dt><dd>{acct.latency_ms == null ? '—' : `${acct.latency_ms} ms`}</dd></div>
                <div><dt>Polls today</dt><dd>{fmtInt(acct.polls)}</dd></div>
              </dl>
            </header>
            <dl className="stats">
              <div><dt>Open positions</dt><dd>{fresh ? legs.length : '—'}</dd></div>
              <div className={bad > 0 ? 'is-bad' : undefined}>
                <dt>Stops to review</dt>
                <dd>{fresh ? bad : '—'}{waiting > 0 && <small>{waiting} unverified</small>}</dd>
              </div>
              <div><dt>Open orders</dt><dd>{fresh ? openOrders.length : '—'}</dd></div>
              <div><dt>Executed trades</dt><dd>{fresh ? trades.length : '—'}</dd></div>
              <div className={pnlTotal > 0 ? 'is-up' : pnlTotal < 0 ? 'is-bad' : undefined}>
                <dt>Open P&amp;L</dt>
                <dd>{fresh ? (pnlTotal === 0 ? fmtRupee(0) : fmtSigned(pnlTotal)) : '—'}</dd>
              </div>
              <div><dt>Exposure</dt><dd>{fresh ? fmtRupee(exposure) : '—'}</dd></div>
            </dl>
          </section>

          {acct.status === 'FAILED' && (
            <div className="banner banner--warn" role="status">
              <span className="banner__text">
                <strong>This account is not reporting.</strong> {acct.last_error ?? 'Its stops cannot be verified until it reconnects.'}
              </span>
            </div>
          )}
          {error && !fresh && !loading && (
            <div className="banner banner--warn" role="alert">
              <span className="banner__text">
                <strong>Could not load this account.</strong> {error}
              </span>
            </div>
          )}

          <div className={`book__cols${fresh && (legs.length === 0 || orders.length === 0) ? ' book__cols--stacked' : ''}`}>
            <BookPanel title="Open positions" count={fresh ? legs.length : undefined}>
              {!fresh ? skeleton : legs.length === 0 ? <p className="acct__none">{acct.status === 'OK' ? 'No open positions.' : 'Open positions could not be verified.'}</p> : <PositionsTable legs={legs} onOpen={onOpen} selected={selected} />}
            </BookPanel>

            <BookPanel
              title="Orders and trades"
              actions={
                <div className="seg" role="group" aria-label="Order book view">
                  {([
                    ['open', 'Open orders', openOrders.length],
                    ['fills', 'Executions', trades.length],
                    ['all', 'All orders', orders.length],
                  ] as const).map(([id, label, count]) => (
                    <button key={id} type="button" aria-pressed={activeTab === id} onClick={() => setTab(id)}>
                      {label} {fresh && <i>{count}</i>}
                    </button>
                  ))}
                </div>
              }
            >
              {!fresh ? (
                skeleton
              ) : activeTab === 'fills' ? (
                trades.length === 0 ? (
                  <p className="acct__none">No executed trades today.</p>
                ) : (
                  <TradesTable rows={trades} />
                )
              ) : (activeTab === 'open' ? openOrders : orders).length === 0 ? (
                <p className="acct__none">{activeTab === 'open' ? 'No open orders.' : 'No orders today.'}</p>
              ) : (
                <OrdersTable rows={activeTab === 'open' ? openOrders : orders} />
              )}
              {fresh && activeTab === 'fills' && trades.length > 0 && (
                <div className="core__foot">
                  <span>
                    Bought <b className="num">{fmtRupee(bought)}</b>
                  </span>
                  <span>
                    Sold <b className="num">{fmtRupee(sold)}</b>
                  </span>
                </div>
              )}
            </BookPanel>
          </div>

          <BookPanel title="Account incidents" count={acctIncidents.length}>
            {acctIncidents.length === 0 ? (
              <p className="acct__none">No stop-loss incidents recorded today.</p>
            ) : (
              <IncidentsTable incidents={acctIncidents} now={now} look={look} showClient={false} onAck={onAck} ackBusy={ackBusy} onOpen={onOpen} selected={selected} />
            )}
          </BookPanel>
        </section>
      )}
    </div>
  );
}

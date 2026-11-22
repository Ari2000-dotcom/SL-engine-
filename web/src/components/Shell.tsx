import { useEffect, useMemo, useRef, useState } from 'react';
import { BROKER_NAME, STATUS_LABEL } from '../format';
import { Asleep, Light, Logout, Notification, NotificationOff, Search, Security } from '../icons';
import type { Link } from '../hooks';
import { fmtDuration } from '../format';
import type { Account, ClientOverview, Incident, Role } from '../types';

export type Page = 'overview' | 'book' | 'legs' | 'incidents' | 'accounts' | 'risk';

export const ITEMS: { id: Page; label: string; adminOnly?: boolean }[] = [
  { id: 'overview', label: 'Overview' },
  { id: 'book', label: 'Account books' },
  { id: 'legs', label: 'All open legs' },
  { id: 'incidents', label: 'Incidents' },
  { id: 'accounts', label: 'Connections', adminOnly: true },
  { id: 'risk', label: 'Risk settings', adminOnly: true },
];

export const PAGE_TITLE: Record<Page, string> = {
  overview: 'Desk overview',
  book: 'Account books',
  legs: 'All open legs',
  incidents: 'Incidents',
  accounts: 'Connections',
  risk: 'Risk settings',
};

// NSE cash hours, Monday to Friday 09:15 to 15:30 IST. Holidays are not modelled.
function istParts(now: number) {
  const f = new Intl.DateTimeFormat('en-GB', {
    timeZone: 'Asia/Kolkata',
    hour12: false,
    weekday: 'short',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).formatToParts(new Date(now));
  const g = (t: string) => f.find((x) => x.type === t)?.value ?? '';
  return { wd: g('weekday'), h: Number(g('hour')) % 24, m: Number(g('minute')), text: `${g('hour')}:${g('minute')}:${g('second')}` };
}

export function marketOpen(now: number): boolean {
  const t = istParts(now);
  if (t.wd === 'Sat' || t.wd === 'Sun') return false;
  const mins = t.h * 60 + t.m;
  return mins >= 9 * 60 + 15 && mins < 15 * 60 + 30;
}


interface Hit {
  kind: 'Page' | 'Client' | 'Account' | 'Symbol';
  label: string;
  sub: string;
  hash: string;
}

interface CommandProps {
  pages: { id: Page; label: string }[];
  clients: ClientOverview[];
  accounts: Account[];
  symbols: string[];
  onGo: (hash: string) => void;
}

/** The terminal's command line: type a page, client, account or symbol, then Enter. Ctrl+K focuses it. */
export function Command(p: CommandProps) {
  const [q, setQ] = useState('');
  const [open, setOpen] = useState(false);
  const [idx, setIdx] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const hits = useMemo<Hit[]>(() => {
    const n = q.trim().toLowerCase();
    if (!n) return [];
    const out: Hit[] = [];
    p.pages.filter((x) => x.label.toLowerCase().includes(n) || x.id.includes(n)).forEach((x) => out.push({ kind: 'Page', label: x.label, sub: 'Page', hash: x.id }));
    p.clients
      .filter((c) => `${c.name} ${c.group}`.toLowerCase().includes(n))
      .slice(0, 5)
      .forEach((c) => {
        const acc = p.accounts.find((a) => a.client_id === c.client_id);
        if (acc) out.push({ kind: 'Client', label: c.name, sub: c.group, hash: `book/${encodeURIComponent(acc.account_id)}` });
      });
    const names = new Map(p.clients.map((c) => [c.client_id, c.name]));
    p.accounts
      .filter((a) => `${a.account_id} ${BROKER_NAME[a.broker]}`.toLowerCase().includes(n))
      .slice(0, 5)
      .forEach((a) => out.push({ kind: 'Account', label: `${BROKER_NAME[a.broker]} ${a.account_id}`, sub: names.get(a.client_id) ?? '', hash: `book/${encodeURIComponent(a.account_id)}` }));
    p.symbols
      .filter((s) => s.toLowerCase().includes(n))
      .slice(0, 4)
      .forEach((s) => out.push({ kind: 'Symbol', label: s, sub: 'Open legs', hash: `legs/${encodeURIComponent(s)}` }));
    return out.slice(0, 12);
  }, [q, p.pages, p.clients, p.accounts, p.symbols]);

  const run = (h: Hit | undefined) => {
    if (!h) return;
    p.onGo(h.hash);
    setQ('');
    setOpen(false);
    input.current?.blur();
  };

  return (
    <div className="cmd">
      <Search size={16} aria-hidden="true" />
      <input
        ref={input}
        className="cmd__input"
        aria-label="Command line"
        placeholder="Go to client, account or symbol"
        value={q}
        autoComplete="off"
        spellCheck={false}
        onFocus={() => setOpen(true)}
        onBlur={() => window.setTimeout(() => setOpen(false), 120)}
        onChange={(e) => {
          setQ(e.target.value);
          setIdx(0);
          setOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown') {
            e.preventDefault();
            setIdx((i) => Math.min(hits.length - 1, i + 1));
          } else if (e.key === 'ArrowUp') {
            e.preventDefault();
            setIdx((i) => Math.max(0, i - 1));
          } else if (e.key === 'Enter') {
            run(hits[idx]);
          } else if (e.key === 'Escape') {
            setQ('');
            input.current?.blur();
          }
        }}
      />
      <kbd>Ctrl K</kbd>
      {open && q.trim() !== '' && (
        <ul className="cmd__list" role="listbox">
          {hits.length === 0 && <li className="cmd__none">Nothing matches.</li>}
          {hits.map((h, i) => (
            <li key={`${h.kind}-${h.hash}`} role="option" aria-selected={i === idx} className="cmd__item" onMouseDown={(e) => e.preventDefault()} onClick={() => run(h)} onMouseEnter={() => setIdx(i)}>
              <span className="cmd__kind">{h.kind}</span>
              <span className="cmd__label">{h.label}</span>
              <span className="cmd__sub">{h.sub}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

interface NavProps {
  page: Page;
  onPage: (p: Page) => void;
  role: Role;
  username: string;
  openIncidents: number;
  link: Link;
  lastAt: number;
  now: number;
  dark: boolean;
  onToggleDark: () => void;
  sound: boolean;
  onToggleSound: () => void;
  onLogout: () => void;
  clients: ClientOverview[];
  accounts: Account[];
  symbols: string[];
  onGo: (hash: string) => void;
}

/** One square navigation container, like QuantDesk's: brand, page tabs, then status and controls. */
export function Nav(p: NavProps) {
  const items = ITEMS.filter((x) => !x.adminOnly || p.role === 'ADMIN');
  const age = Math.max(0, Math.round((p.now - p.lastAt) / 1000));
  const linkText = p.link === 'live' ? `Live · ${age}s` : p.link === 'connecting' ? 'Connecting' : 'Offline';
  const mkt = marketOpen(p.now);
  return (
    <nav className="nav" aria-label="Sections">
      <div className="nav__brand">
        <Security size={16} aria-hidden="true" />
        <span className="nav__title">SL Guardian</span>
      </div>
      <span className="nav__rule" aria-hidden="true" />
      <Command pages={items} clients={p.clients} accounts={p.accounts} symbols={p.symbols} onGo={p.onGo} />
      <span className="nav__rule nav__rule--cmd" aria-hidden="true" />
      <div className="tabs" role="group" aria-label="Pages">
        {items.map((x) => (
          <button key={x.id} type="button" className="tabs__item" aria-pressed={p.page === x.id} onClick={() => p.onPage(x.id)}>
            {x.label}
            {x.id === 'incidents' && p.openIncidents > 0 && <span className="tabs__count">{p.openIncidents}</span>}
          </button>
        ))}
      </div>
      <span className="nav__spacer" />
      <span className="chip nav__clock">
        <span className={`dot ${mkt ? '' : 'dot--grey'}`} aria-hidden="true" />
        NSE {mkt ? 'open' : 'closed'} <b className="num">{istParts(p.now).text}</b> IST
      </span>
      <span className="chip" role="status">
        <span className={`dot dot--${p.link} ${p.link === 'live' ? 'dot--pulse' : ''}`} aria-hidden="true" />
        {linkText}
      </span>
      <span className="nav__rule" aria-hidden="true" />
      <button type="button" className="iconbtn" onClick={p.onToggleSound} aria-label={p.sound ? 'Mute alert sound' : 'Turn on alert sound'} title={p.sound ? 'Alert sound on' : 'Alert sound off'}>
        {p.sound ? <Notification size={16} /> : <NotificationOff size={16} />}
      </button>
      <button type="button" className="iconbtn" onClick={p.onToggleDark} aria-label={p.dark ? 'Switch to light theme' : 'Switch to dark theme'} title={p.dark ? 'Light theme' : 'Dark theme'}>
        {p.dark ? <Light size={16} /> : <Asleep size={16} />}
      </button>
      <span className="nav__rule" aria-hidden="true" />
      <span className="nav__user" title={p.role === 'ADMIN' ? 'Administrator' : 'Client'}>
        <span className="nav__avatar">{p.username.slice(0, 1)}</span>
        <span className="nav__name">{p.username}</span>
      </span>
      <button type="button" className="iconbtn" onClick={p.onLogout} aria-label="Sign out" title="Sign out">
        <Logout size={16} />
      </button>
    </nav>
  );
}

interface TickerProps {
  red: number;
  amber: number;
  open: Incident[];
  accountsDown: number;
  now: number;
  totals: { total: number; green: number };
  onNav: (p: Page) => void;
}

/** The desk's own strip under the navigation, like QuantDesk's index ticker. */
export function Ticker(p: TickerProps) {
  const longest = p.open.reduce((m, i) => Math.max(m, (p.now - new Date(i.opened_at).getTime()) / 1000), 0);
  // Newest unacknowledged incidents first, capped so the tape stays readable.
  const tape = [...p.open].filter((i) => !i.acknowledged_at).sort((a, b) => b.id - a.id).slice(0, 12);
  return (
    <div className="ticker" role="status" aria-label="Desk status">
      <span className={`tick ${p.red ? 'tick--red' : 'tick--ok'}`}>
        No stop-loss <b className="num">{p.red}</b>
      </span>
      <span className={`tick ${p.amber ? 'tick--amber' : 'tick--ok'}`}>
        Partial or far <b className="num">{p.amber}</b>
      </span>
      <span className="tick">
        Protected <b className="num">{p.totals.green}</b> of <b className="num">{p.totals.total}</b>
      </span>
      {p.open.length > 0 && (
        <span className="tick">
          Longest unprotected <b className="num">{fmtDuration(longest)}</b>
        </span>
      )}
      {p.accountsDown > 0 && (
        <span className="tick tick--amber">
          Feeds down <b className="num">{p.accountsDown}</b>
        </span>
      )}
      {tape.length > 0 && (
        <button type="button" className="tape" onClick={() => p.onNav('incidents')} title="Open incidents" aria-label="Open incidents, latest first">
          <span className="tape__track">
            {[0, 1].map((copy) => (
              <span key={copy} className="tape__run" aria-hidden={copy === 1}>
                {tape.map((i) => (
                  <span key={`${copy}-${i.id}`} className={`tape__item tape__item--${i.worst_status === 'NO_SL' || i.worst_status === 'SL_REJECTED' || i.worst_status === 'SL_CANCELLED' || i.worst_status === 'WRONG_SIDE' ? 'red' : 'amber'}`}>
                    <i aria-hidden="true" />
                    <b>{i.symbol}</b> {i.account_id} {STATUS_LABEL[i.current_status].toLowerCase()} · {fmtDuration((p.now - new Date(i.opened_at).getTime()) / 1000)}
                  </span>
                ))}
              </span>
            ))}
          </span>
        </button>
      )}
    </div>
  );
}

interface StatusProps {
  role: Role;
  username: string;
  now: number;
  lastAt: number;
  link: Link;
  page: Page;
  onPage: (p: Page) => void;
  accounts: { status: string; latency_ms: number | null }[];
  legs: number;
}

/** A terminal's bottom line: feed health, the function keys, and who is signed in. */
export function StatusBar(p: StatusProps) {
  const items = ITEMS.filter((x) => !x.adminOnly || p.role === 'ADMIN');
  const ok = p.accounts.filter((a) => a.status === 'OK').length;
  const lat = p.accounts.map((a) => a.latency_ms).filter((v): v is number => v != null);
  const avg = lat.length ? Math.round(lat.reduce((s, v) => s + v, 0) / lat.length) : null;
  const age = p.lastAt ? Math.max(0, Math.round((p.now - p.lastAt) / 1000)) : null;
  return (
    <footer className="status" aria-label="Status bar">
      <span className="status__cell">
        <span className={`dot dot--${p.link === 'live' ? '' : p.link}`} aria-hidden="true" />
        Feeds <b className="num">{ok}</b>/<b className="num">{p.accounts.length}</b>
      </span>
      <span className="status__cell">
        Latency <b className="num">{avg == null ? '-' : `${avg} ms`}</b>
      </span>
      <span className="status__cell">
        Legs <b className="num">{p.legs}</b>
      </span>
      <span className="status__keys" role="group" aria-label="Function keys">
        {items.map((x, i) => (
          <button key={x.id} type="button" className="fkey" aria-pressed={p.page === x.id} onClick={() => p.onPage(x.id)} title={`${x.label} (F${i + 1})`}>
            <kbd>F{i + 1}</kbd>
            <span>{x.label}</span>
          </button>
        ))}
      </span>
      <span className="status__spacer" />
      <span className="status__cell">
        Updated <b className="num">{age == null ? '-' : `${age}s ago`}</b>
      </span>
      <span className="status__cell">
        {p.username} <span className="status__role">{p.role === 'ADMIN' ? 'admin' : 'client'}</span>
      </span>
    </footer>
  );
}

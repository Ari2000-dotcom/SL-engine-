import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { BROKER_NAME } from '../format';
import { Tooltip } from './arc/tooltip/tooltip';
import { Asleep, Book, Close, ConnectionSignal, Dashboard, Light, Logout, Notification, NotificationOff, Search, Security, Settings, SidePanelClose, SidePanelOpen, Table, WarningAlt } from '../icons';
import type { Link } from '../hooks';
import type { Account, ClientOverview, Role } from '../types';

export type Page = 'overview' | 'book' | 'legs' | 'incidents' | 'accounts' | 'risk';

export const ITEMS: { id: Page; label: string; adminOnly?: boolean }[] = [
  { id: 'overview', label: 'Overview' },
  { id: 'book', label: 'Account books' },
  { id: 'legs', label: 'All open legs' },
  { id: 'incidents', label: 'Incidents' },
  { id: 'accounts', label: 'Connections', adminOnly: true },
  { id: 'risk', label: 'Risk settings', adminOnly: true },
];

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

function marketOpen(t: ReturnType<typeof istParts>): boolean {
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
  suspendShortcut?: boolean;
  onGo: (hash: string) => void;
}

/** The terminal's command line: type a page, client, account or symbol, then Enter. Ctrl+K focuses it. */
export function Command(p: CommandProps) {
  const [q, setQ] = useState('');
  const [open, setOpen] = useState(false);
  const [idx, setIdx] = useState(0);
  const resultsId = useId();
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (p.suspendShortcut) return;
    const onKey = (e: KeyboardEvent) => {
      if (!e.defaultPrevented && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [p.suspendShortcut]);

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
  const showResults = open && q.trim() !== '';

  return (
    <div className="cmd">
      <Search size={16} aria-hidden="true" />
      <input
        ref={input}
        className="cmd__input"
        aria-label="Search the desk"
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={showResults}
        aria-controls={showResults ? resultsId : undefined}
        aria-activedescendant={showResults && hits[idx] ? `${resultsId}-${idx}` : undefined}
        placeholder="Search the desk…"
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
            setIdx((i) => Math.max(0, Math.min(hits.length - 1, i + 1)));
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
      {showResults && (
        <ul id={resultsId} className="cmd__list" role="listbox" aria-label="Desk search results">
          {hits.length === 0 && <li className="cmd__none">Nothing matches.</li>}
          {hits.map((h, i) => (
              <li key={`${h.kind}-${h.hash}`} id={`${resultsId}-${i}`} role="option" aria-selected={i === idx} className="cmd__item" onMouseDown={(e) => e.preventDefault()} onClick={() => run(h)} onMouseEnter={() => setIdx(i)}>
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

const PAGE_ICONS = { overview: Dashboard, book: Book, legs: Table, incidents: WarningAlt, accounts: ConnectionSignal, risk: Settings };
const SIDEBAR_ID = 'desk-sidebar';

interface NavigationProps {
  page: Page;
  onPage: (p: Page) => void;
  role: Role;
  username: string;
  openIncidents: number;
  open: boolean;
  mobile: boolean;
  onClose: () => void;
  onLogout: () => void;
}

export function DeskBrand() {
  return (
    <div className="desk-brand">
      <span className="desk-brand__mark"><Security size={22} aria-hidden="true" /></span>
      <div><strong>RMS Desk</strong><small>SL Guardian</small></div>
    </div>
  );
}

export function Navigation({ page, onPage, role, username, openIncidents, open, mobile, onClose, onLogout }: NavigationProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const drawer = dialog.current;
    if (!mobile || !open || !drawer) return;
    drawer.showModal();
    return () => drawer.close();
  }, [mobile, open]);

  const deskItems = ITEMS.filter((item) => !item.adminOnly);
  const adminItems = ITEMS.filter((item) => item.adminOnly);
  const group = (label: string, items: typeof ITEMS, offset = 0) => (
    <div className="sidebar__group">
      <span className="sidebar__label">{label}</span>
      {items.map((item, index) => {
        const Icon = PAGE_ICONS[item.id];
        return (
          <button key={item.id} type="button" className="sidebar__item" aria-current={page === item.id ? 'page' : undefined} onClick={() => onPage(item.id)} title={`${item.label} (F${offset + index + 1})`}>
            <Icon size={20} aria-hidden="true" />
            <span>{item.label}</span>
            {item.id === 'incidents' && openIncidents > 0 && <span className="sidebar__count">{openIncidents}</span>}
          </button>
        );
      })}
    </div>
  );

  const sidebar = (
    <aside id={mobile ? undefined : SIDEBAR_ID} className="sidebar" hidden={!mobile && !open}>
      <div className="sidebar__brand">
        <DeskBrand />
        {mobile && <button type="button" className="iconbtn" onClick={onClose} aria-label="Close navigation"><Close size={20} aria-hidden="true" /></button>}
      </div>
      <nav className="sidebar__nav" aria-label="Sections">
        {group('Monitor', deskItems)}
        {role === 'ADMIN' && group('Administration', adminItems, deskItems.length)}
      </nav>
      <div className="sidebar__profile">
        <span className="sidebar__avatar" aria-hidden="true">{username.slice(0, 1)}</span>
        <div className="sidebar__identity"><strong>{username}</strong><small>{role === 'ADMIN' ? 'Administrator' : 'Client'}</small></div>
        <button type="button" className="iconbtn" onClick={onLogout} aria-label="Sign out" title="Sign out"><Logout size={18} /></button>
      </div>
    </aside>
  );

  return mobile ? (
    <dialog
      id={SIDEBAR_ID}
      ref={dialog}
      className="sidebar-drawer"
      aria-label="Desk navigation"
      onCancel={(e) => { e.preventDefault(); onClose(); }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      {sidebar}
    </dialog>
  ) : sidebar;
}

interface ToolbarProps extends Omit<CommandProps, 'pages' | 'suspendShortcut'> {
  role: Role;
  link: Link;
  now: number;
  dark: boolean;
  onToggleDark: () => void;
  sound: boolean;
  onToggleSound: () => void;
  sidebarOpen: boolean;
  mobile: boolean;
  onToggleSidebar: () => void;
  suspendShortcuts: boolean;
}

export function Toolbar({ role, link, now, dark, onToggleDark, sound, onToggleSound, sidebarOpen, mobile, onToggleSidebar, suspendShortcuts, ...search }: ToolbarProps) {
  const clock = istParts(now);
  const market = marketOpen(clock);
  const linkText = link === 'live' ? 'Desk online' : link === 'connecting' ? 'Connecting' : 'Desk offline';

  return (
    <header className="toolbar">
      <div className="toolbar__start">
        <Tooltip content={sidebarOpen ? 'Hide sidebar' : 'Show sidebar'} side="bottom">
          <button
            type="button"
            className="iconbtn toolbar__menu"
            aria-label={sidebarOpen ? 'Hide sidebar' : 'Show sidebar'}
            aria-expanded={sidebarOpen}
            aria-controls={SIDEBAR_ID}
            aria-haspopup={mobile ? 'dialog' : undefined}
            onClick={onToggleSidebar}
          >
            {sidebarOpen ? <SidePanelClose size={20} aria-hidden="true" /> : <SidePanelOpen size={20} aria-hidden="true" />}
          </button>
        </Tooltip>
        {!mobile && !sidebarOpen && <span className="toolbar__name">RMS Desk</span>}
        <Command pages={ITEMS.filter((item) => !item.adminOnly || role === 'ADMIN')} suspendShortcut={suspendShortcuts} {...search} />
      </div>
      <div className="toolbar__status">
        <span className="toolbar__clock" title="Regular NSE trading hours">
          NSE {market ? 'open' : 'closed'} <b className="num">{clock.text}</b> IST
        </span>
        <span className={`feed feed--${link}`} role="status"><i aria-hidden="true" />{linkText}</span>
        <div className="toolbar__controls">
          <Tooltip content={sound ? 'Mute alert sound' : 'Turn on alert sound'} side="bottom">
            <button type="button" className="iconbtn" onClick={onToggleSound} aria-label={sound ? 'Mute alert sound' : 'Turn on alert sound'}>
              {sound ? <Notification size={20} aria-hidden="true" /> : <NotificationOff size={20} aria-hidden="true" />}
            </button>
          </Tooltip>
          <Tooltip content={dark ? 'Light theme' : 'Dark theme'} side="bottom">
            <button type="button" className="iconbtn" onClick={onToggleDark} aria-label={dark ? 'Switch to light theme' : 'Switch to dark theme'}>
              {dark ? <Light size={20} aria-hidden="true" /> : <Asleep size={20} aria-hidden="true" />}
            </button>
          </Tooltip>
        </div>
      </div>
    </header>
  );
}

export function StatusBar({ now, lastAt, accounts }: { now: number; lastAt: number; accounts: Account[] }) {
  const reporting = accounts.filter((a) => a.status === 'OK').length;
  const latencies = accounts.flatMap((a) => a.latency_ms == null ? [] : [a.latency_ms]);
  const avg = latencies.length ? Math.round(latencies.reduce((sum, ms) => sum + ms, 0) / latencies.length) : null;
  const age = Math.max(0, Math.round((now - lastAt) / 1000));

  return (
    <footer className="desk-status" aria-label="Status bar">
      <span>Read-only monitoring</span>
      <span>{accounts.length ? <><b>{reporting}/{accounts.length}</b> broker feeds reporting</> : 'No broker accounts'}</span>
      {avg != null && <span>Average latency <b>{avg} ms</b></span>}
      <span className="desk-status__updated">{lastAt ? <>Updated <b>{age}s ago</b></> : 'Awaiting data'}</span>
    </footer>
  );
}

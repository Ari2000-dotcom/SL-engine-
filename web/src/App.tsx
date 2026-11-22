import { useCallback, useEffect, useMemo, useState } from 'react';
import { AuthExpired, acknowledge, loadSession, saveSession } from './api';
import { LegDrawer } from './components/LegDrawer';
import { ITEMS, Nav, StatusBar, Ticker, type Page } from './components/Shell';
import { makeLookups } from './components/tables';
import { useAlerts, useLive, useNow } from './hooks';
import { Accounts } from './pages/Accounts';
import { Incidents } from './pages/Incidents';
import { Book } from './pages/Book';
import { Legs } from './pages/Legs';
import { Login } from './pages/Login';
import { Overview } from './pages/Overview';
import { Risk } from './pages/Risk';
import type { Leg, Session } from './types';

const PAGES: Page[] = ['overview', 'book', 'legs', 'incidents', 'accounts', 'risk'];

function pageFromHash(): Page {
  const h = location.hash.replace('#', '').split('/')[0] as Page;
  return PAGES.includes(h) ? h : 'overview';
}

// Dark is the default: desks run dark. A saved choice wins.
function useTheme() {
  const [dark, setDark] = useState(() => {
    try {
      const saved = localStorage.getItem('slg.theme');
      if (saved) return saved === 'dark';
    } catch {
      /* storage blocked */
    }
    return true;
  });
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    document.documentElement.dataset.carbonTheme = dark ? 'g100' : 'g10';
    try {
      localStorage.setItem('slg.theme', dark ? 'dark' : 'light');
    } catch {
      /* storage blocked */
    }
  }, [dark]);
  return [dark, () => setDark((d) => !d)] as const;
}

function Console({ session, onLogout, dark, toggleDark }: { session: Session; onLogout: () => void; dark: boolean; toggleDark: () => void }) {
  const [page, setPage] = useState<Page>(pageFromHash);
  const [sound, setSound] = useState(true);
  const [ackBusy, setAckBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  // Protected legs are not in the live push, so the row that was clicked is kept for the panel.
  const [clicked, setClicked] = useState<Leg | null>(null);
  const openLeg = useCallback((key: string, leg?: Leg) => {
    setSelected(key);
    setClicked(leg ?? null);
  }, []);
  const now = useNow();
  const { state, link, lastAt } = useLive(session.token, onLogout);
  const { unlock } = useAlerts(state?.incidents, state?.accounts, sound);

  useEffect(() => {
    const onHash = () => setPage(pageFromHash());
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  const admin = session.user.role === 'ADMIN';
  const current: Page = !admin && (page === 'accounts' || page === 'risk') ? 'overview' : page;

  const go = useCallback((p: Page) => {
    location.hash = p;
    setPage(p);
    setSelected(null);
  }, []);

  // 1 to 5 jump between sections, unless the user is typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      const typing = ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName);
      if (e.metaKey || e.ctrlKey || e.altKey || (typing && !/^F[1-9]$/.test(e.key))) return;
      const visible = ITEMS.filter((x) => admin || !x.adminOnly).map((x) => x.id);
      const f = /^F([1-9])$/.exec(e.key);
      const p = visible[Number(f ? f[1] : e.key) - 1];
      if (p) {
        e.preventDefault();
        go(p);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [admin, go]);

  const look = useMemo(() => makeLookups(state?.overview ?? [], state?.accounts ?? []), [state?.overview, state?.accounts]);

  const onAck = async (id: number) => {
    setAckBusy(id);
    setError(null);
    try {
      await acknowledge(session.token, id);
    } catch (e) {
      if (e instanceof AuthExpired) return onLogout();
      setError(e instanceof Error ? e.message : 'Could not acknowledge');
    } finally {
      setAckBusy(null);
    }
  };

  const open = state?.incidents.filter((i) => !i.resolved_at) ?? [];
  const symbols = useMemo(() => [...new Set((state?.legs ?? []).map((l) => l.symbol))].sort(), [state?.legs]);
  const red = state?.totals.red ?? 0;
  const amber = state?.totals.amber ?? 0;
  const down = state?.accounts.filter((a) => a.status === 'FAILED').length ?? 0;

  const selLeg = state?.legs.find((l) => l.leg_key === selected) ?? (clicked?.leg_key === selected ? clicked : undefined);
  // Prefer the open incident for the leg, otherwise the most recent one.
  const selInc = state?.incidents
    .filter((i) => i.leg_key === selected)
    .sort((a, b) => Number(!a.resolved_at) - Number(!b.resolved_at) || b.id - a.id)[0];

  return (
    <div className="app" onClick={unlock}>
      <Nav
        page={current}
        onPage={go}
        role={session.user.role}
        username={session.user.username}
        openIncidents={open.length}
        link={link}
        lastAt={lastAt}
        now={now}
        dark={dark}
        onToggleDark={toggleDark}
        sound={sound}
        onToggleSound={() => {
          unlock();
          setSound((s) => !s);
        }}
        onLogout={onLogout}
        clients={state?.overview ?? []}
        accounts={state?.accounts ?? []}
        symbols={symbols}
        onGo={(h) => {
          location.hash = h;
          setSelected(null);
        }}
      />
      <Ticker red={red} amber={amber} open={open} accountsDown={down} now={now} totals={state?.totals ?? { total: 0, green: 0 }} onNav={go} />
      <div className="main">
        <main className="content">
          {error && (
            <div className="banner banner--warn" role="alert">
              <span className="banner__text">
                <strong>{error}</strong>
              </span>
            </div>
          )}
          {!state ? (
            <div className="skeleton" aria-busy="true" aria-label="Loading the latest state">
              <div />
              <div style={{ height: '14rem' }} />
            </div>
          ) : current === 'overview' ? (
            <Overview view={state} now={now} look={look} onAck={onAck} ackBusy={ackBusy} onNav={go} onOpen={openLeg} selected={selected} />
          ) : current === 'book' ? (
            <Book view={state} now={now} look={look} token={session.token} onAuthLost={onLogout} onOpen={openLeg} selected={selected} onAck={onAck} ackBusy={ackBusy} />
          ) : current === 'legs' ? (
            <Legs view={state} look={look} token={session.token} onAuthLost={onLogout} onOpen={openLeg} selected={selected} />
          ) : current === 'incidents' ? (
            <Incidents view={state} now={now} look={look} onAck={onAck} ackBusy={ackBusy} onOpen={openLeg} selected={selected} />
          ) : current === 'accounts' ? (
            <Accounts view={state} now={now} look={look} token={session.token} admin={admin} onAuthLost={onLogout} />
          ) : (
            <Risk view={state} token={session.token} />
          )}
        </main>
      </div>
      <StatusBar
        role={session.user.role}
        username={session.user.username}
        now={now}
        lastAt={lastAt}
        link={link}
        page={current}
        onPage={go}
        accounts={state?.accounts ?? []}
        legs={state?.totals.total ?? 0}
      />
      {(selLeg || selInc) && <LegDrawer leg={selLeg} incident={selInc} look={look} now={now} onClose={() => setSelected(null)} onAck={onAck} ackBusy={ackBusy} />}
    </div>
  );
}

export default function App() {
  const [session, setSession] = useState<Session | null>(loadSession);
  const [dark, toggleDark] = useTheme();

  const set = (s: Session | null) => {
    saveSession(s);
    setSession(s);
  };

  return session ? <Console session={session} onLogout={() => set(null)} dark={dark} toggleDark={toggleDark} /> : <Login onSession={set} />;
}

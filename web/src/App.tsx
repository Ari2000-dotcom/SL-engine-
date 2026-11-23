import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { AuthExpired, acknowledge, loadSession, saveSession } from './api';
import { LegDrawer } from './components/LegDrawer';
import { PageHead } from './components/cards';
import { ITEMS, Navigation, StatusBar, Toolbar, type Page } from './components/Shell';
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
const MOBILE_NAV = '(max-width: 760px)';
const SIDEBAR_PREF = 'slg.sidebar';

function pageFromHash(): Page {
  const h = location.hash.replace('#', '').split('/')[0] as Page;
  return PAGES.includes(h) ? h : 'overview';
}

// Keep the user's chosen theme across sessions.
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
  const [mobile, setMobile] = useState(() => matchMedia(MOBILE_NAV).matches);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [desktopSidebarOpen, setDesktopSidebarOpen] = useState(() => {
    try {
      return localStorage.getItem(SIDEBAR_PREF) !== 'closed';
    } catch {
      return true;
    }
  });
  const sidebarOpen = mobile ? drawerOpen : desktopSidebarOpen;
  const modalNavigationOpen = mobile && drawerOpen;
  const scrollArea = useRef<HTMLDivElement>(null);
  const closeLeg = useCallback(() => setSelected(null), []);
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
    const media = matchMedia(MOBILE_NAV);
    const onResize = (e: MediaQueryListEvent) => {
      setMobile(e.matches);
      setDrawerOpen(false);
    };
    media.addEventListener('change', onResize);
    return () => media.removeEventListener('change', onResize);
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem(SIDEBAR_PREF, desktopSidebarOpen ? 'open' : 'closed');
    } catch {
      /* storage blocked */
    }
  }, [desktopSidebarOpen]);

  useEffect(() => {
    const onHash = () => setPage(pageFromHash());
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  const admin = session.user.role === 'ADMIN';
  const current: Page = !admin && (page === 'accounts' || page === 'risk') ? 'overview' : page;
  useEffect(() => {
    scrollArea.current?.scrollTo({ top: 0, left: 0 });
  }, [current]);

  const go = useCallback((p: Page) => {
    location.hash = p;
    setPage(p);
    setSelected(null);
    setDrawerOpen(false);
  }, []);

  // Number and function keys jump between the visible sections.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      const typing = ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName);
      if (selected || modalNavigationOpen || e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || (typing && !/^F[1-9]$/.test(e.key))) return;
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
  }, [admin, go, selected, modalNavigationOpen]);

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

  const selLeg = state?.legs.find((l) => l.leg_key === selected) ?? (clicked?.leg_key === selected ? clicked : undefined);
  // Prefer the open incident for the leg, otherwise the most recent one.
  const selInc = state?.incidents
    .filter((i) => i.leg_key === selected)
    .sort((a, b) => Number(!a.resolved_at) - Number(!b.resolved_at) || b.id - a.id)[0];

  return (
    <div className={`app${!sidebarOpen ? ' app--sidebar-closed' : ''}`} onClick={unlock}>
      <Navigation
        page={current}
        onPage={go}
        role={session.user.role}
        username={session.user.username}
        openIncidents={open.length}
        open={sidebarOpen}
        mobile={mobile}
        onClose={() => setDrawerOpen(false)}
        onLogout={onLogout}
      />
      <div className="workspace">
        <Toolbar
          role={session.user.role}
          link={link}
          now={now}
          dark={dark}
          onToggleDark={toggleDark}
          sound={sound}
          onToggleSound={() => {
            unlock();
            setSound((s) => !s);
          }}
          clients={state?.overview ?? []}
          accounts={state?.accounts ?? []}
          symbols={symbols}
          sidebarOpen={sidebarOpen}
          mobile={mobile}
          onToggleSidebar={() => {
            if (mobile) setDrawerOpen((value) => !value);
            else setDesktopSidebarOpen((value) => !value);
          }}
          suspendShortcuts={modalNavigationOpen || selected !== null}
          onGo={(h) => {
            location.hash = h;
            setSelected(null);
          }}
        />
        <div className="main" ref={scrollArea}>
          <main className="content">
            {link === 'down' && (
              <div className="banner banner--warn" role="status">
                {state ? 'Live feed disconnected. Showing the last available snapshot.' : 'Live feed disconnected. Reconnecting…'}
              </div>
            )}
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
              <>
                <PageHead title="Account books" />
                <Book view={state} now={now} look={look} token={session.token} onAuthLost={onLogout} onOpen={openLeg} selected={selected} onAck={onAck} ackBusy={ackBusy} />
              </>
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
        <StatusBar now={now} lastAt={lastAt} accounts={state?.accounts ?? []} />
      </div>
      {(selLeg || selInc) && <LegDrawer leg={selLeg} incident={selInc} look={look} now={now} onClose={closeLeg} onAck={onAck} ackBusy={ackBusy} />}
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

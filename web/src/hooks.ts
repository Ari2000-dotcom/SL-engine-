import { useCallback, useEffect, useRef, useState } from 'react';
import { wsUrl } from './api';
import type { Account, Incident, StateView } from './types';

export type Link = 'connecting' | 'live' | 'down';

/** Live server state over a websocket, with automatic reconnect. */
export function useLive(token: string, onAuthLost: () => void) {
  const [state, setState] = useState<StateView | null>(null);
  const [link, setLink] = useState<Link>('connecting');
  const [lastAt, setLastAt] = useState<number>(0);
  const authLost = useRef(onAuthLost);
  authLost.current = onAuthLost;

  useEffect(() => {
    let closed = false;
    let ws: WebSocket | null = null;
    let retry: number | undefined;
    let attempt = 0;

    const connect = () => {
      setLink((l) => (l === 'live' ? l : 'connecting'));
      ws = new WebSocket(wsUrl(token));
      ws.onopen = () => {
        attempt = 0;
        setLink('live');
      };
      ws.onmessage = (e) => {
        try {
          setState(JSON.parse(e.data as string) as StateView);
          setLastAt(Date.now());
        } catch {
          /* ignore a malformed frame, the next push replaces it */
        }
      };
      ws.onclose = async () => {
        if (closed) return;
        setLink('down');
        // A rejected upgrade looks like a close; check whether the token is still valid.
        try {
          const r = await fetch('/api/state', { headers: { authorization: `Bearer ${token}` } });
          if (closed) return;
          if (r.status === 401) return authLost.current();
          if (r.ok) setState((await r.json()) as StateView);
        } catch {
          /* server unreachable, keep retrying */
        }
        // The component may have unmounted (logout) while the check above was in flight.
        if (closed) return;
        attempt += 1;
        retry = window.setTimeout(() => {
          if (!closed) connect();
        }, Math.min(1000 * 2 ** attempt, 15000));
      };
    };
    connect();
    return () => {
      closed = true;
      window.clearTimeout(retry);
      ws?.close();
    };
  }, [token]);

  return { state, link, lastAt };
}

/** A ticking clock so durations on screen keep moving between server pushes. */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}

/** Sound and desktop notification for new unacknowledged incidents and for broker feeds that drop. */
export function useAlerts(incidents: Incident[] | undefined, accounts: Account[] | undefined, enabled: boolean) {
  const seen = useRef<Set<string> | null>(null);
  const seenDown = useRef<Set<string> | null>(null);
  const ctx = useRef<AudioContext | null>(null);

  // Browsers only allow audio and notification prompts after a user gesture. Any click on the
  // page counts, so the first click after sign-in (including the sign-in click) readies both.
  const unlock = useCallback(() => {
    try {
      ctx.current ??= new AudioContext();
      void ctx.current.resume();
    } catch {
      /* no audio support */
    }
    if ('Notification' in window && Notification.permission === 'default') void Notification.requestPermission();
  }, []);

  const beep = useCallback(() => {
    try {
      ctx.current ??= new AudioContext();
    } catch {
      return;
    }
    const ac = ctx.current;
    void ac.resume();
    [880, 660].forEach((freq, i) => {
      const osc = ac.createOscillator();
      const gain = ac.createGain();
      osc.type = 'sine';
      osc.frequency.value = freq;
      const t = ac.currentTime + i * 0.22;
      gain.gain.setValueAtTime(0.0001, t);
      gain.gain.exponentialRampToValueAtTime(0.25, t + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.2);
      osc.connect(gain).connect(ac.destination);
      osc.start(t);
      osc.stop(t + 0.22);
    });
  }, []);

  const notify = useCallback((title: string, body: string) => {
    if ('Notification' in window && Notification.permission === 'granted') new Notification(title, { body });
  }, []);

  useEffect(() => {
    if (!incidents) return;
    const open = incidents.filter((i) => !i.resolved_at && !i.acknowledged_at);
    const keys = new Set(open.map((i) => `${i.id}:${i.worst_status}`));
    if (seen.current === null) {
      seen.current = keys; // first load: show state, do not alarm for what already existed
      return;
    }
    const fresh = open.filter((i) => !seen.current!.has(`${i.id}:${i.worst_status}`));
    seen.current = keys;
    if (fresh.length === 0 || !enabled) return;
    // Amber problems (partial or too-far stops) also deserve a sound, a quieter reminder is not
    // worth the risk of a missed one.
    beep();
    const first = fresh[0];
    notify('Stop-loss alert', `${first.symbol}: ${first.worst_status.replace(/_/g, ' ').toLowerCase()}`);
  }, [incidents, enabled, beep, notify]);

  useEffect(() => {
    if (!accounts) return;
    const down = new Set(accounts.filter((a) => a.status === 'FAILED').map((a) => a.account_id));
    if (seenDown.current === null) {
      seenDown.current = down;
      return;
    }
    const fresh = [...down].filter((id) => !seenDown.current!.has(id));
    seenDown.current = down;
    if (fresh.length === 0 || !enabled) return;
    beep();
    notify('Broker feed down', `${fresh.length} account(s) stopped reporting. Their stops cannot be verified.`);
  }, [accounts, enabled, beep, notify]);

  return { unlock };
}

/**
 * Fetch on mount and whenever `deps` change, then refresh on an interval. Stale answers are
 * dropped (a newer request aborts the older one), and the previous data stays on screen while a
 * refresh is in flight so tables do not flicker.
 */
export function usePolled<T>(load: (signal: AbortSignal) => Promise<T>, deps: unknown[], everyMs: number, onAuthLost: () => void) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const loadRef = useRef(load);
  loadRef.current = load;
  const lost = useRef(onAuthLost);
  lost.current = onAuthLost;

  useEffect(() => {
    let stop = false;
    let ctl: AbortController | null = null;
    let timer: number | undefined;
    setLoading(true);
    const run = async () => {
      ctl?.abort();
      ctl = new AbortController();
      try {
        const d = await loadRef.current(ctl.signal);
        if (stop) return;
        setData(d);
        setError(null);
      } catch (e) {
        if (stop || (e instanceof DOMException && e.name === 'AbortError')) return;
        if (e instanceof Error && e.message === 'Session expired') return lost.current();
        setError(e instanceof Error ? e.message : 'Could not load');
      } finally {
        if (!stop) setLoading(false);
      }
      if (!stop) timer = window.setTimeout(run, everyMs);
    };
    void run();
    return () => {
      stop = true;
      ctl?.abort();
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  return { data, error, loading };
}

/** Value that follows `value` after it has been still for `ms`. Keeps typing from hammering the API. */
export function useDebounced<T>(value: T, ms = 250): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const id = window.setTimeout(() => setV(value), ms);
    return () => window.clearTimeout(id);
  }, [value, ms]);
  return v;
}

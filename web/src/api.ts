import type { BookView, LegPage, RiskBook, Session } from './types';

const KEY = 'slg.session';

export function loadSession(): Session | null {
  try {
    const raw = sessionStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as Session) : null;
  } catch {
    return null;
  }
}

export function saveSession(s: Session | null): void {
  try {
    if (s) sessionStorage.setItem(KEY, JSON.stringify(s));
    else sessionStorage.removeItem(KEY);
  } catch {
    /* storage can be blocked; the session then lives only in memory */
  }
}

export async function login(username: string, password: string): Promise<Session> {
  const res = await fetch('/api/login', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ username, password }),
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error((body as { error?: string }).error ?? 'Could not sign in');
  }
  return (await res.json()) as Session;
}

function authHeaders(token: string): HeadersInit {
  return { authorization: `Bearer ${token}`, 'content-type': 'application/json' };
}

export class AuthExpired extends Error {}

async function check(res: Response): Promise<void> {
  if (res.status === 401) throw new AuthExpired('Session expired');
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error((body as { error?: string }).error ?? `Request failed (${res.status})`);
  }
}

export async function acknowledge(token: string, id: number): Promise<void> {
  await check(await fetch(`/api/incidents/${id}/ack`, { method: 'POST', headers: authHeaders(token) }));
}

export async function saveRisk(token: string, book: RiskBook): Promise<void> {
  await check(await fetch('/api/risk', { method: 'PUT', headers: authHeaders(token), body: JSON.stringify(book) }));
}

export function wsUrl(token: string): string {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${location.host}/ws?token=${encodeURIComponent(token)}`;
}

async function getJson<T>(token: string, url: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(url, { headers: authHeaders(token), signal });
  await check(res);
  return (await res.json()) as T;
}

export const fetchBook = (token: string, accountId: string, signal?: AbortSignal) =>
  getJson<BookView>(token, `/api/accounts/${encodeURIComponent(accountId)}/book`, signal);

export interface LegFilter {
  severity: string;
  client_id: string;
  broker: string;
  q: string;
  offset: number;
  limit: number;
}

export function fetchLegs(token: string, f: LegFilter, signal?: AbortSignal) {
  const p = new URLSearchParams();
  if (f.severity !== 'ALL') p.set('severity', f.severity);
  if (f.client_id !== 'ALL') p.set('client_id', f.client_id);
  if (f.broker !== 'ALL') p.set('broker', f.broker);
  if (f.q.trim()) p.set('q', f.q.trim());
  p.set('offset', String(f.offset));
  p.set('limit', String(f.limit));
  return getJson<LegPage>(token, `/api/legs?${p}`, signal);
}

// ---------- Broker accounts the administrator adds (keys are sent once and never returned) ----------
export interface ManagedAccount {
  account_id: string;
  client_id: string;
  broker: string;
  label: string;
  has_secret: boolean;
  has_token: boolean;
  auto_login: boolean;
  token_at: string | null;
}

export interface NewAccount {
  client_id?: string;
  new_client?: { name: string; group: string };
  broker: 'ZERODHA' | 'ANGEL' | 'KOTAK';
  label?: string;
  api_key: string;
  api_secret?: string;
  access_token?: string;
  request_token?: string;
  /** Broker-specific secrets: user_id, password, client_code, pin, mobile, ucc, mpin, totp_secret. */
  extra?: Record<string, string>;
}

export const fetchManaged = (token: string, signal?: AbortSignal) => getJson<{ enabled: boolean; accounts: ManagedAccount[] }>(token, '/api/admin/accounts', signal);

export async function addAccount(token: string, body: NewAccount): Promise<{ account_id: string }> {
  const res = await fetch('/api/admin/accounts', { method: 'POST', headers: authHeaders(token), body: JSON.stringify(body) });
  await check(res);
  return (await res.json()) as { account_id: string };
}

export async function setAccountToken(token: string, id: string, body: { access_token?: string; request_token?: string }): Promise<void> {
  await check(await fetch(`/api/admin/accounts/${encodeURIComponent(id)}/session`, { method: 'PUT', headers: authHeaders(token), body: JSON.stringify(body) }));
}

export async function removeAccount(token: string, id: string): Promise<void> {
  await check(await fetch(`/api/admin/accounts/${encodeURIComponent(id)}`, { method: 'DELETE', headers: authHeaders(token) }));
}

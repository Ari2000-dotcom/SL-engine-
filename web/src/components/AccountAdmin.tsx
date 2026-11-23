import { useState } from 'react';
import { addAccount, removeAccount, setAccountToken, type ManagedAccount, type NewAccount } from '../api';
import { BROKER_NAME, fmtAgo } from '../format';
import type { Broker, ClientOverview } from '../types';

const NEW = '__new__';
type Pick = 'ZERODHA' | 'ANGEL' | 'KOTAK';

/** One labelled text box. Secrets are password boxes, so they are not shown on screen or saved by the browser. */
function Field({ label, hint, value, onChange, secret, max = 128, placeholder }: { label: string; hint?: string; value: string; onChange: (v: string) => void; secret?: boolean; max?: number; placeholder?: string }) {
  return (
    <label className="fld">
      <span>{label}</span>
      <input type={secret ? 'password' : 'text'} value={value} onChange={(e) => onChange(e.target.value)} maxLength={max} placeholder={placeholder} spellCheck={false} autoCapitalize="off" autoComplete={secret ? 'new-password' : 'off'} />
      {hint && <small>{hint}</small>}
    </label>
  );
}

/** The one form for putting a broker account on the desk. Keys go to the server once and are not shown again. */
export function AddAccount({ token, clients, onDone, onCancel }: { token: string; clients: ClientOverview[]; onDone: () => void; onCancel: () => void }) {
  const [client, setClient] = useState(clients[0]?.client_id ?? NEW);
  const [broker, setBroker] = useState<Pick>('ZERODHA');
  const [name, setName] = useState('');
  const [desk, setDesk] = useState('');
  const [label, setLabel] = useState('');
  const [f, setF] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (k: string) => (v: string) => setF((p) => ({ ...p, [k]: v }));
  const v = (k: string) => f[k] ?? '';
  const filled = (...keys: string[]) => keys.every((k) => v(k).trim() !== '');

  const isNew = client === NEW;
  const zAuto = ['user_id', 'password', 'totp_secret'].some((k) => v(k).trim() !== '');
  const brokerReady =
    broker === 'ANGEL'
      ? filled('api_key', 'client_code', 'pin', 'totp_secret')
      : broker === 'KOTAK'
        ? filled('api_key', 'mobile', 'ucc', 'mpin', 'totp_secret')
        : filled('api_key') && (!zAuto || filled('user_id', 'password', 'totp_secret', 'api_secret'));
  const ready = brokerReady && (!isNew || (name.trim() !== '' && desk.trim() !== ''));

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const t = (k: string) => v(k).trim() || undefined;
    const extra: Record<string, string> = {};
    const take = (...keys: string[]) => keys.forEach((k) => v(k).trim() && (extra[k] = v(k).trim()));
    if (broker === 'ANGEL') take('client_code', 'pin', 'totp_secret');
    if (broker === 'KOTAK') take('mobile', 'ucc', 'mpin', 'totp_secret');
    if (broker === 'ZERODHA') take('user_id', 'password', 'totp_secret');
    const body: NewAccount = {
      ...(isNew ? { new_client: { name: name.trim(), group: desk.trim() } } : { client_id: client }),
      broker,
      label: label.trim() || undefined,
      api_key: v('api_key').trim(),
      api_secret: broker === 'ZERODHA' ? t('api_secret') : undefined,
      access_token: broker === 'ZERODHA' ? t('access_token') : undefined,
      request_token: broker === 'ZERODHA' ? t('request_token') : undefined,
      extra: Object.keys(extra).length ? extra : undefined,
    };
    try {
      await addAccount(token, body);
      setF({}); // nothing typed stays in the page after it is saved
      onDone();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not add the account');
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="tray">
      <div className="core">
        <div className="core__head">
          <h2>Add a broker account</h2>
          <span>Broker credentials are encrypted on the server.</span>
        </div>
        <form className="aform" onSubmit={submit} autoComplete="off">
          <div className="aform__grid">
            <label className="fld">
              <span>Client</span>
              <select value={client} onChange={(e) => setClient(e.target.value)}>
                {clients.map((c) => (
                  <option key={c.client_id} value={c.client_id}>
                    {c.name} · {c.group}
                  </option>
                ))}
                <option value={NEW}>New client…</option>
              </select>
            </label>
            <label className="fld">
              <span>Broker</span>
              <select
                value={broker}
                onChange={(e) => {
                  setBroker(e.target.value as Pick);
                  setF({});
                }}
              >
                <option value="ZERODHA">Zerodha Kite</option>
                <option value="ANGEL">Angel One</option>
                <option value="KOTAK">Kotak Neo</option>
              </select>
            </label>
            {isNew && (
              <>
                <Field label="Client name" value={name} onChange={setName} max={60} placeholder="Full name" />
                <Field label="Desk" value={desk} onChange={setDesk} max={40} placeholder="For example Mumbai desk" />
              </>
            )}
            <Field label="Label (optional)" value={label} onChange={setLabel} max={40} placeholder="For example Main trading" />

            {broker === 'ZERODHA' && (
              <>
                <Field label="API key" value={v('api_key')} onChange={set('api_key')} max={64} />
                <Field label="API secret" secret value={v('api_secret')} onChange={set('api_secret')} hint="Needed for automatic sign-in, or to use a request token." />
              </>
            )}
            {broker === 'ANGEL' && (
              <>
                <Field label="API key" value={v('api_key')} onChange={set('api_key')} max={64} hint="The short key from the SmartAPI dashboard, not the long secret key." />
                <Field label="Client code" value={v('client_code')} onChange={set('client_code')} max={40} />
                <Field label="MPIN" secret value={v('pin')} onChange={set('pin')} />
                <Field label="TOTP secret" secret value={v('totp_secret')} onChange={set('totp_secret')} hint="The text key under the QR code when TOTP was set up." />
              </>
            )}
            {broker === 'KOTAK' && (
              <>
                <Field label="API access token" secret value={v('api_key')} onChange={set('api_key')} hint="The access token from the Neo API dashboard." />
                <Field label="Registered mobile number" value={v('mobile')} onChange={set('mobile')} max={20} placeholder="+91XXXXXXXXXX" />
                <Field label="Client code (UCC)" value={v('ucc')} onChange={set('ucc')} max={40} />
                <Field label="MPIN" secret value={v('mpin')} onChange={set('mpin')} />
                <Field label="TOTP secret" secret value={v('totp_secret')} onChange={set('totp_secret')} hint="The text key under the QR code when TOTP was set up." />
              </>
            )}
          </div>

          {broker === 'ZERODHA' && (
            <fieldset className="aform__set">
              <legend>Zerodha sign-in</legend>
              <p className="aform__note">
                Use automatic sign-in or enter a fresh token each day. First-time automatic setup requires approval in Kite.
              </p>
              <div className="aform__grid">
                <Field label="Zerodha user id" value={v('user_id')} onChange={set('user_id')} max={40} hint="For automatic sign-in." />
                <Field label="Kite password" secret value={v('password')} onChange={set('password')} hint="For automatic sign-in." />
                <Field label="TOTP secret" secret value={v('totp_secret')} onChange={set('totp_secret')} hint="The External TOTP key (Can't scan? Copy key)." />
                <Field label="Or a request token" value={v('request_token')} onChange={set('request_token')} hint="Single use and short lived. Needs the API secret." />
                <Field label="Or an access token" secret value={v('access_token')} onChange={set('access_token')} hint="Stops working every morning." />
              </div>
            </fieldset>
          )}

          {error && (
            <div className="banner banner--warn" role="alert">
              <span className="banner__text">{error}</span>
            </div>
          )}
          <div className="aform__actions">
            <button type="button" className="btn" onClick={onCancel}>
              Cancel
            </button>
            <button type="submit" className="btn btn--primary" disabled={!ready || busy}>
              {busy ? 'Adding' : 'Add account'}
            </button>
          </div>
        </form>
      </div>
    </section>
  );
}

/** Accounts the administrator added: how each one signs in, a way to renew or retry it, and removal. */
export function ManagedList({ token, rows, clients, now, onChanged }: { token: string; rows: ManagedAccount[]; clients: ClientOverview[]; now: number; onChanged: () => void }) {
  const [open, setOpen] = useState<string | null>(null);
  const [tok, setTok] = useState('');
  const [kind, setKind] = useState<'request_token' | 'access_token'>('request_token');
  const [confirm, setConfirm] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ id: string; msg: string } | null>(null);
  const name = (id: string) => clients.find((c) => c.client_id === id)?.name ?? id;

  const toggleRenew = (id: string) => {
    setOpen(open === id ? null : id);
    setTok('');
    setError(null);
  };
  const retry = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await setAccountToken(token, id, {});
      onChanged();
    } catch (e) {
      setError({ id, msg: e instanceof Error ? e.message : 'Could not retry' });
    } finally {
      setBusy(false);
    }
  };
  const save = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await setAccountToken(token, id, { [kind]: tok.trim() });
      setTok('');
      setOpen(null);
      onChanged();
    } catch (e) {
      setError({ id, msg: e instanceof Error ? e.message : 'Could not save the token' });
    } finally {
      setBusy(false);
    }
  };
  const remove = async (id: string) => {
    setBusy(true);
    try {
      await removeAccount(token, id);
      setConfirm(null);
      onChanged();
    } catch (e) {
      setError({ id, msg: e instanceof Error ? e.message : 'Could not remove the account' });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="tray">
      <div className="core">
        <div className="core__head">
          <h2>
            Accounts you added <span className="count">{rows.length}</span>
          </h2>
          <span>Accounts that sign in by themselves need nothing daily; the others need a fresh Zerodha token each morning</span>
        </div>
        {rows.length === 0 ? (
          <p className="acct__none">No accounts added yet. Use Add account to connect the first one.</p>
        ) : (
          <ul className="mlist">
            {rows.map((r) => (
              <li key={r.account_id}>
                <div className="mlist__row">
                  <div className="mlist__who">
                    <b>{name(r.client_id)}</b>
                    <small>
                      {BROKER_NAME[r.broker as Broker] ?? r.broker} · {r.account_id}
                      {r.label ? ` · ${r.label}` : ''}
                    </small>
                  </div>
                  {r.auto_login ? (
                    <span className="tag tag--green">
                      <i aria-hidden="true" />
                      Signs in by itself
                    </span>
                  ) : (
                    <span className={`tag tag--${r.has_token ? 'green' : 'amber'}`}>
                      <i aria-hidden="true" />
                      {r.has_token ? `Token set ${fmtAgo(r.token_at, now)}` : "Needs today's token"}
                    </span>
                  )}
                  <div className="mlist__actions">
                    {r.auto_login ? (
                      <button type="button" className="btn" disabled={busy} onClick={() => void retry(r.account_id)}>
                        Retry sign-in
                      </button>
                    ) : (
                      <button type="button" className="btn" onClick={() => toggleRenew(r.account_id)}>
                        Renew token
                      </button>
                    )}
                    {confirm === r.account_id ? (
                      <>
                        <button type="button" className="btn btn--danger" disabled={busy} onClick={() => void remove(r.account_id)}>
                          Confirm remove
                        </button>
                        <button type="button" className="btn" onClick={() => setConfirm(null)}>
                          Keep
                        </button>
                      </>
                    ) : (
                      <button type="button" className="btn" onClick={() => setConfirm(r.account_id)}>
                        Remove
                      </button>
                    )}
                  </div>
                </div>
                {open === r.account_id && !r.auto_login && (
                  <form
                    className="mlist__renew"
                    onSubmit={(e) => {
                      e.preventDefault();
                      void save(r.account_id);
                    }}
                    autoComplete="off"
                  >
                    <select value={kind} onChange={(e) => setKind(e.target.value as typeof kind)} aria-label="Token type">
                      <option value="request_token" disabled={!r.has_secret}>
                        Request token{r.has_secret ? '' : ' (no API secret saved)'}
                      </option>
                      <option value="access_token">Access token</option>
                    </select>
                    <input type="password" value={tok} onChange={(e) => setTok(e.target.value)} maxLength={128} placeholder="Paste the token" aria-label="Token" autoComplete="new-password" />
                    <button type="submit" className="btn btn--primary" disabled={!tok.trim() || busy}>
                      Save token
                    </button>
                  </form>
                )}
                {error?.id === r.account_id && (
                  <p className="mlist__err" role="alert">
                    {error.msg}
                  </p>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}

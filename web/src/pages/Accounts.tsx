import { useState } from 'react';
import { fetchManaged } from '../api';
import { AddAccount, ManagedList } from '../components/AccountAdmin';
import { PageHead, StatCard, cellsFor } from '../components/cards';
import { fmtInt } from '../format';
import { usePolled } from '../hooks';
import { AccountsTable, type Lookups } from '../components/tables';
import type { StateView } from '../types';

export function Accounts({ view, now, look, token, admin, onAuthLost }: { view: StateView; now: number; look: Lookups; token: string; admin: boolean; onAuthLost: () => void }) {
  const [adding, setAdding] = useState(false);
  const [rev, setRev] = useState(0);
  const { data: managed } = usePolled((signal) => (admin ? fetchManaged(token, signal) : Promise.resolve(null)), [token, admin, rev], 10000, onAuthLost);
  const total = view.accounts.length;
  const failed = view.accounts.filter((a) => a.status === 'FAILED');
  const ok = view.accounts.filter((a) => a.status === 'OK').length;
  const lat = view.accounts.map((a) => a.latency_ms).filter((v): v is number => v != null);
  const avg = lat.length ? Math.round(lat.reduce((s, v) => s + v, 0) / lat.length) : null;
  const polls = view.accounts.reduce((s, a) => s + a.polls, 0);
  return (
    <>
      <PageHead
        title="Connections"
        count={`${total} accounts`}
        actions={
          admin && managed?.enabled ? (
            <button type="button" className="btn btn--primary" onClick={() => setAdding((v) => !v)} aria-expanded={adding}>
              {adding ? 'Close form' : 'Add account'}
            </button>
          ) : undefined
        }
      >
        <b className="t-green">{ok} reporting</b>
        {failed.length > 0 ? (
          <>
            , <b className="t-amber">{failed.length} not reporting</b>. A failing account cannot be verified, so its legs stay grey instead of green.
          </>
        ) : (
          <>. Every account is reading normally.</>
        )}
      </PageHead>

      {adding && (
        <AddAccount
          token={token}
          clients={view.overview}
          onCancel={() => setAdding(false)}
          onDone={() => {
            setAdding(false);
            setRev((n) => n + 1);
          }}
        />
      )}

      <div className="stats4">
        <StatCard
          label="Reporting"
          chip={failed.length === 0 ? 'All connected' : 'Mostly connected'}
          tone="green"
          value={ok}
          unit={`of ${total} accounts`}
          meta={`${total ? Math.round((ok / total) * 100) : 0}% of accounts`}
          filled={cellsFor(ok, total)}
          note="The console reads order books, trade books and positions. It never places, changes or cancels an order."
          foot="Checked on every poll"
        />
        <StatCard
          label="Not reporting"
          chip={failed.length > 0 ? 'Check logins' : 'None'}
          tone={failed.length > 0 ? 'amber' : 'green'}
          value={failed.length}
          unit={failed.length === 1 ? 'account is down' : 'accounts are down'}
          meta={failed[0]?.last_error ?? 'No login or data errors'}
          filled={cellsFor(failed.length, total)}
          barTone="amber"
          note="A down account is never shown as protected. Its legs stay grey until it reconnects."
          foot="Login or data read failing"
        />
        <StatCard
          label="Reads today"
          chip={avg == null ? 'Waiting' : 'Normal'}
          tone="green"
          value={fmtInt(polls)}
          unit="broker reads"
          meta={avg == null ? 'No latency yet' : `Average reply ${avg} ms`}
          filled={0}
          note="Each account is read again every few seconds, and on every order update from the broker."
          foot="Since the day started"
        />
      </div>

      {admin && managed && managed.enabled && <ManagedList token={token} rows={managed.accounts} clients={view.overview} now={now} onChanged={() => setRev((n) => n + 1)} />}

      <section className="tray">
        <div className="core">
          <div className="core__head">
            <h2>
              All accounts <span className="count">{total}</span>
            </h2>
            <span>
              {ok} connected, {failed.length} down
            </span>
          </div>
          <AccountsTable accounts={view.accounts} look={look} now={now} />
        </div>
      </section>
    </>
  );
}

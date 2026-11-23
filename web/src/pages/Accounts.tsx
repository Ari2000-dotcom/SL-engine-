import { useState } from 'react';
import { fetchManaged } from '../api';
import { AddAccount, ManagedList } from '../components/AccountAdmin';
import { Metric, PageHead } from '../components/cards';
import { Add, ConnectionSignal, StatTrades, WarningAlt } from '../icons';
import { fmtInt } from '../format';
import { usePolled } from '../hooks';
import { AccountsTable, Empty, type Lookups } from '../components/tables';
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
              {adding ? 'Close form' : <><Add size={16} aria-hidden="true" /> Add account</>}
            </button>
          ) : undefined
        }
      />

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

      <div className="metrics">
        <Metric label="Reporting" value={ok} unit={total ? `/ ${total}` : undefined} tone="green" icon={ConnectionSignal}>
          {total ? `${total - ok - failed.length} accounts connecting` : 'No accounts connected'}
        </Metric>
        <Metric label="Feeds down" value={failed.length} tone="amber" icon={WarningAlt}>
          {failed.length ? 'Login or data read failed' : 'No broker feed errors'}
        </Metric>
        <Metric label="Reads today" value={fmtInt(polls)} tone="blue" icon={StatTrades}>
          {avg == null ? 'Awaiting first reply' : `Average reply ${avg} ms`}
        </Metric>
      </div>

      {admin && managed?.enabled && managed.accounts.length > 0 && <ManagedList token={token} rows={managed.accounts} clients={view.overview} now={now} onChanged={() => setRev((n) => n + 1)} />}

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
          {total > 0 ? <AccountsTable accounts={view.accounts} look={look} now={now} /> : (
            <Empty icon={ConnectionSignal} title="No broker accounts">
              {admin && managed?.enabled ? 'Add an account to start reading its positions and orders.' : 'Connected accounts will appear here.'}
            </Empty>
          )}
        </div>
      </section>
    </>
  );
}

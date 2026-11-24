import { useMemo, useState } from 'react';
import { fmtDuration } from '../format';
import Switch from '../components/arc/switch/switch';
import { Gauge } from '../components/arc/gauge/gauge';
import { ClientsTable, Empty, IncidentsTable, LegsTable, type Lookups, type OpenLeg } from '../components/tables';
import { Metric, PageHead } from '../components/cards';
import type { Page } from '../components/Shell';
import { Add, ConnectionSignal, Search, StatProtected, StatUnprotected, Table, WarningAlt } from '../icons';
import type { ClientOverview, StateView } from '../types';

interface Props {
  view: StateView;
  now: number;
  look: Lookups;
  onAck: (id: number) => void;
  ackBusy: number | null;
  onNav: (p: Page) => void;
  onOpen: OpenLeg;
  selected: string | null;
}

type Tone = 'red' | 'down' | 'amber' | 'waiting' | 'ok';
const toneOf = (c: ClientOverview): Tone => (c.red > 0 ? 'red' : c.accounts_failed > 0 ? 'down' : c.amber > 0 ? 'amber' : c.grey > 0 || c.green === 0 ? 'waiting' : 'ok');
const TONE_RANK: Record<Tone, number> = { red: 0, down: 1, amber: 2, waiting: 3, ok: 4 };
const TONE_TEXT: Record<Tone, string> = { red: 'No stop-loss', down: 'Feed down', amber: 'Stop needs review', waiting: 'No verified positions', ok: 'Protected' };

/** Every client as one tile, worst first, so a desk with hundreds of clients reads in a glance. */
function DeskWall({ rows, onPick }: { rows: ClientOverview[]; onPick: (clientId: string) => void }) {
  const sorted = useMemo(
    () => [...rows].sort((a, b) => TONE_RANK[toneOf(a)] - TONE_RANK[toneOf(b)] || b.red - a.red || a.name.localeCompare(b.name)),
    [rows],
  );
  const n = (t: Tone) => rows.filter((c) => toneOf(c) === t).length;
  return (
    <section className="tray">
      <div className="core">
        <div className="core__head">
          <h2>All clients at a glance</h2>
          <ul className="legend" aria-label="Legend">
            {(['red', 'down', 'amber', 'waiting', 'ok'] as Tone[]).map((t) => (
              <li key={t}>
                <i className={`tile tile--${t}`} aria-hidden="true" />
                {TONE_TEXT[t]} <b className="num">{n(t)}</b>
              </li>
            ))}
          </ul>
        </div>
        <div className="wall" role="list">
          {sorted.map((c) => {
            const t = toneOf(c);
            return (
              <button
                key={c.client_id}
                type="button"
                role="listitem"
                className={`tile tile--${t}`}
                title={`${c.name} · ${c.group} · ${TONE_TEXT[t]}${c.red + c.amber ? ` · ${c.red + c.amber} legs` : ''}`}
                aria-label={`${c.name}: ${TONE_TEXT[t]}`}
                onClick={() => onPick(c.client_id)}
              />
            );
          })}
        </div>
      </div>
    </section>
  );
}


const dateFormatter = new Intl.DateTimeFormat('en-IN', {
  timeZone: 'Asia/Kolkata', weekday: 'short', day: 'numeric', month: 'short', year: 'numeric',
});

export function Overview({ view, now, look, onAck, ackBusy, onNav, onOpen, selected }: Props) {
  const admin = view.role === 'ADMIN';
  const { totals, accounts } = view;
  const open = view.incidents.filter((incident) => !incident.resolved_at);
  const unacked = open.filter((incident) => !incident.acknowledged_at).length;
  const failed = accounts.filter((account) => account.status === 'FAILED').length;
  const reporting = accounts.filter((account) => account.status === 'OK').length;
  const longest = open.reduce((max, incident) => Math.max(max, (now - Date.parse(incident.opened_at)) / 1000), 0);
  const coverage = [
    { label: 'Protected', count: totals.green, tone: 'green' },
    { label: 'Unprotected', count: totals.red, tone: 'red' },
    { label: 'Needs review', count: totals.amber, tone: 'amber' },
    { label: 'Awaiting verification', count: totals.grey, tone: 'neutral' },
  ] as const;

  const [q, setQ] = useState('');
  const [problemsOnly, setProblemsOnly] = useState(false);
  const clientRows = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return [...view.overview]
      .filter((c) => (!needle || `${c.name} ${c.group}`.toLowerCase().includes(needle)) && (!problemsOnly || c.red + c.amber + c.grey + c.accounts_failed > 0))
      .sort((a, b) => b.red - a.red || b.amber - a.amber || b.accounts_failed - a.accounts_failed || a.name.localeCompare(b.name));
  }, [view.overview, q, problemsOnly]);

  const openBook = (clientId: string) => {
    const account = accounts.find((a) => a.client_id === clientId);
    if (account) location.hash = `book/${encodeURIComponent(account.account_id)}`;
  };

  return (
    <>
      <PageHead
        title={admin ? 'Desk overview' : 'Your positions'}
        eyebrow={dateFormatter.format(now)}
        actions={
          <>
            <button type="button" className="btn" onClick={() => onNav('legs')}><Table size={17} aria-hidden="true" /> View open legs</button>
            {admin && accounts.length === 0 ? (
              <button type="button" className="btn btn--primary" onClick={() => onNav('accounts')}><Add size={18} aria-hidden="true" /> Connect a broker</button>
            ) : (
              <button type="button" className="btn btn--primary" onClick={() => onNav('incidents')}><WarningAlt size={17} aria-hidden="true" /> View incidents</button>
            )}
          </>
        }
      />

      <div className="metrics">
        <Metric label="Unprotected" value={totals.red} tone="red" icon={StatUnprotected}>
          {unacked ? `${unacked} awaiting acknowledgement` : 'Missing or invalid stop-loss'}
        </Metric>
        <Metric label="Needs review" value={totals.amber} tone="amber" icon={WarningAlt}>
          Coverage or trigger issues
        </Metric>
        <Metric label="Protected" value={totals.green} tone="green" icon={StatProtected}>
          {totals.total ? `Of ${totals.total} open legs` : 'No open legs to check'}
        </Metric>
        <Metric label="Broker feeds" value={reporting} unit={accounts.length ? `/ ${accounts.length}` : undefined} tone="blue" icon={ConnectionSignal}>
          {accounts.length ? <>{failed} down · {accounts.length - reporting - failed} connecting</> : 'No accounts connected'}
        </Metric>
      </div>

      <div className="overview-grid">
        <section className="core overview-incidents">
          <header className="overview-panel-head">
            <div><h2>Open incidents <span className="count">{open.length}</span></h2><p>Stop-loss exceptions that need attention</p></div>
            <button type="button" className="btn btn--quiet" onClick={() => onNav('incidents')}>View all</button>
          </header>
          {open.length > 0 ? (
            <IncidentsTable compact incidents={open} now={now} look={look} showClient={admin} onAck={onAck} ackBusy={ackBusy} onOpen={onOpen} selected={selected} />
          ) : (
            <Empty icon={WarningAlt} title="No open incidents">
              {accounts.length === 0
                ? 'Connect a broker account to start monitoring for missing or invalid stop-loss orders.'
                : reporting === 0
                  ? 'Waiting for a broker feed to report. Protection will be checked when position data arrives.'
                  : 'New stop-loss exceptions will appear here as broker data updates.'}
            </Empty>
          )}
          <footer className="overview-panel-foot"><span>Awaiting acknowledgement <b>{unacked}</b></span><span>Oldest incident <b>{open.length ? fmtDuration(longest) : '—'}</b></span></footer>
        </section>

        <section className="core coverage">
          <header className="overview-panel-head"><div><h2>Stop-loss coverage</h2><p>Breakdown of all open legs</p></div></header>
          <div className="coverage__body">
            {totals.total ? (
              <Gauge
                label="Legs protected"
                value={Math.round((totals.green / totals.total) * 100)}
                detail={`${totals.green} of ${totals.total} open legs have a valid stop-loss`}
                thresholds={[{ from: 0, tone: 'danger', label: 'Exposed' }, { from: 70, tone: 'warning', label: 'Partly covered' }, { from: 95, tone: 'success', label: 'Covered' }]}
              />
            ) : (
              <p className="coverage__headline"><strong className="num">—</strong><span>No open legs to check</span></p>
            )}
            <div className="coverage__bar" aria-hidden="true">
              {coverage.filter((item) => item.count > 0).map((item) => <span key={item.tone} className={`coverage__segment tone-${item.tone}`} style={{ flexGrow: item.count }} />)}
            </div>
            <dl className="coverage__legend">
              {coverage.map((item) => (
                <div key={item.tone}>
                  <dt><i className={`tone-${item.tone}`} aria-hidden="true" />{item.label}</dt>
                  <dd className="num">{item.count}</dd>
                </div>
              ))}
            </dl>
          </div>
        </section>
      </div>

      {admin && view.overview.length > 8 && <DeskWall rows={view.overview} onPick={openBook} />}
      {admin && (
        <section className="core">
          <header className="overview-panel-head">
            <div><h2>Clients <span className="count">{view.overview.length}</span></h2><p>Protection and feed status by client</p></div>
            {accounts.length > 0 && <button type="button" className="btn btn--quiet" onClick={() => onNav('book')}>Account books</button>}
          </header>
          {view.overview.length > 0 && (
            <div className="core__tools">
              <div className="search search--sm">
                <Search size={16} aria-hidden="true" />
                <input aria-label="Find a client" placeholder="Find a client or desk" value={q} onChange={(e) => setQ(e.target.value)} />
              </div>
              <Switch label="Needs attention only" checked={problemsOnly} onCheckedChange={setProblemsOnly} />
            </div>
          )}
          {clientRows.length ? <ClientsTable rows={clientRows} onPick={openBook} /> : (
            <Empty icon={ConnectionSignal} title={view.overview.length ? 'No clients match' : 'No broker accounts yet'}>
              {view.overview.length ? 'Change the search or attention filter to see more clients.' : 'Clients appear here once a broker account is added.'}
            </Empty>
          )}
        </section>
      )}
      {!admin && view.legs.some((leg) => leg.severity === 'RED' || leg.severity === 'AMBER') && (
        <section className="core">
          <div className="core__head"><h2>Legs that need attention</h2></div>
          <LegsTable legs={view.legs.filter((leg) => leg.severity === 'RED' || leg.severity === 'AMBER')} look={look} showClient={false} onOpen={onOpen} selected={selected} />
        </section>
      )}
    </>
  );
}

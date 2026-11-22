import { useMemo, useState } from 'react';
import { fmtDuration } from '../format';
import { ClientsTable, Empty, IncidentsTable, LegsTable, type Lookups, type OpenLeg } from '../components/tables';
import { StatCard, cellsFor } from '../components/cards';
import type { Page } from '../components/Shell';
import { Search } from '../icons';
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

type Tone = 'red' | 'down' | 'amber' | 'ok';
const toneOf = (c: ClientOverview): Tone => (c.red > 0 ? 'red' : c.accounts_failed > 0 ? 'down' : c.amber > 0 ? 'amber' : 'ok');
const TONE_RANK: Record<Tone, number> = { red: 0, down: 1, amber: 2, ok: 3 };
const TONE_TEXT: Record<Tone, string> = { red: 'No stop-loss', down: 'Feed down', amber: 'Partial or far stop', ok: 'Protected' };

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
            {(['red', 'down', 'amber', 'ok'] as Tone[]).map((t) => (
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


export function Overview({ view, now, look, onAck, ackBusy, onNav, onOpen, selected }: Props) {
  const admin = view.role === 'ADMIN';
  const tot = view.totals;
  const open = view.incidents.filter((i) => !i.resolved_at);
  const unacked = open.filter((i) => !i.acknowledged_at);
  const down = view.accounts.filter((a) => a.status === 'FAILED');
  const needsAction = view.legs.filter((l) => l.severity === 'RED' || l.severity === 'AMBER');

  const [q, setQ] = useState('');
  const [problemsOnly, setProblemsOnly] = useState(true);
  const clientRows = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return [...view.overview]
      .filter((c) => (needle ? `${c.name} ${c.group}`.toLowerCase().includes(needle) : true))
      .filter((c) => (problemsOnly && !needle ? c.red + c.amber + c.accounts_failed > 0 : true))
      .sort((a, b) => b.red - a.red || b.amber - a.amber || a.name.localeCompare(b.name));
  }, [view.overview, q, problemsOnly]);

  const openBook = (clientId: string) => {
    const acc = view.accounts.find((a) => a.client_id === clientId);
    if (acc) location.hash = `book/${encodeURIComponent(acc.account_id)}`;
  };

  const hit = view.overview.filter((c) => c.red > 0).length;
  const part = view.overview.filter((c) => c.amber > 0).length;
  const clientsN = Math.max(1, view.overview.length);
  const longest = open.reduce((m, i) => Math.max(m, (now - new Date(i.opened_at).getTime()) / 1000), 0);
  const worstClients = [...view.overview]
    .filter((c) => c.red + c.amber > 0)
    .sort((a, b) => b.red - a.red || b.amber - a.amber)
    .slice(0, 4);
  const worstLegs = worstClients.reduce((s, c) => s + c.red + c.amber, 0);

  // Nothing is connected yet. Showing green "all protected" cards here would say the desk is safe when
  // it is simply not watching anything.
  if (view.accounts.length === 0) {
    return (
      <>
        <div className="head">
          <div>
            <h1>Desk overview</h1>
            <p>Nothing is connected yet, so no stop-loss is being checked.</p>
          </div>
        </div>
        <section className="tray">
          <div className="core">
            <Empty title="No broker accounts yet">
              {admin ? 'Add the first client and broker account on the Connections page. The desk starts checking stop-losses as soon as it is connected.' : 'No accounts are linked to your login yet. Ask the desk administrator.'}
            </Empty>
            {admin && (
              <div className="aform__actions" style={{ padding: 'var(--s4) var(--gut)' }}>
                <button type="button" className="btn btn--primary" onClick={() => onNav('accounts')}>
                  Add an account
                </button>
              </div>
            )}
          </div>
        </section>
      </>
    );
  }

  return (
    <>
      <div className="head">
        <div>
          <h1>
            {admin ? 'Desk overview' : 'Your positions'}
            <span className="count">{admin ? `${view.overview.length} clients` : `${tot.total} legs`}</span>
          </h1>
          <p>
            Every open leg is checked for a valid stop-loss. <b className="t-red">{tot.red} have none</b>, <b className="t-amber">{tot.amber} are partial or too far</b>
            {down.length > 0 ? (
              <>
                , and <b className="t-amber">{down.length} {down.length === 1 ? 'account is' : 'accounts are'} not reporting</b>.
              </>
            ) : (
              <>
                , and <b className="t-green">{tot.green} are protected</b>.
              </>
            )}
          </p>
        </div>
        <div className="head__actions">
          <button type="button" className="btn" onClick={() => onNav('legs')}>
            All open legs
          </button>
          <button type="button" className="btn btn--primary" onClick={() => onNav('incidents')}>
            Open incidents{open.length > 0 ? ` (${open.length})` : ''}
          </button>
        </div>
      </div>

      <div className="stats4">
        <StatCard
          label="Unprotected now"
          chip={tot.red > 0 ? 'Needs action' : 'Clear'}
          tone={tot.red > 0 ? 'red' : 'green'}
          value={tot.red}
          unit={tot.red === 1 ? 'leg has no stop-loss' : 'legs have no stop-loss'}
          meta={`${hit} of ${clientsN} clients affected`}
          filled={cellsFor(hit, clientsN)}
          note={open.length > 0 ? `Longest without a stop: ${fmtDuration(longest)}. ${unacked.length} not yet acknowledged.` : 'Every open leg has a valid stop-loss.'}
          foot={`${open.length} open incidents`}
          action={{ label: 'Open incidents', run: () => onNav('incidents') }}
        />
        <StatCard
          label="Partial or too far"
          chip={tot.amber > 0 ? 'Needs a look' : 'Clear'}
          tone={tot.amber > 0 ? 'amber' : 'green'}
          value={tot.amber}
          unit={tot.amber === 1 ? 'leg is part-covered or far' : 'legs are part-covered or far'}
          meta={`${part} of ${clientsN} clients affected`}
          filled={cellsFor(part, clientsN)}
          note="The stop covers only part of the position, or sits further away than the loss limit allows."
          foot="Set in Risk settings"
          action={{ label: 'See legs', run: () => onNav('legs') }}
        />
        <StatCard
          label="Broker feeds"
          chip={down.length > 0 ? 'Check logins' : 'All connected'}
          tone={down.length > 0 ? 'amber' : 'green'}
          value={down.length}
          unit={`of ${view.accounts.length} accounts not reporting`}
          meta={`${view.accounts.length - down.length} reporting normally`}
          filled={cellsFor(view.accounts.length - down.length, view.accounts.length)}
          barTone="green"
          note="Stops on an account that is not reporting cannot be verified, so its legs stay grey."
          foot="Login or data read failing"
          action={admin ? { label: 'Connections', run: () => onNav('accounts') } : undefined}
        />
        <StatCard
          label="Protected"
          chip="On track"
          tone="green"
          value={tot.green}
          unit={`of ${tot.total} open legs`}
          meta={`${tot.total ? Math.round((tot.green / tot.total) * 100) : 0}% of all legs`}
          filled={cellsFor(tot.green, tot.total)}
          note="A live stop-loss covers the full position, on the right side, within the loss limit."
          foot="Checked on every update"
          action={{ label: 'All open legs', run: () => onNav('legs') }}
        />
      </div>

      <div className={`split ${admin ? 'split--wide' : ''}`}>
        <section className="tray">
          <div className="core">
            <div className="core__head">
              <h2>
                Needs action now <span className="count">{open.length}</span>
              </h2>
              <ul className="legend">
                <li>
                  <i className="sw sw--red" />
                  No stop-loss
                </li>
                <li>
                  <i className="sw sw--amber" />
                  Partial or far
                </li>
              </ul>
            </div>
            {open.length === 0 ? (
              <Empty ok title="Nothing is unprotected">
                Every open leg has a valid stop-loss. New problems appear here as soon as the next order update or poll arrives.
              </Empty>
            ) : (
              <IncidentsTable incidents={open} now={now} look={look} showClient={admin} onAck={onAck} ackBusy={ackBusy} onOpen={onOpen} selected={selected} />
            )}
            <div className="core__foot core__foot--split">
              <span>
                {tot.red} no stop-loss · {tot.amber} partial or far
              </span>
              <span>Oldest {open.length ? fmtDuration(longest) : '-'}</span>
            </div>
          </div>
        </section>

        {admin ? (
          <section className="insight">
            <header className="insight__head">
              <span>Where it concentrates</span>
              <span className="muted">
                {worstClients.length} clients · {worstLegs} legs
              </span>
            </header>
            <div className="insight__card">
              <h3>{worstClients.length ? `${worstClients[0].name} has the most legs to fix` : 'No client needs attention'}</h3>
              <p className="insight__lede">
                {worstClients.length
                  ? `The four clients below hold ${worstLegs} of the ${tot.red + tot.amber} legs that need a stop-loss action.`
                  : 'New problems appear here as soon as the next poll arrives.'}
              </p>
              <ol className="steps">
                {worstClients.map((c, i) => (
                  <li key={c.client_id}>
                    <span className="steps__n">{i + 1}</span>
                    <button type="button" className="steps__main" onClick={() => openBook(c.client_id)}>
                      <b>{c.name}</b>
                      <small>{c.group}</small>
                    </button>
                    {c.red > 0 && <span className="tag tag--red">{c.red} no stop</span>}
                    {c.amber > 0 && <span className="tag tag--amber">{c.amber} partial</span>}
                  </li>
                ))}
              </ol>
              <div className="insight__after">
                <div>
                  <small>No stop-loss</small>
                  <b className="num">
                    {tot.red} <i>→</i> {Math.max(0, tot.red - worstClients.reduce((s, c) => s + c.red, 0))}
                  </b>
                  <small>if these four are fixed</small>
                </div>
                <div>
                  <small>Partial or far</small>
                  <b className="num">
                    {tot.amber} <i>→</i> {Math.max(0, tot.amber - worstClients.reduce((s, c) => s + c.amber, 0))}
                  </b>
                  <small>if these four are fixed</small>
                </div>
              </div>
              <button type="button" className="btn btn--primary btn--block" onClick={() => onNav('incidents')}>
                Work through the incidents
              </button>
              <p className="insight__foot">Acknowledging an incident silences the alert. It closes when the stop is placed.</p>
            </div>
          </section>
        ) : (
          needsAction.length > 0 && (
            <section className="tray">
              <div className="core">
                <div className="core__head">
                  <h2>Legs to fix</h2>
                  <span>{needsAction.length} legs</span>
                </div>
                <LegsTable legs={needsAction} look={look} showClient={false} onOpen={onOpen} selected={selected} />
              </div>
            </section>
          )
        )}
      </div>

      {admin && view.overview.length > 8 && <DeskWall rows={view.overview} onPick={openBook} />}

      {admin && (
        <section className="tray">
          <div className="core">
            <div className="core__head">
              <h2>
                Clients <span className="count">{view.overview.length}</span>
              </h2>
              <span>
                {clientRows.length} shown
              </span>
            </div>
            {view.overview.length > 8 && (
              <div className="core__tools">
                <div className="search search--sm">
                  <Search size={16} aria-hidden="true" />
                  <input aria-label="Find a client" placeholder="Find a client or desk" value={q} onChange={(e) => setQ(e.target.value)} />
                </div>
                <label className="check">
                  <input type="checkbox" checked={problemsOnly} onChange={(e) => setProblemsOnly(e.target.checked)} />
                  <span>Needs attention only</span>
                </label>
              </div>
            )}
            {clientRows.length === 0 ? (
              <Empty title="No clients to show">{q ? 'No client or desk matches that search.' : 'No client needs attention right now.'}</Empty>
            ) : (
              <ClientsTable rows={clientRows} onPick={openBook} />
            )}
          </div>
        </section>
      )}
    </>
  );
}

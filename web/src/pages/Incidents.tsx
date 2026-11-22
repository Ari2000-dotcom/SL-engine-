import { useState } from 'react';
import { PageHead } from '../components/cards';
import { Empty, IncidentsTable, type Lookups, type OpenLeg } from '../components/tables';
import type { StateView } from '../types';

interface Props {
  view: StateView;
  now: number;
  look: Lookups;
  onAck: (id: number) => void;
  ackBusy: number | null;
  onOpen: OpenLeg;
  selected: string | null;
}

type Tab = 'open' | 'unseen' | 'closed';

export function Incidents({ view, now, look, onAck, ackBusy, onOpen, selected }: Props) {
  const [tab, setTab] = useState<Tab>('open');
  const open = view.incidents.filter((i) => !i.resolved_at);
  const unseen = open.filter((i) => !i.acknowledged_at);
  const closed = view.incidents.filter((i) => i.resolved_at);
  const rows = tab === 'open' ? open : tab === 'unseen' ? unseen : closed;
  const tabs: [Tab, string, number][] = [
    ['open', 'Open', open.length],
    ['unseen', 'Not acknowledged', unseen.length],
    ['closed', 'Closed', closed.length],
  ];

  return (
    <>
      <PageHead
        title="Incidents today"
        count={`${open.length} open`}
        actions={
          <div className="seg" role="group" aria-label="Incident filter">
            {tabs.map(([id, label, n]) => (
              <button key={id} type="button" aria-pressed={tab === id} onClick={() => setTab(id)}>
                {label}
                <i>{n}</i>
              </button>
            ))}
          </div>
        }
      >
        <b className="t-red">{unseen.length} not acknowledged</b>, {open.length - unseen.length} acknowledged, <b className="t-green">{closed.length} closed today</b>. An incident closes when the stop-loss is placed or the position is closed. Acknowledging only silences the alert.
      </PageHead>
      <section className="tray"><div className="core">
        <div className="core__head">
          <h2>
            {tab === 'open' ? 'Open incidents' : tab === 'unseen' ? 'Not acknowledged' : 'Closed today'} <span className="count">{rows.length}</span>
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
        {rows.length === 0 ? (
          <Empty ok={tab !== 'closed'} title={tab === 'closed' ? 'No closed incidents yet' : tab === 'unseen' ? 'Everything is acknowledged' : 'No open incidents'}>
            {tab === 'closed'
              ? 'Incidents that were fixed or whose position was closed today are listed here. The list clears at the start of each trading day.'
              : tab === 'unseen'
                ? 'Open incidents are still listed under Open until the stop-loss is placed.'
                : 'All open legs are protected right now.'}
          </Empty>
        ) : (
          <IncidentsTable incidents={rows} now={now} look={look} showClient={view.role === 'ADMIN'} onAck={onAck} ackBusy={ackBusy} onOpen={onOpen} selected={selected} />
        )}
      </div></section>
    </>
  );
}

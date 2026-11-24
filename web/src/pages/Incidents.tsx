import { useState } from 'react';
import { PageHead } from '../components/cards';
import SegmentedControl from '../components/arc/segmented-control/segmented-control';
import { Empty, IncidentsTable, type Lookups, type OpenLeg } from '../components/tables';
import { Add, ConnectionSignal, Notification, StatProtected } from '../icons';
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

const TAB_ORDER = ['open', 'unseen', 'closed'] as const;
type Tab = typeof TAB_ORDER[number];

const FILTERS = {
  open: {
    label: 'Open',
    icon: Notification,
    empty: 'No open incidents',
    detail: 'Stop-loss exceptions appear here as broker feeds update.',
  },
  unseen: {
    label: 'To acknowledge',
    icon: Notification,
    empty: 'No incidents awaiting acknowledgement',
    detail: 'Acknowledged incidents stay open until the stop-loss is placed or the position closes.',
  },
  closed: {
    label: 'Closed today',
    icon: StatProtected,
    empty: 'No incidents closed today',
    detail: 'Resolved incidents from this trading day appear here.',
  },
};

export function Incidents({ view, now, look, onAck, ackBusy, onOpen, selected }: Props) {
  const [tab, setTab] = useState<Tab>('open');
  const open = view.incidents.filter((i) => !i.resolved_at);
  const unseen = open.filter((i) => !i.acknowledged_at);
  const closed = view.incidents.filter((i) => i.resolved_at);
  const groups = { open, unseen, closed };
  const rows = groups[tab];
  const active = FILTERS[tab];
  const needsConnection = tab === 'open' && rows.length === 0 && view.accounts.length === 0;

  return (
    <>
      <PageHead title="Incidents" />
      <section className="core incident-results" aria-label="Incident queue">
        <header className="incident-results__toolbar">
          <SegmentedControl
            label="Filter incidents"
            value={tab}
            onValueChange={(v) => setTab(v as Tab)}
            options={TAB_ORDER.map((id) => ({ value: id, label: FILTERS[id].label, accessory: <span className="num">{groups[id].length}</span> }))}
          />
          {tab !== 'closed' && rows.length > 0 && (
            <ul className="legend" aria-label="Incident severity">
              <li><i className="sw sw--red" aria-hidden="true" />Critical</li>
              <li><i className="sw sw--amber" aria-hidden="true" />Needs review</li>
            </ul>
          )}
        </header>
        <div id="incident-results">
          {rows.length === 0 ? (
            <div className="incident-results__empty">
              <Empty icon={needsConnection ? ConnectionSignal : active.icon} title={needsConnection ? 'No broker feeds connected' : active.empty}>
                {needsConnection ? 'Stop-loss incidents appear once broker data is available.' : active.detail}
              </Empty>
              {needsConnection && view.role === 'ADMIN' && (
                <a className="btn btn--primary" href="#accounts"><Add size={18} aria-hidden="true" /> Connect a broker</a>
              )}
            </div>
          ) : (
            <IncidentsTable incidents={rows} now={now} look={look} showClient={view.role === 'ADMIN'} onAck={onAck} ackBusy={ackBusy} onOpen={onOpen} selected={selected} />
          )}
        </div>
      </section>
    </>
  );
}

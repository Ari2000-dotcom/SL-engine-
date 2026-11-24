import { useEffect, useRef, useState } from 'react';
import { fetchLegs } from '../api';
import { PageHead } from '../components/cards';
import SegmentedControl from '../components/arc/segmented-control/segmented-control';
import { Combobox } from '../components/arc/combobox/combobox';
import { Select } from '../components/arc/select/select';
import { Pagination } from '../components/arc/pagination/pagination';
import { Search } from '../icons';
import { Empty, LegsTable, type Lookups, type OpenLeg } from '../components/tables';
import { useDebounced, usePolled } from '../hooks';
import type { StateView } from '../types';

interface Props {
  view: StateView;
  look: Lookups;
  token: string;
  onAuthLost: () => void;
  onOpen: OpenLeg;
  selected: string | null;
}

const PAGE = 100;

/**
 * With hundreds of clients the browser must not hold every leg, so this page asks the server for
 * one filtered page at a time and refreshes it every few seconds.
 */
export function Legs({ view, look, token, onAuthLost, onOpen, selected }: Props) {
  const admin = view.role === 'ADMIN';
  const [q, setQ] = useState('');
  const [sev, setSev] = useState('ALL');
  const [client, setClient] = useState('ALL');
  const [broker, setBroker] = useState('ALL');
  const [page, setPage] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const dq = useDebounced(q, 250);

  // The command line sends #legs/<symbol>: show that symbol.
  useEffect(() => {
    const apply = () => {
      const m = /^#legs\/(.+)$/.exec(location.hash);
      if (m) setQ(decodeURIComponent(m[1]));
    };
    apply();
    window.addEventListener('hashchange', apply);
    return () => window.removeEventListener('hashchange', apply);
  }, []);

  // Any filter change goes back to the first page.
  useEffect(() => setPage(0), [dq, sev, client, broker]);

  // "/" jumps to search, as in most desk tools.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (e.key === '/' && !['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName)) {
        e.preventDefault();
        input.current?.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const { data, loading, error } = usePolled(
    (signal) => fetchLegs(token, { severity: sev, client_id: client, broker, q: dq, offset: page * PAGE, limit: PAGE }, signal),
    [token, sev, client, broker, dq, page],
    5000,
    onAuthLost,
  );

  const tot = view.totals;
  const counts: Record<string, number> = { ALL: tot.total, RED: tot.red, AMBER: tot.amber, GREEN: tot.green, GREY: tot.grey };
  const SEGS = [
    ['ALL', 'All'],
    ['RED', 'No SL'],
    ['AMBER', 'Partial / far'],
    ['GREEN', 'Protected'],
    ['GREY', 'Waiting'],
  ] as const;

  const total = data?.total ?? 0;
  const from = total === 0 ? 0 : page * PAGE + 1;
  const to = Math.min(total, (page + 1) * PAGE);
  const filtered = sev !== 'ALL' || client !== 'ALL' || broker !== 'ALL' || dq.trim() !== '';

  return (
    <>
      <PageHead title="All open legs" count={`${tot.total} legs`} />

      <div className="filters">
        <div className="search">
          <Search size={16} aria-hidden="true" />
          <input ref={input} aria-label="Search legs" placeholder="Search symbol or client" value={q} onChange={(e) => setQ(e.target.value)} />
          <kbd>/</kbd>
        </div>
        <SegmentedControl
          label="Status filter"
          value={sev}
          onValueChange={setSev}
          options={SEGS.map(([id, label]) => ({ value: id, label, accessory: <span className="num">{counts[id]}</span> }))}
        />
        {admin && (
          <Combobox
            label="Client"
            placeholder="All clients"
            emptyMessage="No client matches"
            value={client}
            onValueChange={(v) => setClient(v || 'ALL')}
            options={[
              { value: 'ALL', label: 'All clients' },
              ...[...view.overview].sort((a, b) => a.name.localeCompare(b.name)).map((c) => ({ value: c.client_id, label: c.name, keywords: [c.group] })),
            ]}
          />
        )}
        <Select
          label="Broker"
          value={broker}
          onValueChange={setBroker}
          options={[
            { value: 'ALL', label: 'All brokers' },
            { value: 'ZERODHA', label: 'Zerodha Kite' },
            { value: 'ANGEL', label: 'Angel One' },
            { value: 'KOTAK', label: 'Kotak Neo' },
          ]}
        />
      </div>

      <section className="tray">
        <div className="core">
          <div className="core__head">
            <h2>
              {filtered ? 'Filtered legs' : 'All open legs'} <span className="count">{total}</span>
            </h2>
            <span>{total === 0 ? '0 legs' : `Showing ${from}–${to}`} · loss if hit runs from the average price to the stop trigger</span>
          </div>
          {error && !data ? (
            <Empty title="Could not load legs">{error}</Empty>
          ) : !data && loading ? (
            <div className="skeleton" style={{ padding: 'var(--s4)' }} aria-busy="true">
              <div />
              <div />
            </div>
          ) : data && data.legs.length === 0 ? (
            <Empty title={filtered ? 'No legs match' : 'No open legs'}>
              {filtered ? 'Clear the search or filters to see every open leg.' : 'Open positions appear here as broker data updates.'}
            </Empty>
          ) : (
            data && <LegsTable legs={data.legs} look={look} showClient={admin} onOpen={onOpen} selected={selected} />
          )}
          {total > PAGE && (
            <div className="pager">
              <span className="muted">
                Page {page + 1} of {Math.ceil(total / PAGE)}
              </span>
              <Pagination label="Legs pages" page={page + 1} pageCount={Math.ceil(total / PAGE)} onPageChange={(p) => setPage(p - 1)} />
            </div>
          )}
        </div>
      </section>
    </>
  );
}

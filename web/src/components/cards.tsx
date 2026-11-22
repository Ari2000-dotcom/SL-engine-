import type { ReactNode } from 'react';

/** A row of small cells, filled from the left. The reference's segmented progress bar. */
export function Cells({ n, filled, tone }: { n: number; filled: number; tone: 'red' | 'amber' | 'green' | 'blue' }) {
  return (
    <span className="cells" aria-hidden="true">
      {Array.from({ length: n }, (_, i) => (
        <i key={i} className={i < filled ? `on on--${tone}` : ''} />
      ))}
    </span>
  );
}

export const cellsFor = (part: number, whole: number, n = 24) => (part <= 0 || whole <= 0 ? 0 : Math.min(n, Math.max(1, Math.round((part / whole) * n))));

export interface StatProps {
  label: string;
  chip: string;
  tone: 'red' | 'amber' | 'green';
  value: string | number;
  unit: string;
  meta: string;
  filled: number;
  barTone?: 'red' | 'amber' | 'green';
  note: string;
  foot: string;
  action?: { label: string; run: () => void };
}

/** A tray with a small header, and an inner card holding one headline figure. */
export function StatCard(p: StatProps) {
  return (
    <section className={`stat stat--${p.tone}`}>
      <header className="stat__head">
        <span className="stat__label">{p.label}</span>
        <span className={`tag tag--${p.tone}`}>
          <i aria-hidden="true" />
          {p.chip}
        </span>
      </header>
      <div className="stat__card">
        <div className="stat__big">
          <b className="num">{p.value}</b>
          <span>{p.unit}</span>
        </div>
        <div className="stat__meta">{p.meta}</div>
        <Cells n={24} filled={p.filled} tone={p.barTone ?? p.tone} />
        <p className="stat__note">{p.note}</p>
        <div className="stat__foot">
          <span>{p.foot}</span>
          {p.action && (
            <button type="button" className="btn btn--ink" onClick={p.action.run}>
              {p.action.label}
              <span aria-hidden="true">›</span>
            </button>
          )}
        </div>
      </div>
    </section>
  );
}

/** The page title row: a title with a count chip, a one-line summary, and actions on the right. */
export function PageHead({ title, count, actions, children }: { title: string; count?: string; actions?: ReactNode; children?: ReactNode }) {
  return (
    <div className="head">
      <div>
        <h1>
          {title}
          {count != null && <span className="count">{count}</span>}
        </h1>
        {children && <p>{children}</p>}
      </div>
      {actions && <div className="head__actions">{actions}</div>}
    </div>
  );
}

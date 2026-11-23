import type { ReactNode } from 'react';
import type { Table } from '../icons';

interface MetricProps {
  label: string;
  value: string | number;
  unit?: string;
  tone: 'red' | 'amber' | 'green' | 'blue';
  icon: typeof Table;
  children: ReactNode;
}

export function Metric({ label, value, unit, tone, icon: Icon, children }: MetricProps) {
  return (
    <section className={`metric metric--${tone}`}>
      <span className="metric__icon"><Icon size={22} aria-hidden="true" /></span>
      <h2>{label}</h2>
      <p className="metric__value"><strong className="num">{value}</strong>{unit && <small className="num">{unit}</small>}</p>
      <p className="metric__detail">{children}</p>
    </section>
  );
}

export function PageHead({ title, count, eyebrow, actions, children }: { title: string; count?: string; eyebrow?: string; actions?: ReactNode; children?: ReactNode }) {
  return (
    <header className="head">
      <div>
        {eyebrow && <div className="head__eyebrow">{eyebrow}</div>}
        <h1>
          {title}
          {count != null && <span className="count">{count}</span>}
        </h1>
        {children && <p>{children}</p>}
      </div>
      {actions && <div className="head__actions">{actions}</div>}
    </header>
  );
}

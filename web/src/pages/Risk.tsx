import { Add, TrashCan } from '../icons';
import { useState } from 'react';
import { saveRisk } from '../api';
import { PageHead } from '../components/cards';
import type { RiskRule, RiskSetting, Segment, StateView } from '../types';

const SEGMENTS: Segment[] = ['EQ', 'FUT', 'OPT', 'CUR', 'COM'];
const DEFAULT: RiskSetting = { mode: 'PCT', max_pct: 40, max_rupee: 25000, grace_sec: 15 };

function validate(rules: RiskRule[]): string | null {
  if (!rules.some((r) => r.client_id === null && r.segment === null)) {
    return 'Keep one default rule for all clients and all segments.';
  }
  const seen = new Set<string>();
  for (const r of rules) {
    const key = `${r.client_id ?? '*'}|${r.segment ?? '*'}`;
    if (seen.has(key)) return 'Two rules have the same client and segment. Remove one.';
    seen.add(key);
    const s = r.setting;
    // Only the field for the chosen mode matters; the other one is ignored.
    if (s.mode === 'PCT' && !(s.max_pct > 0 && s.max_pct <= 1000)) return 'Maximum loss % must be above 0 and at most 1000.';
    if (s.mode === 'RUPEE' && !(s.max_rupee >= 0)) return 'Maximum loss in rupees must be zero or more.';
    if (!(Number.isInteger(s.grace_sec) && s.grace_sec >= 0 && s.grace_sec <= 3600)) {
      return 'Grace period must be a whole number of seconds from 0 to 3600.';
    }
  }
  return null;
}

export function Risk({ view, token }: { view: StateView; token: string }) {
  const initial = view.risk?.rules.length ? view.risk.rules : [{ client_id: null, segment: null, setting: DEFAULT }];
  const [rules, setRules] = useState<RiskRule[]>(initial);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ kind: 'success' | 'error'; text: string } | null>(null);

  const patch = (i: number, fn: (r: RiskRule) => RiskRule) => {
    setMsg(null);
    setRules((rs) => rs.map((r, j) => (j === i ? fn(r) : r)));
  };
  const num = (v: string) => (v === '' ? NaN : Number(v));

  const save = async () => {
    const problem = validate(rules);
    if (problem) return setMsg({ kind: 'error', text: problem });
    setBusy(true);
    try {
      // A cleared field in the unused mode would serialize as null, so send the default instead.
      const clean = rules.map((r) => ({
        ...r,
        setting: {
          ...r.setting,
          max_pct: Number.isFinite(r.setting.max_pct) ? r.setting.max_pct : DEFAULT.max_pct,
          max_rupee: Number.isFinite(r.setting.max_rupee) ? r.setting.max_rupee : DEFAULT.max_rupee,
        },
      }));
      await saveRisk(token, { rules: clean });
      setMsg({ kind: 'success', text: 'Saved. Every leg was re-checked against the new limits.' });
    } catch (e) {
      setMsg({ kind: 'error', text: e instanceof Error ? e.message : 'Could not save' });
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <PageHead title="Risk settings" count={`${rules.length} ${rules.length === 1 ? 'rule' : 'rules'}`} />
      <section className="core risk-rules" aria-label="Loss limits">
        <div className="core__head">
          <h2>Loss limits</h2>
          <span>The most specific client and segment rule takes priority.</span>
        </div>
        <form className="risk-form" onSubmit={(e) => { e.preventDefault(); void save(); }} noValidate>
          {rules.map((r, i) => {
            const defaultRule = r.client_id === null && r.segment === null;
            const percent = r.setting.mode === 'PCT';
            const limit = percent ? 'max_pct' : 'max_rupee';
            return (
              <fieldset key={i} className="risk-rule">
                <legend>{defaultRule ? 'Default rule' : `Rule ${i + 1}`}</legend>
                <div className="risk-grid">
                  <label className="fld">
                    <span>Client</span>
                    <select value={r.client_id ?? ''} onChange={(e) => patch(i, (x) => ({ ...x, client_id: e.target.value || null }))}>
                      <option value="">All clients</option>
                      {view.overview.map((c) => <option key={c.client_id} value={c.client_id}>{c.name}</option>)}
                    </select>
                  </label>
                  <label className="fld">
                    <span>Segment</span>
                    <select value={r.segment ?? ''} onChange={(e) => patch(i, (x) => ({ ...x, segment: (e.target.value || null) as Segment | null }))}>
                      <option value="">All segments</option>
                      {SEGMENTS.map((segment) => <option key={segment} value={segment}>{segment}</option>)}
                    </select>
                  </label>
                  <label className="fld">
                    <span>Check loss by</span>
                    <select value={r.setting.mode} onChange={(e) => patch(i, (x) => ({ ...x, setting: { ...x.setting, mode: e.target.value as RiskSetting['mode'] } }))}>
                      <option value="PCT">% of entry price</option>
                      <option value="RUPEE">Rupees per leg</option>
                    </select>
                  </label>
                  <label className="fld">
                    <span>{percent ? 'Maximum loss (%)' : 'Maximum loss (₹)'}</span>
                    <input type="number" step="any" value={Number.isNaN(r.setting[limit]) ? '' : r.setting[limit]} onChange={(e) => patch(i, (x) => ({ ...x, setting: { ...x.setting, [limit]: num(e.target.value) } }))} />
                  </label>
                  <label className="fld">
                    <span>Grace (seconds)</span>
                    <input type="number" min="0" max="3600" value={Number.isNaN(r.setting.grace_sec) ? '' : r.setting.grace_sec} onChange={(e) => patch(i, (x) => ({ ...x, setting: { ...x.setting, grace_sec: num(e.target.value) } }))} />
                  </label>
                  {!defaultRule && (
                    <button type="button" className="btn risk-grid__remove" onClick={() => {
                      setMsg(null);
                      setRules((rs) => rs.filter((_, j) => j !== i));
                    }}>
                      <TrashCan size={16} aria-hidden="true" /> Remove
                    </button>
                  )}
                </div>
              </fieldset>
            );
          })}
          <p className="risk-form__hint">Grace is the time allowed for a new position to receive a stop-loss.</p>
          {msg && <div className={`banner${msg.kind === 'success' ? ' banner--success' : ''}`} role={msg.kind === 'error' ? 'alert' : 'status'}>{msg.text}</div>}
          <div className="form-actions">
            <button type="button" className="btn" onClick={() => {
              setMsg(null);
              setRules((rs) => [...rs, { client_id: null, segment: 'OPT', setting: { ...DEFAULT } }]);
            }}>
              <Add size={16} aria-hidden="true" /> Add rule
            </button>
            <button type="submit" className="btn btn--primary" disabled={busy}>
              {busy ? 'Saving…' : 'Save changes'}
            </button>
          </div>
        </form>
      </section>
    </>
  );
}

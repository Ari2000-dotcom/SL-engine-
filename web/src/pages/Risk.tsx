import { Add, TrashCan } from '../icons';
import { useState } from 'react';
import { saveRisk } from '../api';
import { PageHead } from '../components/cards';
import { NumberField } from '../components/arc/number-field/number-field';
import Toast from '../components/arc/toast/toast';
import Alert from '../components/arc/alert/alert';
import { Combobox } from '../components/arc/combobox/combobox';
import { Select } from '../components/arc/select/select';
import type { RiskRule, RiskSetting, Segment, StateView } from '../types';

const SEGMENTS: Segment[] = ['EQ', 'FUT', 'OPT', 'CUR', 'COM'];
const ALL = '__all__';
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
  const [saved, setSaved] = useState(false);

  const patch = (i: number, fn: (r: RiskRule) => RiskRule) => {
    setMsg(null);
    setRules((rs) => rs.map((r, j) => (j === i ? fn(r) : r)));
  };

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
      setMsg(null);
      setSaved(true);
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
                  <Combobox
                    label="Client"
                    placeholder="All clients"
                    emptyMessage="No client matches"
                    value={r.client_id ?? ALL}
                    onValueChange={(v) => patch(i, (x) => ({ ...x, client_id: !v || v === ALL ? null : v }))}
                    options={[{ value: ALL, label: 'All clients' }, ...view.overview.map((c) => ({ value: c.client_id, label: c.name, keywords: [c.group] }))]}
                  />
                  <Select
                    label="Segment"
                    value={r.segment ?? ALL}
                    onValueChange={(v) => patch(i, (x) => ({ ...x, segment: (v === ALL ? null : v) as Segment | null }))}
                    options={[{ value: ALL, label: 'All segments' }, ...SEGMENTS.map((segment) => ({ value: segment, label: segment }))]}
                  />
                  <Select
                    label="Check loss by"
                    value={r.setting.mode}
                    onValueChange={(v) => patch(i, (x) => ({ ...x, setting: { ...x.setting, mode: v as RiskSetting['mode'] } }))}
                    options={[{ value: 'PCT', label: '% of entry price' }, { value: 'RUPEE', label: 'Rupees per leg' }]}
                  />
                  <NumberField
                    size="sm"
                    label={percent ? 'Maximum loss (%)' : 'Maximum loss (₹)'}
                    value={Number.isNaN(r.setting[limit]) ? DEFAULT[limit] : r.setting[limit]}
                    min={0}
                    max={percent ? 1000 : 100000000}
                    step={percent ? 0.5 : 100}
                    suffix={percent ? '%' : undefined}
                    prefix={percent ? undefined : '₹'}
                    onValueChange={(v) => patch(i, (x) => ({ ...x, setting: { ...x.setting, [limit]: v } }))}
                  />
                  <NumberField
                    size="sm"
                    label="Grace (seconds)"
                    value={Number.isNaN(r.setting.grace_sec) ? DEFAULT.grace_sec : r.setting.grace_sec}
                    min={0}
                    max={3600}
                    step={1}
                    largeStep={15}
                    suffix=" s"
                    onValueChange={(v) => patch(i, (x) => ({ ...x, setting: { ...x.setting, grace_sec: Math.round(v) } }))}
                  />
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
          {msg && <Alert tone={msg.kind === 'success' ? 'success' : 'danger'} title={msg.text} />}
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
      <div className="toast-dock">
        <Toast title="Risk limits saved" description="Every leg was re-checked against the new limits." open={saved} onOpenChange={setSaved} />
      </div>
    </>
  );
}

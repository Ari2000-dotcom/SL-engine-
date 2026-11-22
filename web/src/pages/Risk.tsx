import { Button, InlineNotification, Select, SelectItem, TextInput } from '@carbon/react';
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
      <PageHead title="Risk settings" count={`${rules.length} ${rules.length === 1 ? 'rule' : 'rules'}`}>
        Decides when a stop-loss counts as <b className="t-amber">too far</b> from the entry price. A more specific rule wins: client and segment, then client, then segment, then the default.
      </PageHead>

      {msg && <InlineNotification kind={msg.kind} title={msg.kind === 'error' ? 'Check the rules' : 'Done'} subtitle={msg.text} lowContrast onCloseButtonClick={() => setMsg(null)} />}

      <div className="tray"><div className="core">
        <div className="core__head">
          <h2>
            Loss limits <span className="count">{rules.length}</span>
          </h2>
          <span>Grace is how long a new position may go without a stop before it is flagged</span>
        </div>
        <div className="core__body">
        {rules.map((r, i) => (
          <div key={i} className="risk-grid">
            <Select id={`rc-${i}`} labelText="Client" value={r.client_id ?? ''} onChange={(e: React.ChangeEvent<HTMLSelectElement>) => patch(i, (x) => ({ ...x, client_id: e.target.value || null }))}>
              <SelectItem value="" text="All clients" />
              {view.overview.map((c) => (
                <SelectItem key={c.client_id} value={c.client_id} text={c.name} />
              ))}
            </Select>
            <Select id={`rs-${i}`} labelText="Segment" value={r.segment ?? ''} onChange={(e: React.ChangeEvent<HTMLSelectElement>) => patch(i, (x) => ({ ...x, segment: (e.target.value || null) as Segment | null }))}>
              <SelectItem value="" text="All segments" />
              {SEGMENTS.map((s) => (
                <SelectItem key={s} value={s} text={s} />
              ))}
            </Select>
            <Select id={`rm-${i}`} labelText="Check loss by" value={r.setting.mode} onChange={(e: React.ChangeEvent<HTMLSelectElement>) => patch(i, (x) => ({ ...x, setting: { ...x.setting, mode: e.target.value as RiskSetting['mode'] } }))}>
              <SelectItem value="PCT" text="Percent of entry price" />
              <SelectItem value="RUPEE" text="Rupees per leg" />
            </Select>
            <TextInput
              id={`rp-${i}`}
              type="number"
              labelText="Maximum loss (%)"
              helperText="Used in percent mode"
              disabled={r.setting.mode !== 'PCT'}
              value={Number.isNaN(r.setting.max_pct) ? '' : r.setting.max_pct}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => patch(i, (x) => ({ ...x, setting: { ...x.setting, max_pct: num(e.target.value) } }))}
            />
            <TextInput
              id={`rr-${i}`}
              type="number"
              labelText="Maximum loss (rupees)"
              helperText="Used in rupee mode"
              disabled={r.setting.mode !== 'RUPEE'}
              value={Number.isNaN(r.setting.max_rupee) ? '' : r.setting.max_rupee}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => patch(i, (x) => ({ ...x, setting: { ...x.setting, max_rupee: num(e.target.value) } }))}
            />
            <TextInput
              id={`rg-${i}`}
              type="number"
              labelText="Grace (seconds)"
              helperText="Before a new position is flagged"
              value={Number.isNaN(r.setting.grace_sec) ? '' : r.setting.grace_sec}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) => patch(i, (x) => ({ ...x, setting: { ...x.setting, grace_sec: num(e.target.value) } }))}
            />
            <div>
              <Button
                kind="ghost"
                size="md"
                renderIcon={TrashCan}
                disabled={r.client_id === null && r.segment === null}
                onClick={() => {
                  setMsg(null);
                  setRules((rs) => rs.filter((_, j) => j !== i));
                }}
              >
                Remove
              </Button>
            </div>
          </div>
        ))}
        <div className="form-actions" style={{ marginTop: 0 }}>
          <Button kind="tertiary" renderIcon={Add} onClick={() => setRules((rs) => [...rs, { client_id: null, segment: 'OPT', setting: { ...DEFAULT } }])}>
            Add rule
          </Button>
          <Button onClick={save} disabled={busy}>
            {busy ? 'Saving' : 'Save changes'}
          </Button>
        </div>
      </div></div></div>
    </>
  );
}

// Mirrors the JSON the Rust server sends. Keep in sync with crates/core and crates/server.

export type Role = 'ADMIN' | 'CLIENT';
export type Broker = 'KOTAK' | 'ZERODHA' | 'ANGEL';
export type Severity = 'RED' | 'AMBER' | 'GREEN' | 'GREY';
export type Segment = 'EQ' | 'FUT' | 'OPT' | 'CUR' | 'COM';
export type RiskMode = 'PCT' | 'RUPEE';

export type LegStatus =
  | 'GRACE'
  | 'PROTECTED'
  | 'NO_SL'
  | 'SL_REJECTED'
  | 'SL_CANCELLED'
  | 'PARTIAL_SL'
  | 'OVER_COVERED'
  | 'WRONG_SIDE'
  | 'TOO_FAR'
  | 'UNVERIFIED'
  | 'STALE_DATA';

export interface Leg {
  leg_key: string;
  account_id: string;
  client_id: string;
  broker: Broker;
  exchange: string;
  segment: Segment;
  symbol: string;
  product: string;
  net_qty: number;
  avg_price: number;
  ltp: number | null;
  sl_order_ids: string[];
  sl_qty: number;
  sl_trigger: number | null;
  risk_mode: RiskMode;
  limit_value: number;
  dist_pct: number | null;
  risk_rupee: number | null;
  status: LegStatus;
  severity: Severity;
  detail: string | null;
  opened_ts: string | null;
  evaluated_at: string;
}

export interface Incident {
  id: number;
  leg_key: string;
  account_id: string;
  client_id: string;
  symbol: string;
  first_status: LegStatus;
  worst_status: LegStatus;
  current_status: LegStatus;
  detail: string | null;
  opened_at: string;
  acknowledged_at: string | null;
  acknowledged_by: string | null;
  resolved_at: string | null;
  resolution: 'SL_PLACED' | 'POSITION_CLOSED' | null;
  history: [string, LegStatus][];
  unprotected_secs: number;
}

export interface Account {
  account_id: string;
  client_id: string;
  broker: Broker;
  label: string;
  status: 'CONNECTING' | 'OK' | 'FAILED';
  last_ok: string | null;
  last_error: string | null;
  latency_ms: number | null;
  polls: number;
}

export interface ClientOverview {
  client_id: string;
  name: string;
  group: string;
  red: number;
  amber: number;
  green: number;
  grey: number;
  open_incidents: number;
  accounts_failed: number;
  accounts_total: number;
}

export interface RiskSetting {
  mode: RiskMode;
  max_pct: number;
  max_rupee: number;
  grace_sec: number;
}

export interface RiskRule {
  client_id: string | null;
  segment: Segment | null;
  setting: RiskSetting;
}

export interface RiskBook {
  rules: RiskRule[];
}

export interface Order {
  broker_order_id: string;
  exchange: string;
  symbol: string;
  side: 'BUY' | 'SELL';
  kind: string;
  product: string;
  qty: number;
  filled_qty: number;
  pending_qty: number;
  price: number;
  trigger_price: number | null;
  state: string;
  status_raw: string;
  reject_reason: string | null;
  order_ts: string;
  updated_ts: string;
}

export interface Trade {
  trade_id: string;
  broker_order_id: string;
  exchange: string;
  symbol: string;
  side: 'BUY' | 'SELL';
  product: string;
  qty: number;
  price: number;
  fill_ts: string;
}

export interface BookView {
  account_id: string;
  legs: Leg[];
  orders: Order[];
  trades: Trade[];
}

export interface LegTotals {
  total: number;
  red: number;
  amber: number;
  green: number;
  grey: number;
}

export interface LegPage {
  total: number;
  offset: number;
  legs: Leg[];
}

export interface StateView {
  now: string;
  role: Role;
  client_id: string | null;
  overview: ClientOverview[];
  /** Only legs that need attention. Protected legs are in `totals` and fetched on demand. */
  legs: Leg[];
  totals: LegTotals;
  incidents: Incident[];
  accounts: Account[];
  risk: RiskBook | null;
}

export interface Session {
  token: string;
  user: { username: string; role: Role; client_id: string | null };
}

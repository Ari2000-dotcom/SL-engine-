# SL Guardian

A read-only monitor for an RMS desk. It reads each client's order book, trade book and positions from
Kotak Neo, Zerodha Kite and Angel One, and flags any open leg that has no valid stop-loss. It never
places, modifies or cancels an order: the broker adapter interface has no method that could.

Design reference: [docs/sl-guardian-design.html](docs/sl-guardian-design.html).

## Status

| Part | State |
|---|---|
| Rule engine (`crates/core`) | Done, 28 tests |
| Broker response normalizers for Kite, Angel, Kotak (`crates/brokers`) | Done, tested against payload shapes from the broker docs |
| Mock broker with scripted scenarios | Test fixture only |
| Server: incidents, JWT login, roles, REST, websocket (`crates/server`) | Done |
| Dashboard (`web/`, React and Vite) | Done: overview, four-client account boards, legs, incidents, connections, risk settings, light and dark |
| Daily Parquet store with DuckDB (`crates/store`) | Library written, tests being re-run after compiling Parquet in. Not yet wired into the server |
| Live connections (`kite_live.rs`, `angel_live.rs`, `kotak_live.rs`): orders, trades, positions, stop orders, prices | Built. Each signs in by itself (see below). Tested against local stand-in servers that copy the brokers' replies; Zerodha, Angel and Kotak logins are not yet proven on a real funded account |
| Adding accounts at runtime, stored encrypted (`vault.rs`, `accounts.rs`) | Done: admin form on the Connections page, one form for all three brokers |
| Telegram and mobile push | Later phase |

The server has no demo data. The administrator adds clients and broker accounts from the Connections page; they are saved in an AES-256-GCM encrypted file under `SLG_DATA_DIR` and survive restarts. The server needs `SLG_ADMIN_PASSWORD`, and client logins for live clients are not built yet (only the admin can sign in).

### Automatic sign-in

- **Angel One**: client code, MPIN and the TOTP secret. The desk makes the 6-digit code itself, signs in, and signs in again when the session ends at midnight.
- **Kotak Neo**: the app's access token, registered mobile, client code (UCC), MPIN and the TOTP secret. Two-step sign-in, repeated when the session ends.
- **Zerodha Kite**: Kite has no official unattended login, so this copies QuantDesk's method: the web login with user id and password, then the TOTP code, then the normal token exchange with the API secret. It needs a one-time manual "Authorize" click in a browser the first time an account connects to your Kite app. Zerodha's terms on automated login are a grey area; confirm before relying on it. Without the user id, password and TOTP secret you can still paste a request or access token each morning.

Storing a TOTP secret and MPIN means the server holds everything needed to sign in as the client. The adapters only read, but protect `SLG_MASTER_KEY` and the data folder accordingly.

## Run it

Needs Rust and Node 20 or newer.

```powershell
cd web
npm install
npm run build
cd ..
$env:SLG_ADMIN_PASSWORD = "choose-a-password"
cargo run -p sl-server
```

Open http://127.0.0.1:8080 and sign in as `admin`. The desk starts empty. Add the first client and broker
account on the Connections page. The server only binds to a loopback address unless `SLG_ALLOW_REMOTE=1`.

For frontend work with hot reload, run `cargo run -p sl-server` with `SLG_BIND=127.0.0.1:8787`, then
`npm run dev` in `web/` (it proxies `/api` and `/ws` to port 8787).

### Settings (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `SLG_DATA_DIR` | `data` | Where the encrypted account file is kept |
| `SLG_MASTER_KEY` | a generated `master.key` in the data folder | Encrypts the account file. Set it in production |
| `SLG_BIND` | `127.0.0.1:8080` | Listen address |
| `SLG_POLL_SECS` | `5` | Full poll interval per account |
| `SLG_ADMIN_PASSWORD` | required | Administrator password |
| `SLG_ALLOW_REMOTE` | unset | Set to `1` to listen on a non-loopback address (the server has no TLS; put it behind HTTPS) |
| `SLG_JWT_SECRET` | random per start | Signing key for sessions |
| `SLG_WEB_DIR` | `web/dist` | Built frontend |

## Tests

```powershell
cargo test --workspace
```

## Leg rules

A leg is open while its net quantity is not zero. A protecting order is an SL, SL-M or GTT stop on the
same exchange, symbol and product, on the exit side, in a live state. Statuses:

- Red: no stop, rejected, cancelled, trigger on the wrong side of the last price.
- Amber: stop covers part of the position, covers more than the position, loss at the stop is above the limit (percent of entry or rupees, chosen by the admin), or a check could not run.
- Green: protected.
- Grey: just opened (grace period) or broker data is stale. Stale data is never shown as green.

## Known gaps to confirm against live accounts

- Kotak Neo: QuantDesk's working adapter reads the order-book trigger price from `trgPrc`, which is the first key we try (see `crates/brokers/src/kotak.rs`). Still confirm on a live account.
- Angel One: stop orders placed as GTT rules are not read yet (only the order book), so a GTT-only stop would show as missing.
- Kotak Neo: timestamp format of `ordDtTm` and `exTm`, and whether quantities are units or lots.
- Kite: multi-client use normally needs Zerodha's business Kite Connect app, and automated login is a grey area under broker terms. Confirm with each broker.

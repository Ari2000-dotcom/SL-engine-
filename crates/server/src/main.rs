mod accounts;
mod api;
mod auth;
mod incidents;
mod monitor;
mod state;
mod vault;

use anyhow::{bail, Result};
use auth::{hash_password, random_secret, AuthState, Role, User};
use state::{AppState, ClientInfo};
use std::sync::Arc;
use std::time::Duration;
use tower_http::services::{ServeDir, ServeFile};

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(env("RUST_LOG").unwrap_or_else(|| "info,tower_http=warn".into()))
        .init();

    let bind = env("SLG_BIND").unwrap_or_else(|| "127.0.0.1:8080".into());
    let poll = Duration::from_secs(env("SLG_POLL_SECS").and_then(|v| v.parse().ok()).unwrap_or(5).max(1));
    let web_dir = env("SLG_WEB_DIR").unwrap_or_else(|| "web/dist".into());

    // The demo mode with fake clients has been removed. Everything on screen is a real account.
    if let Some(mode) = env("SLG_MODE") {
        if mode != "live" {
            bail!("SLG_MODE={mode}: the demo mode was removed. The server only runs with real accounts, so leave SLG_MODE unset");
        }
    }

    // The server speaks plain HTTP, and sign-in passwords and broker secrets cross it, so it stays on
    // this machine unless the operator explicitly opts in (and puts TLS in front of it).
    let loopback = bind.parse::<std::net::SocketAddr>().map(|a| a.ip().is_loopback()).unwrap_or(false);
    if !loopback && env("SLG_ALLOW_REMOTE").as_deref() != Some("1") {
        bail!("SLG_BIND={bind} is not a loopback address. The server has no TLS of its own; put it behind an HTTPS proxy and set SLG_ALLOW_REMOTE=1");
    }

    let secret = env("SLG_JWT_SECRET").unwrap_or_else(|| {
        tracing::warn!("SLG_JWT_SECRET not set: using a random secret, sessions end on restart");
        random_secret()
    });
    let Some(admin_pw) = env("SLG_ADMIN_PASSWORD") else {
        bail!("Set SLG_ADMIN_PASSWORD, so the administrator password is the same on every start");
    };

    // Only the administrator can sign in until client logins are built. Clients and their broker
    // accounts are added from the Connections page and come back from the encrypted vault.
    let users = vec![User { username: "admin".into(), hash: hash_password(&admin_pw), role: Role::Admin, client_id: None }];

    let data_dir = std::path::PathBuf::from(env("SLG_DATA_DIR").unwrap_or_else(|| "data".into()));
    let (vault, saved) = vault::Vault::open(&data_dir)?;
    let clients: Vec<ClientInfo> = saved.clients.clone();
    let accounts = saved.accounts.iter().map(|a| a.info.clone()).collect();

    let state = Arc::new(AppState::new(AuthState::new(users, &secret), clients, accounts));
    monitor::spawn_ticker(state.clone());
    {
        let mut store = state.store.lock().await;
        store.poll = poll;
        for a in &saved.accounts {
            let live = accounts::build_live(a, Arc::new(monitor::Control::default()));
            monitor::spawn_account(state.clone(), a.info.clone(), live.adapter.clone(), poll, live.ctl.clone());
            store.live.insert(a.info.account_id.clone(), live);
        }
        store.data = saved;
        store.vault = Some(vault);
    }

    let index = format!("{web_dir}/index.html");
    let app = api::router(state).fallback_service(ServeDir::new(&web_dir).not_found_service(ServeFile::new(index)));

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("SL Guardian listening on http://{bind}");
    axum::serve(listener, app).await?;
    Ok(())
}

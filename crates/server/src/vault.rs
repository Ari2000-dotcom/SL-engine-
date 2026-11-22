//! Encrypted storage for broker accounts and their keys. One file, AES-256-GCM, written atomically.
//!
//! The key comes from `SLG_MASTER_KEY` (any string, hashed to 32 bytes). If that is not set, a random
//! key file is created next to the data, which protects against a copied data file but not against
//! someone who can read the whole folder; production should set the environment variable.

use crate::state::{AccountInfo, ClientInfo};
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"SLG1";

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredAccount {
    pub info: AccountInfo,
    pub api_key: String,
    pub api_secret: Option<String>,
    pub access_token: String,
    pub token_at: Option<DateTime<Utc>>,
    /// Broker-specific secrets: client code, MPIN, TOTP secret and so on. Encrypted with the rest.
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

// Secrets stay out of any debug print.
impl std::fmt::Debug for StoredAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredAccount").field("info", &self.info).finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Persisted {
    pub clients: Vec<ClientInfo>,
    pub accounts: Vec<StoredAccount>,
}

pub struct Vault {
    path: PathBuf,
    key: [u8; 32],
}

impl Vault {
    /// Open (or prepare) the vault in `dir`. Fails loudly if the key does not match an existing file,
    /// so a wrong key can never overwrite good data.
    pub fn open(dir: &Path) -> Result<(Vault, Persisted)> {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        let key = match std::env::var("SLG_MASTER_KEY").ok().filter(|k| !k.is_empty()) {
            Some(k) => Sha256::digest(k.as_bytes()).into(),
            None => {
                let kf = dir.join("master.key");
                if kf.exists() {
                    let raw = std::fs::read(&kf).context("read master.key")?;
                    raw.try_into().map_err(|_| anyhow!("master.key must be exactly 32 bytes"))?
                } else {
                    let k = Aes256Gcm::generate_key(OsRng);
                    std::fs::write(&kf, k).context("write master.key")?;
                    tracing::warn!("SLG_MASTER_KEY not set: created {} to encrypt broker keys. Set SLG_MASTER_KEY in production.", kf.display());
                    k.into()
                }
            }
        };
        let vault = Vault { path: dir.join("accounts.enc"), key };
        let data = if vault.path.exists() { vault.load()? } else { Persisted::default() };
        Ok((vault, data))
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key))
    }

    fn load(&self) -> Result<Persisted> {
        let raw = std::fs::read(&self.path).context("read accounts.enc")?;
        if raw.len() < MAGIC.len() + 12 || &raw[..4] != MAGIC {
            bail!("accounts.enc is not a valid vault file");
        }
        let nonce = Nonce::from_slice(&raw[4..16]);
        let plain = self
            .cipher()
            .decrypt(nonce, &raw[16..])
            .map_err(|_| anyhow!("accounts.enc could not be decrypted: wrong SLG_MASTER_KEY or master.key"))?;
        Ok(serde_json::from_slice(&plain).context("accounts.enc contents")?)
    }

    /// Write the whole state to a temp file, then replace the real one, so a crash never leaves half a file.
    pub fn save(&self, data: &Persisted) -> Result<()> {
        let plain = serde_json::to_vec(data)?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let cipher = self.cipher().encrypt(&nonce, plain.as_ref()).map_err(|_| anyhow!("encrypt failed"))?;
        let mut out = Vec::with_capacity(16 + cipher.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&cipher);
        let tmp = self.path.with_extension("enc.tmp");
        std::fs::write(&tmp, out).context("write temp vault")?;
        std::fs::rename(&tmp, &self.path).context("replace vault")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sl_core::Broker;

    fn sample() -> Persisted {
        Persisted {
            clients: vec![ClientInfo { id: "c1".into(), name: "Test Client".into(), group: "Desk".into() }],
            accounts: vec![StoredAccount {
                info: AccountInfo { account_id: "c1-ZERODHA".into(), client_id: "c1".into(), broker: Broker::Zerodha, label: "Kite".into() },
                api_key: "key123".into(),
                api_secret: Some("secret456".into()),
                access_token: "tok789".into(),
                token_at: None,
                extra: [("pin".to_string(), "pin9876".to_string())].into(),
            }],
        }
    }

    #[test]
    fn round_trip_and_not_plain_on_disk() {
        let dir = std::env::temp_dir().join(format!("slg-vault-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (v, d) = Vault::open(&dir).unwrap();
        assert!(d.accounts.is_empty());
        v.save(&sample()).unwrap();
        let raw = std::fs::read(dir.join("accounts.enc")).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("secret456") && !text.contains("tok789") && !text.contains("key123") && !text.contains("pin9876"));
        let (_, back) = Vault::open(&dir).unwrap();
        assert_eq!(back.accounts[0].api_secret.as_deref(), Some("secret456"));
        assert_eq!(back.accounts[0].extra.get("pin").map(String::as_str), Some("pin9876"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_key_refuses_to_open() {
        let dir = std::env::temp_dir().join(format!("slg-vault-wrong-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (v, _) = Vault::open(&dir).unwrap();
        v.save(&sample()).unwrap();
        std::fs::write(dir.join("master.key"), [7u8; 32]).unwrap();
        assert!(Vault::open(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

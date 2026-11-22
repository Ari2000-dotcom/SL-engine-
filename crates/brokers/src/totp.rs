//! Time-based one-time passwords (RFC 6238: HMAC-SHA1, 6 digits, 30 second steps), the same code an
//! authenticator app shows. Angel One and Kotak Neo both ask for one at login.

use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use sha1::Sha1;

/// Authenticator apps show secrets in groups ("ABCD EFGH"), sometimes in lower case. Normalise to plain
/// upper-case base32, or return None if it is not a valid secret.
pub fn normalize_secret(raw: &str) -> Option<String> {
    let s: String = raw.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '=').collect::<String>().to_uppercase();
    if s.len() < 16 || BASE32_NOPAD.decode(s.as_bytes()).is_err() {
        return None;
    }
    Some(s)
}

/// The 6-digit code for `secret` at unix time `unix_secs`.
pub fn code_at(secret: &str, unix_secs: u64) -> Option<String> {
    let key = BASE32_NOPAD.decode(normalize_secret(secret)?.as_bytes()).ok()?;
    let counter = (unix_secs / 30).to_be_bytes();
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(&key).ok()?;
    mac.update(&counter);
    let h = mac.finalize().into_bytes();
    let off = (h[19] & 0x0f) as usize;
    let bin = ((u32::from(h[off]) & 0x7f) << 24) | (u32::from(h[off + 1]) << 16) | (u32::from(h[off + 2]) << 8) | u32::from(h[off + 3]);
    Some(format!("{:06}", bin % 1_000_000))
}

/// A code that will still be valid when the broker checks it. A code minted in the last 3 seconds of
/// its window has often rolled over by the time it arrives, and that failure looks exactly like a wrong
/// secret, so wait for the next window first.
pub async fn fresh_code(secret: &str) -> Result<String, String> {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let into = ms % 30_000;
    if 30_000 - into <= 3_000 {
        tokio::time::sleep(std::time::Duration::from_millis(30_000 - into + 250)).await;
    }
    code_now(secret).ok_or_else(|| "The TOTP secret is not valid. Use the text key shown under the QR code when TOTP was set up, not a 6-digit code.".to_string())
}

pub fn code_now(secret: &str) -> Option<String> {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    code_at(secret, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 appendix B: secret "12345678901234567890" (ASCII), SHA-1, 8 digits there; the last
    // six digits are the 6-digit code.
    const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn matches_the_rfc_6238_vectors() {
        assert_eq!(code_at(SECRET, 59).unwrap(), "287082");
        assert_eq!(code_at(SECRET, 1111111109).unwrap(), "081804");
        assert_eq!(code_at(SECRET, 1234567890).unwrap(), "005924");
        assert_eq!(code_at(SECRET, 2000000000).unwrap(), "279037");
    }

    #[test]
    fn tolerates_spaces_and_lower_case() {
        assert_eq!(code_at("gezd gnbv gy3t qojq gezd gnbv gy3t qojq", 59).unwrap(), "287082");
    }

    #[test]
    fn rejects_garbage() {
        assert!(normalize_secret("short").is_none());
        assert!(normalize_secret("this is not base32 !!!!!!!!!").is_none());
    }
}

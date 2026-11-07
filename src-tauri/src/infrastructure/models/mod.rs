//! Model provider adapters and the factory that builds them from stored records.
pub mod anthropic;
pub mod chatgpt;
pub mod copilot;
pub mod http;
pub mod ollama;
pub mod openai;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::ports::{ModelProvider, ProviderFactory, ProviderKind, ProviderRecord, SecretStore};
use crate::infrastructure::auth::AccountAuth;

/// Renew access tokens this long before they expire.
const RENEW_BEFORE_MS: i64 = 15 * 60 * 1000;

pub struct DefaultProviderFactory {
    secrets: Arc<dyn SecretStore>,
    auth: Arc<AccountAuth>,
    copilot_home: PathBuf,
}

impl DefaultProviderFactory {
    pub fn new(secrets: Arc<dyn SecretStore>, auth: Arc<AccountAuth>, copilot_home: PathBuf) -> Self {
        Self { secrets, auth, copilot_home }
    }

    fn secret(&self, record: &ProviderRecord) -> AppResult<Value> {
        let key = record.secret_ref.as_deref().ok_or_else(|| AppError::new(ErrorCode::Unsupported, "no credential stored"))?;
        let raw = self.secrets.get(key)?.ok_or_else(|| AppError::new(ErrorCode::Unsupported, "signed_out: credential missing"))?;
        serde_json::from_str(&raw).map_err(|_| AppError::internal("credential unreadable"))
    }

    /// Returns a valid access token, renewing (and persisting) it when close to expiry.
    fn fresh_token(&self, record: &ProviderRecord, renew: impl Fn(&Value) -> AppResult<Value>) -> AppResult<String> {
        let mut secret = self.secret(record)?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let expires = secret.get("expires_at").and_then(Value::as_i64);
        let has_refresh = secret.get("refresh_token").and_then(Value::as_str).is_some();
        if expires.is_some_and(|e| e - now < RENEW_BEFORE_MS) && has_refresh {
            secret = renew(&secret)?;
            self.secrets.set(record.secret_ref.as_deref().unwrap_or_default(), &secret.to_string())?;
        }
        secret.get("access_token").and_then(Value::as_str).map(str::to_string).ok_or_else(|| AppError::new(ErrorCode::Unsupported, "signed_out"))
    }
}

impl ProviderFactory for DefaultProviderFactory {
    fn open(&self, record: &ProviderRecord) -> AppResult<Arc<dyn ModelProvider>> {
        Ok(match record.kind {
            ProviderKind::Ollama => Arc::new(ollama::Ollama::new(&record.base_url)),
            ProviderKind::OpenaiCompatible => {
                // No stored credential: a keyless local gateway (accepted only on loopback).
                let key = match record.secret_ref {
                    Some(_) => self.secret(record)?.get("api_key").and_then(Value::as_str).unwrap_or_default().to_string(),
                    None => String::new(),
                };
                Arc::new(openai::OpenAiCompatible::new(&record.base_url, key))
            }
            ProviderKind::Anthropic => {
                let key = self.secret(record)?.get("api_key").and_then(Value::as_str).unwrap_or_default().to_string();
                Arc::new(anthropic::Anthropic::new(&record.base_url, key))
            }
            ProviderKind::Chatgpt => {
                let token = self.fresh_token(record, |s| self.auth.refresh_chatgpt(s))?;
                Arc::new(chatgpt::ChatGptPlan::new(token))
            }
            ProviderKind::GithubCopilot => {
                let token = self.fresh_token(record, |s| self.auth.refresh_github(s))?;
                Arc::new(copilot::Copilot::new(&record.id, token, self.copilot_home.join(&record.id)))
            }
        })
    }
}

/// Windows Credential Manager (via `keyring`). Values never touch SQLite or logs.
pub struct OsSecretStore {
    inner: ChunkedSecrets<KeyringRaw>,
}

impl OsSecretStore {
    pub fn new(service: &str) -> Self {
        Self { inner: ChunkedSecrets { raw: KeyringRaw { service: service.into() } } }
    }
}

impl SecretStore for OsSecretStore {
    fn set(&self, key: &str, value: &str) -> AppResult<()> {
        self.inner.set(key, value)
    }
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        self.inner.get(key)
    }
    fn delete(&self, key: &str) -> AppResult<()> {
        self.inner.delete(key)
    }
}

/// One credential entry, as the OS stores it.
pub(crate) trait RawSecrets {
    fn set(&self, key: &str, value: &str) -> AppResult<()>;
    fn get(&self, key: &str) -> AppResult<Option<String>>;
    fn delete(&self, key: &str) -> AppResult<()>;
}

struct KeyringRaw {
    service: String,
}

impl KeyringRaw {
    fn entry(&self, key: &str) -> AppResult<keyring::Entry> {
        keyring::Entry::new(&self.service, key).map_err(|e| AppError::internal(format!("credential store: {e}")))
    }
}

impl RawSecrets for KeyringRaw {
    fn set(&self, key: &str, value: &str) -> AppResult<()> {
        self.entry(key)?.set_password(value).map_err(|e| AppError::internal(format!("credential store: {e}")))
    }
    fn get(&self, key: &str) -> AppResult<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::internal(format!("credential store: {e}"))),
        }
    }
    fn delete(&self, key: &str) -> AppResult<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::internal(format!("credential store: {e}"))),
        }
    }
}

/// Windows limits one credential to 2560 bytes (stored as UTF-16, so ~1280 characters). Sign-in tokens
/// (ChatGPT's ID, access and refresh tokens) are larger, so long values are split across numbered entries
/// (`key#1`, `key#2`, …) and the main entry holds only a manifest.
pub(crate) struct ChunkedSecrets<R: RawSecrets> {
    pub(crate) raw: R,
}

/// UTF-16 code units per entry: 2400 bytes, under the 2560-byte limit.
const CHUNK_UNITS: usize = 1200;
const MANIFEST: &str = "\u{1}papryvio-chunks:";

fn split_utf16(value: &str, limit: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut units = 0;
    for ch in value.chars() {
        let n = ch.len_utf16();
        if units + n > limit {
            parts.push(std::mem::take(&mut cur));
            units = 0;
        }
        cur.push(ch);
        units += n;
    }
    if !cur.is_empty() || parts.is_empty() {
        parts.push(cur);
    }
    parts
}

fn manifest_count(v: &str) -> Option<usize> {
    v.strip_prefix(MANIFEST).and_then(|n| n.parse().ok())
}

impl<R: RawSecrets> ChunkedSecrets<R> {
    fn part_key(key: &str, i: usize) -> String {
        format!("{key}#{i}")
    }

    fn delete_parts(&self, key: &str, from: usize, to: usize) -> AppResult<()> {
        for i in from..=to {
            self.raw.delete(&Self::part_key(key, i))?;
        }
        Ok(())
    }

    pub(crate) fn set(&self, key: &str, value: &str) -> AppResult<()> {
        let old = self.raw.get(key)?.as_deref().and_then(manifest_count).unwrap_or(0);
        let parts = if value.encode_utf16().count() <= CHUNK_UNITS { Vec::new() } else { split_utf16(value, CHUNK_UNITS) };
        if parts.is_empty() {
            self.raw.set(key, value)?;
        } else {
            // Parts first, manifest last: a reader never sees a manifest without its parts.
            for (i, part) in parts.iter().enumerate() {
                self.raw.set(&Self::part_key(key, i + 1), part)?;
            }
            self.raw.set(key, &format!("{MANIFEST}{}", parts.len()))?;
        }
        if old > parts.len() {
            self.delete_parts(key, parts.len() + 1, old)?;
        }
        Ok(())
    }

    pub(crate) fn get(&self, key: &str) -> AppResult<Option<String>> {
        let Some(head) = self.raw.get(key)? else { return Ok(None) };
        let Some(n) = manifest_count(&head) else { return Ok(Some(head)) };
        let mut out = String::new();
        for i in 1..=n {
            match self.raw.get(&Self::part_key(key, i))? {
                Some(p) => out.push_str(&p),
                // An incomplete credential is treated as missing: the user signs in again.
                None => return Ok(None),
            }
        }
        Ok(Some(out))
    }

    pub(crate) fn delete(&self, key: &str) -> AppResult<()> {
        if let Some(n) = self.raw.get(key)?.as_deref().and_then(manifest_count) {
            self.delete_parts(key, 1, n)?;
        }
        self.raw.delete(key)
    }
}

#[cfg(test)]
mod secret_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Enforces the Windows limit like the real store does.
    #[derive(Default)]
    struct Limited(Mutex<HashMap<String, String>>);
    impl RawSecrets for Limited {
        fn set(&self, key: &str, value: &str) -> AppResult<()> {
            if value.encode_utf16().count() * 2 > 2560 {
                return Err(AppError::internal("longer than platform limit"));
            }
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn get(&self, key: &str) -> AppResult<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn delete(&self, key: &str) -> AppResult<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[test]
    fn long_tokens_are_split_and_rejoined() {
        let s = ChunkedSecrets { raw: Limited::default() };
        let token = format!("{{\"access_token\":\"{}\",\"note\":\"ملاحظة 😀\"}}", "x".repeat(7000));
        s.set("provider:a", &token).unwrap();
        assert_eq!(s.get("provider:a").unwrap().as_deref(), Some(token.as_str()));
        assert!(s.raw.0.lock().unwrap().len() > 2);

        // Shorter value later: stale parts are removed.
        s.set("provider:a", "short").unwrap();
        assert_eq!(s.get("provider:a").unwrap().as_deref(), Some("short"));
        assert_eq!(s.raw.0.lock().unwrap().len(), 1);

        s.set("provider:a", &token).unwrap();
        s.delete("provider:a").unwrap();
        assert!(s.raw.0.lock().unwrap().is_empty());
        assert_eq!(s.get("provider:a").unwrap(), None);
    }

    #[test]
    fn missing_part_reads_as_signed_out() {
        let s = ChunkedSecrets { raw: Limited::default() };
        s.set("k", &"y".repeat(3000)).unwrap();
        s.raw.delete("k#2").unwrap();
        assert_eq!(s.get("k").unwrap(), None);
    }

    /// Writes to the real Windows Credential Manager under a test service, then removes it.
    #[test]
    #[ignore]
    fn os_store_round_trip() {
        let s = OsSecretStore::new("com.papryvio.desktop.test");
        let token = "t".repeat(9000);
        s.set("provider:roundtrip", &token).unwrap();
        assert_eq!(s.get("provider:roundtrip").unwrap().as_deref(), Some(token.as_str()));
        s.delete("provider:roundtrip").unwrap();
        assert_eq!(s.get("provider:roundtrip").unwrap(), None);
        assert_eq!(s.inner.raw.get("provider:roundtrip#1").unwrap(), None);
    }

    #[test]
    fn split_respects_utf16_units() {
        let parts = split_utf16(&"😀".repeat(1000), CHUNK_UNITS);
        assert!(parts.iter().all(|p| p.encode_utf16().count() <= CHUNK_UNITS));
        assert_eq!(parts.concat(), "😀".repeat(1000));
    }
}

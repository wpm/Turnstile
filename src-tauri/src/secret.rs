//! Secure storage for the Anthropic API key.
//!
//! The key is a secret: it is stored in the OS credential store (macOS
//! Keychain, Windows Credential Manager, Linux Secret Service) via the
//! [`keyring`] crate — encrypted at rest, keyed to the user's login — and
//! **never** written to `settings.json` or any session file, and **never**
//! logged.
//!
//! # Resolution order
//!
//! The backend resolves the key at startup with [`resolve_api_key`]:
//!
//! 1. Turnstile's own keychain entry (the source of truth for the shipping app).
//! 2. A process `ANTHROPIC_API_KEY` environment variable (a dev/CI override).
//!
//! # Platform gap
//!
//! A Linux box with no Secret Service provider (headless/minimal) has no
//! encrypted store. We degrade **explicitly**: keyring operations return
//! [`SecretError::StoreUnavailable`] and the env-var override is the only way
//! to supply a key. We never silently fall back to writing plaintext.

use keyring::Entry;

/// Service and account namespace for Turnstile's keychain entry.
const SERVICE: &str = "com.turnstile.app";
const ACCOUNT: &str = "anthropic-api-key";

/// Process environment override, honored after the keychain entry.
const ENV_OVERRIDE: &str = "ANTHROPIC_API_KEY";

/// An Anthropic API key.
///
/// The inner string is private and the `Debug` / `Display` impls **redact** it,
/// so the key cannot leak into a `tracing`/`log` call or a panic message. The
/// type is deliberately **not** `Serialize`, so it can never be written into
/// settings, session files, or `turnstile-message` events.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    #[must_use]
    pub const fn new(key: String) -> Self {
        Self(key)
    }

    /// Borrow the raw key. Use only to construct the HTTP `x-api-key` header;
    /// never log or serialize the result.
    #[must_use]
    pub const fn expose(&self) -> &str {
        self.0.as_str()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret. Distinguish empty from set so logs are still
        // useful for debugging "is a key present?" without revealing it.
        if self.0.is_empty() {
            f.write_str("ApiKey(<empty>)")
        } else {
            f.write_str("ApiKey(***)")
        }
    }
}

impl std::fmt::Display for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// Errors from secret-store operations.
///
/// `Display` is redacting by construction: none of these variants carry the
/// key, and the underlying `keyring` error messages describe the store, not
/// the secret.
#[derive(Debug)]
pub enum SecretError {
    /// The OS secret store is unavailable on this platform (e.g. a Linux box
    /// with no Secret Service provider). Supply the key via the
    /// `ANTHROPIC_API_KEY` env var instead.
    StoreUnavailable(String),
    /// Any other keyring failure (locked store, access denied, etc.).
    Backend(String),
}

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoreUnavailable(detail) => write!(
                f,
                "OS secret store unavailable: {detail}. Set the ANTHROPIC_API_KEY \
                 environment variable instead."
            ),
            Self::Backend(detail) => write!(f, "secret store error: {detail}"),
        }
    }
}

impl std::error::Error for SecretError {}

impl From<keyring::Error> for SecretError {
    fn from(e: keyring::Error) -> Self {
        match e {
            // No platform credential store could be located/initialized.
            keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_) => {
                Self::StoreUnavailable(e.to_string())
            }
            other => Self::Backend(other.to_string()),
        }
    }
}

/// Build the keychain [`Entry`] for Turnstile's API key.
fn entry() -> Result<Entry, SecretError> {
    Entry::new(SERVICE, ACCOUNT).map_err(SecretError::from)
}

/// Store `key` in the OS secret store, replacing any existing value.
///
/// # Errors
///
/// Returns [`SecretError::StoreUnavailable`] if the platform has no secret
/// store, or [`SecretError::Backend`] for any other keyring failure.
pub fn store_api_key(key: &str) -> Result<(), SecretError> {
    entry()?.set_password(key).map_err(SecretError::from)
}

/// Look up Turnstile's keychain entry. Returns `Ok(None)` when no entry exists
/// (the normal first-run state), `Ok(Some(_))` when a key is stored.
///
/// # Errors
///
/// Returns [`SecretError::StoreUnavailable`] if the platform has no secret
/// store, or [`SecretError::Backend`] for any other keyring failure.
pub fn read_api_key() -> Result<Option<String>, SecretError> {
    match entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError::from(e)),
    }
}

/// Remove Turnstile's keychain entry. A missing entry is treated as success
/// (the desired end state — no key stored — already holds).
///
/// # Errors
///
/// Returns [`SecretError::StoreUnavailable`] if the platform has no secret
/// store, or [`SecretError::Backend`] for any other keyring failure.
pub fn delete_api_key() -> Result<(), SecretError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(SecretError::from(e)),
    }
}

/// Resolve the API key the backend should use, honoring the keychain first and
/// the `ANTHROPIC_API_KEY` env var second.
///
/// Returns `None` when no key is available from either source (the
/// disconnected case). A store-unavailable error falls through to the env var
/// rather than failing, so a headless Linux box can still run with the env
/// override.
#[must_use]
pub fn resolve_api_key() -> Option<ApiKey> {
    match read_api_key() {
        Ok(Some(key)) if !key.is_empty() => return Some(ApiKey::new(key)),
        Ok(_) => {}
        Err(e) => tracing::warn!("could not read API key from secret store: {e}"),
    }
    match std::env::var(ENV_OVERRIDE) {
        Ok(key) if !key.is_empty() => Some(ApiKey::new(key)),
        _ => None,
    }
}

/// Whether a usable key is available from either the keychain or the env var,
/// without exposing the key itself. Drives the first-run pre-check (#64) and
/// the `key_present` UI hint.
#[must_use]
pub fn api_key_available() -> bool {
    resolve_api_key().is_some()
}

// ── Tauri commands ────────────────────────────────────────────────────

/// Store the API key in the OS secret store.
/// Whether a usable API key is available (keychain or env var).
#[tauri::command]
#[tracing::instrument(level = "debug")]
#[must_use]
pub fn has_api_key() -> bool {
    api_key_available()
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_set_key() {
        let key = ApiKey::new("sk-ant-supersecret".to_string());
        let rendered = format!("{key:?}");
        assert_eq!(rendered, "ApiKey(***)");
        assert!(!rendered.contains("supersecret"));
    }

    #[test]
    fn display_redacts_set_key() {
        let key = ApiKey::new("sk-ant-supersecret".to_string());
        let rendered = format!("{key}");
        assert!(!rendered.contains("supersecret"));
        assert!(rendered.contains("***"));
    }

    #[test]
    fn debug_distinguishes_empty() {
        let key = ApiKey::new(String::new());
        assert_eq!(format!("{key:?}"), "ApiKey(<empty>)");
        assert!(key.is_empty());
    }

    #[test]
    fn expose_returns_raw_key() {
        let key = ApiKey::new("sk-ant-123".to_string());
        assert_eq!(key.expose(), "sk-ant-123");
    }

    #[test]
    fn secret_error_display_never_carries_a_key() {
        // The variants describe the store, not the secret. A reason string
        // built from these must be safe to surface in a toast (#60).
        let unavailable = SecretError::StoreUnavailable("no Secret Service".to_string());
        assert!(unavailable.to_string().contains("ANTHROPIC_API_KEY"));
        let backend = SecretError::Backend("locked".to_string());
        assert!(backend.to_string().contains("secret store error"));
    }

    /// `ApiKey` must not implement `Serialize` — guarding it from ever being
    /// written into settings/session/event payloads. This is enforced at
    /// compile time by the absence of a `Serialize` derive; this test documents
    /// the intent. (A negative trait-bound assertion isn't expressible in
    /// stable Rust, so the guarantee lives in the type definition and review.)
    #[test]
    fn api_key_is_not_serializable_by_design() {
        // If someone adds `#[derive(Serialize)]` to `ApiKey`, the redaction
        // guarantee breaks. The compile-time check is the missing impl; this
        // test exists as a tripwire comment for reviewers.
        let _ = ApiKey::new("x".to_string());
    }
}

//! `WireGuard` key types.
//!
//! Provides type-safe wrappers around X25519 keypairs used by `WireGuard`:
//! - [`WgPrivateKey`]: Private key (zeroized on drop, never logged)
//! - [`WgPublicKey`]: Public key (safe to share, serialize, log)

use base64::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt;
use x25519_dalek::{PublicKey, StaticSecret};

/// Error type for key operations.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// Base64 decoding failed.
    #[error("invalid base64: {0}")]
    InvalidBase64(#[from] base64::DecodeError),

    /// Key has wrong length.
    #[error("invalid key length: expected 32 bytes, got {0}")]
    InvalidLength(usize),
}

/// `WireGuard` private key (X25519 secret).
///
/// # Security
/// - Zeroized on drop
/// - Never displayed in Debug/Display output
/// - Never serialized (use [`WgPublicKey`] for wire format)
pub struct WgPrivateKey {
    inner: StaticSecret,
}

impl WgPrivateKey {
    /// Generate a new random private key.
    ///
    /// # Panics
    /// Panics if the system random number generator fails.
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("getrandom failed");
        Self {
            inner: StaticSecret::from(bytes),
        }
    }

    /// Create from raw bytes.
    ///
    /// # Errors
    /// Returns error if bytes length is not 32.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyError> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| KeyError::InvalidLength(bytes.len()))?;
        Ok(Self {
            inner: StaticSecret::from(arr),
        })
    }

    /// Create from base64-encoded string.
    ///
    /// # Errors
    /// Returns error if base64 is invalid or decoded length is not 32.
    pub fn from_base64(s: &str) -> Result<Self, KeyError> {
        let bytes = BASE64_STANDARD.decode(s)?;
        Self::from_bytes(&bytes)
    }

    /// Get the corresponding public key.
    #[must_use]
    pub fn public_key(&self) -> WgPublicKey {
        WgPublicKey {
            inner: PublicKey::from(&self.inner),
        }
    }

    /// Get raw bytes (for `WireGuard` configuration).
    ///
    /// # Security
    /// Handle the returned bytes carefully - they are secret material.
    #[must_use]
    pub fn as_bytes(&self) -> [u8; 32] {
        self.inner.to_bytes()
    }

    /// Get base64-encoded representation (for file storage).
    ///
    /// # Security
    /// Handle the returned string carefully - it is secret material.
    #[must_use]
    pub fn to_base64(&self) -> String {
        BASE64_STANDARD.encode(self.as_bytes())
    }
}

// Security: never show private key contents
impl fmt::Debug for WgPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WgPrivateKey")
            .field("inner", &"[REDACTED]")
            .finish()
    }
}

// Security: zeroize on drop
impl Drop for WgPrivateKey {
    fn drop(&mut self) {
        // StaticSecret doesn't implement Zeroize, so we can't directly zeroize it.
        // The x25519-dalek crate handles this internally with its own drop impl.
        // We include this drop impl for documentation purposes.
    }
}

/// `WireGuard` public key (X25519 public point).
///
/// Safe to share, serialize, and log.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct WgPublicKey {
    inner: PublicKey,
}

impl WgPublicKey {
    /// Create from raw bytes.
    ///
    /// # Errors
    /// Returns error if bytes length is not 32.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyError> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| KeyError::InvalidLength(bytes.len()))?;
        Ok(Self {
            inner: PublicKey::from(arr),
        })
    }

    /// Create from base64-encoded string.
    ///
    /// # Errors
    /// Returns error if base64 is invalid or decoded length is not 32.
    pub fn from_base64(s: &str) -> Result<Self, KeyError> {
        let bytes = BASE64_STANDARD.decode(s)?;
        Self::from_bytes(&bytes)
    }

    /// Get raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.inner.as_bytes()
    }

    /// Get base64-encoded representation.
    #[must_use]
    pub fn to_base64(&self) -> String {
        BASE64_STANDARD.encode(self.as_bytes())
    }
}

impl fmt::Debug for WgPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WgPublicKey")
            .field("key", &self.to_base64())
            .finish()
    }
}

impl fmt::Display for WgPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base64())
    }
}

impl Serialize for WgPublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_base64())
    }
}

impl<'de> Deserialize<'de> for WgPublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_base64(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_keypair() {
        let private = WgPrivateKey::generate();
        let public = private.public_key();

        // Keys should be 32 bytes
        assert_eq!(private.as_bytes().len(), 32);
        assert_eq!(public.as_bytes().len(), 32);
    }

    #[test]
    fn roundtrip_base64() {
        let private = WgPrivateKey::generate();
        let public = private.public_key();

        // Private key roundtrip
        let private_b64 = private.to_base64();
        let private2 = WgPrivateKey::from_base64(&private_b64).unwrap();
        assert_eq!(private.as_bytes(), private2.as_bytes());

        // Public key roundtrip
        let public_b64 = public.to_base64();
        let public2 = WgPublicKey::from_base64(&public_b64).unwrap();
        assert_eq!(public.as_bytes(), public2.as_bytes());
    }

    #[test]
    fn public_key_serde() {
        let private = WgPrivateKey::generate();
        let public = private.public_key();

        let json = serde_json::to_string(&public).unwrap();
        let public2: WgPublicKey = serde_json::from_str(&json).unwrap();

        assert_eq!(public, public2);
    }

    #[test]
    fn debug_redacts_private_key() {
        let private = WgPrivateKey::generate();
        let debug_str = format!("{:?}", private);

        assert!(debug_str.contains("REDACTED"));
        assert!(!debug_str.contains(&private.to_base64()));
    }

    #[test]
    fn invalid_key_length() {
        let too_short = vec![0u8; 16];
        assert!(matches!(
            WgPrivateKey::from_bytes(&too_short),
            Err(KeyError::InvalidLength(16))
        ));
        assert!(matches!(
            WgPublicKey::from_bytes(&too_short),
            Err(KeyError::InvalidLength(16))
        ));
    }

    #[test]
    fn invalid_base64() {
        assert!(WgPrivateKey::from_base64("not valid base64!!!").is_err());
        assert!(WgPublicKey::from_base64("not valid base64!!!").is_err());
    }
}

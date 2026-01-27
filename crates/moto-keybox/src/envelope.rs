//! Envelope encryption for secrets at rest.
//!
//! This module implements a two-tier encryption scheme:
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │  Master Key (KEK)                               │
//! │  - Loaded from env/file at startup              │
//! │  - Never persisted in database                  │
//! │  - Future: HSM/KMS backend                      │
//! └─────────────────────────────────────────────────┘
//!            │
//!            │ encrypts
//!            ▼
//! ┌─────────────────────────────────────────────────┐
//! │  Data Encryption Keys (DEKs)                    │
//! │  - One per secret                               │
//! │  - Random AES-256 key                           │
//! │  - Stored encrypted in DB                       │
//! └─────────────────────────────────────────────────┘
//!            │
//!            │ encrypts
//!            ▼
//! ┌─────────────────────────────────────────────────┐
//! │  Secret Values                                  │
//! │  - Encrypted with DEK using AES-256-GCM         │
//! │  - Stored as ciphertext in DB                   │
//! └─────────────────────────────────────────────────┘
//! ```
//!
//! Database theft is useless without the KEK.

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Size of AES-256 key in bytes.
const KEY_SIZE: usize = 32;

/// Size of AES-GCM nonce in bytes (96 bits).
const NONCE_SIZE: usize = 12;

/// Master Key (KEK) used to encrypt/decrypt DEKs.
///
/// The KEK is loaded from environment or file at startup and never persisted
/// in the database. It wraps (encrypts) the DEKs which are stored encrypted.
#[derive(Clone)]
pub struct MasterKey {
    key: Key<Aes256Gcm>,
}

impl MasterKey {
    /// Creates a master key from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the key is not exactly 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != KEY_SIZE {
            return Err(Error::Crypto {
                message: format!("master key must be {KEY_SIZE} bytes, got {}", bytes.len()),
            });
        }
        let key = Key::<Aes256Gcm>::from_slice(bytes);
        Ok(Self { key: *key })
    }

    /// Creates a master key from base64-encoded string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not valid base64 or the decoded key
    /// is not exactly 32 bytes.
    pub fn from_base64(encoded: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|e| Error::Crypto {
            message: format!("invalid base64 in master key: {e}"),
        })?;
        Self::from_bytes(&bytes)
    }

    /// Loads a master key from a file.
    ///
    /// The file should contain the base64-encoded key, optionally with
    /// trailing whitespace.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or contains invalid data.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_base64(contents.trim())
    }

    /// Generates a new random master key.
    ///
    /// Uses cryptographically secure randomness.
    ///
    /// # Panics
    ///
    /// Panics if the system random number generator fails, which should not
    /// happen on any supported platform.
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_SIZE];
        getrandom::fill(&mut bytes).expect("getrandom failed");
        Self {
            key: *Key::<Aes256Gcm>::from_slice(&bytes),
        }
    }

    /// Encodes the master key as base64.
    ///
    /// Use this to save a newly generated key to a file.
    #[must_use]
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.key.as_slice())
    }

    /// Encrypts a DEK with this master key.
    ///
    /// Returns an `EncryptedDek` containing the encrypted key material and nonce.
    ///
    /// # Errors
    ///
    /// Returns an error if encryption fails.
    pub fn wrap_dek(&self, dek: &DataEncryptionKey) -> Result<EncryptedDek> {
        let cipher = Aes256Gcm::new(&self.key);
        let nonce_bytes = generate_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, dek.key.as_slice())
            .map_err(|e| Error::Crypto {
                message: format!("failed to wrap DEK: {e}"),
            })?;

        Ok(EncryptedDek {
            encrypted_key: ciphertext,
            nonce: nonce_bytes.to_vec(),
        })
    }

    /// Decrypts an encrypted DEK using this master key.
    ///
    /// # Errors
    ///
    /// Returns an error if decryption fails (wrong key or corrupted data).
    pub fn unwrap_dek(&self, encrypted: &EncryptedDek) -> Result<DataEncryptionKey> {
        let cipher = Aes256Gcm::new(&self.key);

        if encrypted.nonce.len() != NONCE_SIZE {
            return Err(Error::Crypto {
                message: format!(
                    "invalid nonce size: expected {NONCE_SIZE}, got {}",
                    encrypted.nonce.len()
                ),
            });
        }
        let nonce = Nonce::from_slice(&encrypted.nonce);

        let plaintext = cipher
            .decrypt(nonce, encrypted.encrypted_key.as_slice())
            .map_err(|_| Error::Crypto {
                message: "failed to unwrap DEK: decryption failed".to_string(),
            })?;

        DataEncryptionKey::from_bytes(&plaintext)
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterKey")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

/// Data Encryption Key (DEK) used to encrypt secret values.
///
/// Each secret has its own DEK. The DEK is stored encrypted (wrapped) by the
/// master key. This allows key rotation without re-encrypting all secrets.
#[derive(Clone)]
pub struct DataEncryptionKey {
    key: Key<Aes256Gcm>,
}

impl DataEncryptionKey {
    /// Creates a DEK from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the key is not exactly 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != KEY_SIZE {
            return Err(Error::Crypto {
                message: format!("DEK must be {KEY_SIZE} bytes, got {}", bytes.len()),
            });
        }
        let key = Key::<Aes256Gcm>::from_slice(bytes);
        Ok(Self { key: *key })
    }

    /// Generates a new random DEK.
    ///
    /// Uses cryptographically secure randomness.
    ///
    /// # Panics
    ///
    /// Panics if the system random number generator fails, which should not
    /// happen on any supported platform.
    #[must_use]
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_SIZE];
        getrandom::fill(&mut bytes).expect("getrandom failed");
        Self {
            key: *Key::<Aes256Gcm>::from_slice(&bytes),
        }
    }

    /// Encrypts a secret value with this DEK.
    ///
    /// Returns an `EncryptedSecret` containing the ciphertext and nonce.
    ///
    /// # Errors
    ///
    /// Returns an error if encryption fails.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedSecret> {
        let cipher = Aes256Gcm::new(&self.key);
        let nonce_bytes = generate_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| Error::Crypto {
                message: format!("failed to encrypt secret: {e}"),
            })?;

        Ok(EncryptedSecret {
            ciphertext,
            nonce: nonce_bytes.to_vec(),
        })
    }

    /// Decrypts an encrypted secret value.
    ///
    /// # Errors
    ///
    /// Returns an error if decryption fails (wrong key or corrupted data).
    pub fn decrypt(&self, encrypted: &EncryptedSecret) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new(&self.key);

        if encrypted.nonce.len() != NONCE_SIZE {
            return Err(Error::Crypto {
                message: format!(
                    "invalid nonce size: expected {NONCE_SIZE}, got {}",
                    encrypted.nonce.len()
                ),
            });
        }
        let nonce = Nonce::from_slice(&encrypted.nonce);

        cipher
            .decrypt(nonce, encrypted.ciphertext.as_slice())
            .map_err(|_| Error::Crypto {
                message: "failed to decrypt secret: decryption failed".to_string(),
            })
    }
}

impl std::fmt::Debug for DataEncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataEncryptionKey")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

/// An encrypted DEK stored in the database.
///
/// The DEK is encrypted using AES-256-GCM with the master key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedDek {
    /// The encrypted key material (48 bytes: 32 byte key + 16 byte auth tag).
    pub encrypted_key: Vec<u8>,
    /// The nonce used for encryption (12 bytes).
    pub nonce: Vec<u8>,
}

impl EncryptedDek {
    /// Encodes the encrypted DEK as base64 for storage.
    #[must_use]
    pub fn encode(&self) -> (String, String) {
        (
            URL_SAFE_NO_PAD.encode(&self.encrypted_key),
            URL_SAFE_NO_PAD.encode(&self.nonce),
        )
    }

    /// Decodes an encrypted DEK from base64.
    ///
    /// # Errors
    ///
    /// Returns an error if the base64 is invalid.
    pub fn decode(encrypted_key_b64: &str, nonce_b64: &str) -> Result<Self> {
        let encrypted_key =
            URL_SAFE_NO_PAD
                .decode(encrypted_key_b64)
                .map_err(|e| Error::Crypto {
                    message: format!("invalid base64 in encrypted DEK: {e}"),
                })?;
        let nonce = URL_SAFE_NO_PAD
            .decode(nonce_b64)
            .map_err(|e| Error::Crypto {
                message: format!("invalid base64 in DEK nonce: {e}"),
            })?;
        Ok(Self {
            encrypted_key,
            nonce,
        })
    }
}

/// An encrypted secret value stored in the database.
///
/// The secret is encrypted using AES-256-GCM with a DEK.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSecret {
    /// The encrypted secret value (variable length, includes 16 byte auth tag).
    pub ciphertext: Vec<u8>,
    /// The nonce used for encryption (12 bytes).
    pub nonce: Vec<u8>,
}

impl EncryptedSecret {
    /// Encodes the encrypted secret as base64 for storage.
    #[must_use]
    pub fn encode(&self) -> (String, String) {
        (
            URL_SAFE_NO_PAD.encode(&self.ciphertext),
            URL_SAFE_NO_PAD.encode(&self.nonce),
        )
    }

    /// Decodes an encrypted secret from base64.
    ///
    /// # Errors
    ///
    /// Returns an error if the base64 is invalid.
    pub fn decode(ciphertext_b64: &str, nonce_b64: &str) -> Result<Self> {
        let ciphertext = URL_SAFE_NO_PAD
            .decode(ciphertext_b64)
            .map_err(|e| Error::Crypto {
                message: format!("invalid base64 in ciphertext: {e}"),
            })?;
        let nonce = URL_SAFE_NO_PAD
            .decode(nonce_b64)
            .map_err(|e| Error::Crypto {
                message: format!("invalid base64 in nonce: {e}"),
            })?;
        Ok(Self { ciphertext, nonce })
    }
}

/// Generates a random 12-byte nonce for AES-GCM.
fn generate_nonce() -> [u8; NONCE_SIZE] {
    let mut nonce = [0u8; NONCE_SIZE];
    getrandom::fill(&mut nonce).expect("getrandom failed");
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_key_generate_and_encode() {
        let key = MasterKey::generate();
        let encoded = key.encode();
        let decoded = MasterKey::from_base64(&encoded).unwrap();

        // Verify roundtrip
        assert_eq!(key.key.as_slice(), decoded.key.as_slice());
    }

    #[test]
    fn master_key_from_bytes_wrong_size() {
        let result = MasterKey::from_bytes(&[0u8; 16]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("32 bytes"));
    }

    #[test]
    fn dek_generate_and_encrypt_decrypt() {
        let dek = DataEncryptionKey::generate();
        let plaintext = b"my secret value";

        let encrypted = dek.encrypt(plaintext).unwrap();
        let decrypted = dek.decrypt(&encrypted).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn dek_wrong_key_fails() {
        let dek1 = DataEncryptionKey::generate();
        let dek2 = DataEncryptionKey::generate();

        let encrypted = dek1.encrypt(b"secret").unwrap();
        let result = dek2.decrypt(&encrypted);

        assert!(result.is_err());
    }

    #[test]
    fn envelope_encryption_full_roundtrip() {
        // Generate master key (KEK)
        let master_key = MasterKey::generate();

        // Generate DEK for a secret
        let dek = DataEncryptionKey::generate();

        // Encrypt the secret with DEK
        let secret_value = b"sk-ant-api03-secret-key-value";
        let encrypted_secret = dek.encrypt(secret_value).unwrap();

        // Wrap the DEK with master key for storage
        let encrypted_dek = master_key.wrap_dek(&dek).unwrap();

        // Simulate storage and retrieval...

        // Unwrap the DEK
        let recovered_dek = master_key.unwrap_dek(&encrypted_dek).unwrap();

        // Decrypt the secret
        let recovered_secret = recovered_dek.decrypt(&encrypted_secret).unwrap();

        assert_eq!(secret_value.as_slice(), recovered_secret.as_slice());
    }

    #[test]
    fn envelope_encryption_different_master_key_fails() {
        let master_key1 = MasterKey::generate();
        let master_key2 = MasterKey::generate();

        let dek = DataEncryptionKey::generate();
        let encrypted_dek = master_key1.wrap_dek(&dek).unwrap();

        // Try to unwrap with wrong master key
        let result = master_key2.unwrap_dek(&encrypted_dek);
        assert!(result.is_err());
    }

    #[test]
    fn encrypted_dek_encode_decode() {
        let master_key = MasterKey::generate();
        let dek = DataEncryptionKey::generate();

        let encrypted = master_key.wrap_dek(&dek).unwrap();
        let (key_b64, nonce_b64) = encrypted.encode();

        let decoded = EncryptedDek::decode(&key_b64, &nonce_b64).unwrap();
        assert_eq!(encrypted.encrypted_key, decoded.encrypted_key);
        assert_eq!(encrypted.nonce, decoded.nonce);
    }

    #[test]
    fn encrypted_secret_encode_decode() {
        let dek = DataEncryptionKey::generate();
        let encrypted = dek.encrypt(b"test").unwrap();

        let (ct_b64, nonce_b64) = encrypted.encode();
        let decoded = EncryptedSecret::decode(&ct_b64, &nonce_b64).unwrap();

        assert_eq!(encrypted.ciphertext, decoded.ciphertext);
        assert_eq!(encrypted.nonce, decoded.nonce);
    }

    #[test]
    fn master_key_debug_redacts() {
        let key = MasterKey::generate();
        let debug = format!("{key:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains(&key.encode()));
    }

    #[test]
    fn dek_debug_redacts() {
        let dek = DataEncryptionKey::generate();
        let debug = format!("{dek:?}");
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn empty_secret_roundtrip() {
        let dek = DataEncryptionKey::generate();
        let encrypted = dek.encrypt(b"").unwrap();
        let decrypted = dek.decrypt(&encrypted).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn large_secret_roundtrip() {
        let dek = DataEncryptionKey::generate();
        let large_secret = vec![0xABu8; 1024 * 1024]; // 1MB

        let encrypted = dek.encrypt(&large_secret).unwrap();
        let decrypted = dek.decrypt(&encrypted).unwrap();

        assert_eq!(large_secret, decrypted);
    }
}

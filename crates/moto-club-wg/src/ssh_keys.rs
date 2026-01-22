//! User SSH key management for garage access.
//!
//! This module handles registration and storage of user SSH public keys:
//!
//! - **Register:** Users register their SSH public key once
//! - **Inject:** Keys are injected into garage `authorized_keys` at creation
//! - **Lookup:** Retrieve user's SSH key(s) for injection
//!
//! # Architecture
//!
//! SSH keys are registered per-user (not per-device):
//!
//! ```text
//! SSH Key Registration:
//!   POST /api/v1/users/ssh-keys { public_key }
//!   → { fingerprint }
//!
//! Garage Creation:
//!   User's SSH keys are injected into /home/moto/.ssh/authorized_keys
//! ```
//!
//! # Example
//!
//! ```
//! use moto_club_wg::ssh_keys::{SshKeyManager, InMemorySshKeyStore, SshKeyRegistration};
//! use uuid::Uuid;
//!
//! // Create manager
//! let store = InMemorySshKeyStore::new();
//! let manager = SshKeyManager::new(store);
//!
//! // Register a key
//! let user_id = Uuid::now_v7();
//! let registration = SshKeyRegistration {
//!     public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample user@host".to_string(),
//! };
//!
//! let key = manager.register_key(user_id, registration).unwrap();
//! assert!(key.fingerprint.starts_with("SHA256:"));
//!
//! // List keys for user
//! let keys = manager.list_keys(user_id).unwrap();
//! assert_eq!(keys.len(), 1);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// Error type for SSH key operations.
#[derive(Debug, thiserror::Error)]
pub enum SshKeyError {
    /// Storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),

    /// Invalid SSH public key format.
    #[error("invalid SSH public key: {0}")]
    InvalidKey(String),

    /// Key not found.
    #[error("SSH key not found: {0}")]
    NotFound(String),
}

/// Result type for SSH key operations.
pub type Result<T> = std::result::Result<T, SshKeyError>;

/// Request to register an SSH key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKeyRegistration {
    /// SSH public key in OpenSSH format (e.g., "ssh-ed25519 AAAA... user@host").
    pub public_key: String,
}

/// Registered SSH key information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredSshKey {
    /// Unique key identifier.
    pub key_id: Uuid,

    /// User who owns this key.
    pub user_id: Uuid,

    /// SSH public key in OpenSSH format.
    pub public_key: String,

    /// Key fingerprint (SHA256 format).
    pub fingerprint: String,

    /// Key algorithm (e.g., "ssh-ed25519", "ssh-rsa").
    pub algorithm: String,

    /// Optional comment from the key (e.g., "user@host").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Response for SSH key registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKeyResponse {
    /// Key fingerprint (SHA256 format).
    pub fingerprint: String,
}

/// Storage backend for SSH key manager.
///
/// This trait abstracts the persistence layer, allowing different backends
/// for testing vs production.
pub trait SshKeyStore: Send + Sync {
    /// Get an SSH key by ID.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_key(&self, key_id: Uuid) -> Result<Option<RegisteredSshKey>>;

    /// Get an SSH key by fingerprint.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn get_key_by_fingerprint(&self, fingerprint: &str) -> Result<Option<RegisteredSshKey>>;

    /// Store an SSH key.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn set_key(&self, key: RegisteredSshKey) -> Result<()>;

    /// Remove an SSH key.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn remove_key(&self, key_id: Uuid) -> Result<Option<RegisteredSshKey>>;

    /// List all SSH keys for a user.
    ///
    /// # Errors
    ///
    /// Returns error if the storage operation fails.
    fn list_keys_by_user(&self, user_id: Uuid) -> Result<Vec<RegisteredSshKey>>;
}

/// SSH key manager for registering and retrieving user SSH keys.
pub struct SshKeyManager<S> {
    store: S,
}

impl<S: SshKeyStore> SshKeyManager<S> {
    /// Create a new SSH key manager.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Register an SSH key for a user.
    ///
    /// If the key is already registered (same fingerprint), returns the existing key.
    ///
    /// # Errors
    ///
    /// Returns error if the key format is invalid or storage operations fail.
    pub fn register_key(
        &self,
        user_id: Uuid,
        registration: SshKeyRegistration,
    ) -> Result<RegisteredSshKey> {
        // Parse and validate the key
        let parsed = parse_ssh_public_key(&registration.public_key)?;

        // Check if key already exists (by fingerprint)
        if let Some(existing) = self.store.get_key_by_fingerprint(&parsed.fingerprint)? {
            // Key already registered - return existing (even if different user for now)
            return Ok(existing);
        }

        let key = RegisteredSshKey {
            key_id: Uuid::now_v7(),
            user_id,
            public_key: registration.public_key,
            fingerprint: parsed.fingerprint,
            algorithm: parsed.algorithm,
            comment: parsed.comment,
        };

        self.store.set_key(key.clone())?;
        Ok(key)
    }

    /// Get an SSH key by ID.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn get_key(&self, key_id: Uuid) -> Result<Option<RegisteredSshKey>> {
        self.store.get_key(key_id)
    }

    /// Get an SSH key by fingerprint.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn get_key_by_fingerprint(&self, fingerprint: &str) -> Result<Option<RegisteredSshKey>> {
        self.store.get_key_by_fingerprint(fingerprint)
    }

    /// Remove an SSH key.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails or key not found.
    pub fn remove_key(&self, key_id: Uuid) -> Result<RegisteredSshKey> {
        self.store
            .remove_key(key_id)?
            .ok_or_else(|| SshKeyError::NotFound(key_id.to_string()))
    }

    /// List all SSH keys for a user.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn list_keys(&self, user_id: Uuid) -> Result<Vec<RegisteredSshKey>> {
        self.store.list_keys_by_user(user_id)
    }

    /// Get all SSH public keys for a user, formatted for `authorized_keys`.
    ///
    /// Each key is on its own line, ready to be written to `~/.ssh/authorized_keys`.
    ///
    /// # Errors
    ///
    /// Returns error if storage operation fails.
    pub fn get_authorized_keys(&self, user_id: Uuid) -> Result<String> {
        let keys = self.store.list_keys_by_user(user_id)?;
        let lines: Vec<&str> = keys.iter().map(|k| k.public_key.as_str()).collect();
        Ok(lines.join("\n"))
    }
}

/// Parsed SSH public key components.
struct ParsedSshKey {
    algorithm: String,
    fingerprint: String,
    comment: Option<String>,
}

/// Parse an SSH public key and compute its fingerprint.
///
/// Supports OpenSSH format: `algorithm base64-key [comment]`
fn parse_ssh_public_key(key: &str) -> Result<ParsedSshKey> {
    let key = key.trim();

    // Split into parts: algorithm, key data, optional comment
    let parts: Vec<&str> = key.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(SshKeyError::InvalidKey(
            "key must have algorithm and base64 data".to_string(),
        ));
    }

    let algorithm = parts[0].to_string();

    // Validate algorithm
    let valid_algorithms = [
        "ssh-ed25519",
        "ssh-rsa",
        "ssh-dss",
        "ecdsa-sha2-nistp256",
        "ecdsa-sha2-nistp384",
        "ecdsa-sha2-nistp521",
        "sk-ssh-ed25519@openssh.com",
        "sk-ecdsa-sha2-nistp256@openssh.com",
    ];

    if !valid_algorithms.contains(&algorithm.as_str()) {
        return Err(SshKeyError::InvalidKey(format!(
            "unsupported algorithm: {algorithm}"
        )));
    }

    // Decode and hash the key data
    let key_data = parts[1];
    let decoded = base64_decode(key_data)
        .map_err(|e| SshKeyError::InvalidKey(format!("invalid base64 in key data: {e}")))?;

    // Compute SHA256 fingerprint
    let fingerprint = compute_sha256_fingerprint(&decoded);

    // Comment is everything after algorithm and key data
    let comment = if parts.len() > 2 {
        Some(parts[2..].join(" "))
    } else {
        None
    };

    Ok(ParsedSshKey {
        algorithm,
        fingerprint,
        comment,
    })
}

/// Simple base64 decoder (no external dependency needed for this simple case).
#[allow(clippy::cast_possible_truncation)]
fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    // Standard base64 alphabet
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits_in_buffer = 0;

    for byte in input.bytes() {
        if byte == b'=' {
            // Padding - stop processing
            break;
        }

        let Some(pos) = ALPHABET.iter().position(|&c| c == byte) else {
            // Skip whitespace
            if byte.is_ascii_whitespace() {
                continue;
            }
            return Err(format!("invalid base64 character: {}", byte as char));
        };
        // Safe: ALPHABET has 64 elements, so position is always < 64
        let value = pos as u32;

        buffer = (buffer << 6) | value;
        bits_in_buffer += 6;

        if bits_in_buffer >= 8 {
            bits_in_buffer -= 8;
            output.push((buffer >> bits_in_buffer) as u8);
            buffer &= (1 << bits_in_buffer) - 1;
        }
    }

    Ok(output)
}

/// Compute SHA256 fingerprint in OpenSSH format (SHA256:base64).
fn compute_sha256_fingerprint(data: &[u8]) -> String {
    let hash = sha256_simple(data);

    // Encode as base64 without padding (OpenSSH style)
    let b64 = base64_encode_no_pad(&hash);

    format!("SHA256:{b64}")
}

/// Simple SHA256 implementation.
/// NOTE: This is a simplified implementation for demo purposes.
/// In production, use a proper cryptographic library like `ring` or `sha2`.
#[allow(
    clippy::many_single_char_names,
    clippy::unreadable_literal,
    clippy::items_after_statements
)]
fn sha256_simple(data: &[u8]) -> [u8; 32] {
    // Initial hash values (first 32 bits of fractional parts of square roots of first 8 primes)
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Round constants
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    // Pre-processing: adding padding bits
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);

    // Pad to 56 mod 64 bytes (448 mod 512 bits)
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }

    // Append original length in bits as 64-bit big-endian
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit chunk
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];

        // Copy chunk into first 16 words
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }

        // Extend the first 16 words into the remaining 48 words
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        // Initialize working variables
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        // Compression function main loop
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        // Add the compressed chunk to the current hash value
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    // Produce the final hash value (big-endian)
    let mut result = [0u8; 32];
    for (i, &val) in h.iter().enumerate() {
        result[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }

    result
}

/// Base64 encode without padding (OpenSSH fingerprint style).
fn base64_encode_no_pad(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();
    let mut buffer: u32 = 0;
    let mut bits_in_buffer = 0;

    for &byte in data {
        buffer = (buffer << 8) | u32::from(byte);
        bits_in_buffer += 8;

        while bits_in_buffer >= 6 {
            bits_in_buffer -= 6;
            let index = ((buffer >> bits_in_buffer) & 0x3F) as usize;
            result.push(ALPHABET[index] as char);
        }
    }

    // Handle remaining bits
    if bits_in_buffer > 0 {
        buffer <<= 6 - bits_in_buffer;
        let index = (buffer & 0x3F) as usize;
        result.push(ALPHABET[index] as char);
    }

    result
}

/// In-memory SSH key store for testing.
///
/// Keys are lost when the store is dropped.
pub struct InMemorySshKeyStore {
    inner: Mutex<InMemorySshKeyStoreInner>,
}

struct InMemorySshKeyStoreInner {
    keys: HashMap<Uuid, RegisteredSshKey>,
    fingerprint_index: HashMap<String, Uuid>,
}

impl InMemorySshKeyStore {
    /// Create a new empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(InMemorySshKeyStoreInner {
                keys: HashMap::new(),
                fingerprint_index: HashMap::new(),
            }),
        }
    }
}

impl Default for InMemorySshKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SshKeyStore for InMemorySshKeyStore {
    fn get_key(&self, key_id: Uuid) -> Result<Option<RegisteredSshKey>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.keys.get(&key_id).cloned())
    }

    fn get_key_by_fingerprint(&self, fingerprint: &str) -> Result<Option<RegisteredSshKey>> {
        let inner = self.inner.lock().unwrap();
        if let Some(&key_id) = inner.fingerprint_index.get(fingerprint) {
            Ok(inner.keys.get(&key_id).cloned())
        } else {
            Ok(None)
        }
    }

    fn set_key(&self, key: RegisteredSshKey) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .fingerprint_index
            .insert(key.fingerprint.clone(), key.key_id);
        inner.keys.insert(key.key_id, key);
        drop(inner);
        Ok(())
    }

    fn remove_key(&self, key_id: Uuid) -> Result<Option<RegisteredSshKey>> {
        let mut inner = self.inner.lock().unwrap();
        let result = if let Some(key) = inner.keys.remove(&key_id) {
            inner.fingerprint_index.remove(&key.fingerprint);
            Some(key)
        } else {
            None
        };
        drop(inner);
        Ok(result)
    }

    fn list_keys_by_user(&self, user_id: Uuid) -> Result<Vec<RegisteredSshKey>> {
        let keys = self
            .inner
            .lock()
            .unwrap()
            .keys
            .values()
            .filter(|k| k.user_id == user_id)
            .cloned()
            .collect();
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_manager() -> SshKeyManager<InMemorySshKeyStore> {
        SshKeyManager::new(InMemorySshKeyStore::new())
    }

    // Example SSH keys for testing (these are test keys, not real)
    const TEST_ED25519_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl test@example.com";
    const TEST_RSA_KEY: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7 user@host";
    const TEST_ECDSA_KEY: &str =
        "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABB user@host";

    #[test]
    fn register_ed25519_key() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let registration = SshKeyRegistration {
            public_key: TEST_ED25519_KEY.to_string(),
        };

        let key = manager.register_key(user_id, registration).unwrap();

        assert_eq!(key.user_id, user_id);
        assert_eq!(key.algorithm, "ssh-ed25519");
        assert!(key.fingerprint.starts_with("SHA256:"));
        assert_eq!(key.comment, Some("test@example.com".to_string()));
        assert_eq!(key.public_key, TEST_ED25519_KEY);
    }

    #[test]
    fn register_rsa_key() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let registration = SshKeyRegistration {
            public_key: TEST_RSA_KEY.to_string(),
        };

        let key = manager.register_key(user_id, registration).unwrap();

        assert_eq!(key.algorithm, "ssh-rsa");
        assert!(key.fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn register_ecdsa_key() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let registration = SshKeyRegistration {
            public_key: TEST_ECDSA_KEY.to_string(),
        };

        let key = manager.register_key(user_id, registration).unwrap();

        assert_eq!(key.algorithm, "ecdsa-sha2-nistp256");
        assert!(key.fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn registration_is_idempotent() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let registration = SshKeyRegistration {
            public_key: TEST_ED25519_KEY.to_string(),
        };

        let key1 = manager.register_key(user_id, registration.clone()).unwrap();
        let key2 = manager.register_key(user_id, registration).unwrap();

        // Same key gets same ID
        assert_eq!(key1.key_id, key2.key_id);
        assert_eq!(key1.fingerprint, key2.fingerprint);
    }

    #[test]
    fn get_key() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        // Not registered yet
        let nonexistent = Uuid::now_v7();
        assert!(manager.get_key(nonexistent).unwrap().is_none());

        // Register
        let registration = SshKeyRegistration {
            public_key: TEST_ED25519_KEY.to_string(),
        };
        let registered = manager.register_key(user_id, registration).unwrap();

        // Now found
        let key = manager.get_key(registered.key_id).unwrap().unwrap();
        assert_eq!(key.key_id, registered.key_id);
        assert_eq!(key.fingerprint, registered.fingerprint);
    }

    #[test]
    fn get_key_by_fingerprint() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let registration = SshKeyRegistration {
            public_key: TEST_ED25519_KEY.to_string(),
        };
        let registered = manager.register_key(user_id, registration).unwrap();

        // Found by fingerprint
        let key = manager
            .get_key_by_fingerprint(&registered.fingerprint)
            .unwrap()
            .unwrap();
        assert_eq!(key.key_id, registered.key_id);
    }

    #[test]
    fn remove_key() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let registration = SshKeyRegistration {
            public_key: TEST_ED25519_KEY.to_string(),
        };
        let registered = manager.register_key(user_id, registration).unwrap();

        // Remove
        let removed = manager.remove_key(registered.key_id).unwrap();
        assert_eq!(removed.key_id, registered.key_id);

        // No longer found
        assert!(manager.get_key(registered.key_id).unwrap().is_none());

        // Remove again fails
        let err = manager.remove_key(registered.key_id).unwrap_err();
        assert!(matches!(err, SshKeyError::NotFound(_)));
    }

    #[test]
    fn list_keys() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        // No keys yet
        assert!(manager.list_keys(user_id).unwrap().is_empty());

        // Register a key
        let registration = SshKeyRegistration {
            public_key: TEST_ED25519_KEY.to_string(),
        };
        manager.register_key(user_id, registration).unwrap();

        // Now has one key
        let keys = manager.list_keys(user_id).unwrap();
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn list_keys_different_users() {
        let manager = create_manager();
        let user1 = Uuid::now_v7();
        let user2 = Uuid::now_v7();

        // User 1 registers a key
        manager
            .register_key(
                user1,
                SshKeyRegistration {
                    public_key: TEST_ED25519_KEY.to_string(),
                },
            )
            .unwrap();

        // User 2 registers a different key
        manager
            .register_key(
                user2,
                SshKeyRegistration {
                    public_key: TEST_RSA_KEY.to_string(),
                },
            )
            .unwrap();

        // Each user sees only their keys
        assert_eq!(manager.list_keys(user1).unwrap().len(), 1);
        assert_eq!(manager.list_keys(user2).unwrap().len(), 1);
    }

    #[test]
    fn get_authorized_keys() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        // Register multiple keys
        manager
            .register_key(
                user_id,
                SshKeyRegistration {
                    public_key: TEST_ED25519_KEY.to_string(),
                },
            )
            .unwrap();
        manager
            .register_key(
                user_id,
                SshKeyRegistration {
                    public_key: TEST_RSA_KEY.to_string(),
                },
            )
            .unwrap();

        let authorized = manager.get_authorized_keys(user_id).unwrap();

        // Should contain both keys separated by newline
        assert!(authorized.contains(TEST_ED25519_KEY));
        assert!(authorized.contains(TEST_RSA_KEY));
    }

    #[test]
    fn invalid_key_format() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        // Missing key data
        let err = manager
            .register_key(
                user_id,
                SshKeyRegistration {
                    public_key: "ssh-ed25519".to_string(),
                },
            )
            .unwrap_err();
        assert!(matches!(err, SshKeyError::InvalidKey(_)));

        // Unknown algorithm
        let err = manager
            .register_key(
                user_id,
                SshKeyRegistration {
                    public_key: "unknown-alg AAAA".to_string(),
                },
            )
            .unwrap_err();
        assert!(matches!(err, SshKeyError::InvalidKey(_)));
    }

    #[test]
    fn key_with_no_comment() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let key_without_comment =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
        let registration = SshKeyRegistration {
            public_key: key_without_comment.to_string(),
        };

        let key = manager.register_key(user_id, registration).unwrap();
        assert!(key.comment.is_none());
    }

    #[test]
    fn key_with_multi_word_comment() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let key_with_spaces = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl my laptop key";
        let registration = SshKeyRegistration {
            public_key: key_with_spaces.to_string(),
        };

        let key = manager.register_key(user_id, registration).unwrap();
        assert_eq!(key.comment, Some("my laptop key".to_string()));
    }

    #[test]
    fn ssh_key_registration_serde() {
        let registration = SshKeyRegistration {
            public_key: TEST_ED25519_KEY.to_string(),
        };

        let json = serde_json::to_string(&registration).unwrap();
        let parsed: SshKeyRegistration = serde_json::from_str(&json).unwrap();

        assert_eq!(registration.public_key, parsed.public_key);
    }

    #[test]
    fn registered_ssh_key_serde() {
        let key = RegisteredSshKey {
            key_id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            public_key: TEST_ED25519_KEY.to_string(),
            fingerprint: "SHA256:test".to_string(),
            algorithm: "ssh-ed25519".to_string(),
            comment: Some("test@example.com".to_string()),
        };

        let json = serde_json::to_string(&key).unwrap();
        let parsed: RegisteredSshKey = serde_json::from_str(&json).unwrap();

        assert_eq!(key.key_id, parsed.key_id);
        assert_eq!(key.user_id, parsed.user_id);
        assert_eq!(key.fingerprint, parsed.fingerprint);
        assert_eq!(key.comment, parsed.comment);
    }

    #[test]
    fn ssh_key_response_serde() {
        let response = SshKeyResponse {
            fingerprint: "SHA256:test123".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: SshKeyResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.fingerprint, parsed.fingerprint);
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let manager = create_manager();
        let user_id = Uuid::now_v7();

        let registration = SshKeyRegistration {
            public_key: TEST_ED25519_KEY.to_string(),
        };

        let key1 = manager.register_key(user_id, registration.clone()).unwrap();

        // Create a new manager and register the same key
        let manager2 = create_manager();
        let key2 = manager2.register_key(user_id, registration).unwrap();

        // Fingerprints should match
        assert_eq!(key1.fingerprint, key2.fingerprint);
    }

    #[test]
    fn base64_decode_works() {
        // "SGVsbG8gV29ybGQ=" is "Hello World" in base64
        let decoded = base64_decode("SGVsbG8gV29ybGQ=").unwrap();
        assert_eq!(decoded, b"Hello World");
    }

    #[test]
    fn base64_encode_works() {
        let encoded = base64_encode_no_pad(b"Hello World");
        // Without padding, "Hello World" is "SGVsbG8gV29ybGQ"
        assert_eq!(encoded, "SGVsbG8gV29ybGQ");
    }
}

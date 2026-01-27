//! Cryptographic operations for moto-keybox.
//!
//! This module provides:
//! - Key management (KEK and SVID signing key)
//! - Envelope encryption (DEK generation, encryption, decryption)
//! - SVID signing

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use ed25519_dalek::{SigningKey, VerifyingKey};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Size of AES-256 key in bytes.
const AES_KEY_SIZE: usize = 32;

/// Size of AES-GCM nonce in bytes.
const NONCE_SIZE: usize = 12;

/// Cryptographic errors.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// Failed to read key file.
    #[error("failed to read key file: {0}")]
    KeyFileRead(#[from] std::io::Error),

    /// Invalid key format.
    #[error("invalid key format: {0}")]
    InvalidKeyFormat(String),

    /// Encryption failed.
    #[error("encryption failed")]
    EncryptionFailed,

    /// Decryption failed.
    #[error("decryption failed")]
    DecryptionFailed,

    /// Base64 decode error.
    #[error("base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),
}

/// Result type for cryptographic operations.
pub type CryptoResult<T> = Result<T, CryptoError>;

/// A 256-bit key that zeroizes on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretKey([u8; AES_KEY_SIZE]);

impl SecretKey {
    /// Create a new random key.
    pub fn generate() -> Self {
        let mut key = [0u8; AES_KEY_SIZE];
        OsRng.fill_bytes(&mut key);
        Self(key)
    }

    /// Create from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the slice is not exactly 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> CryptoResult<Self> {
        let key: [u8; AES_KEY_SIZE] = bytes.try_into().map_err(|_| {
            CryptoError::InvalidKeyFormat(format!(
                "expected {AES_KEY_SIZE} bytes, got {}",
                bytes.len()
            ))
        })?;
        Ok(Self(key))
    }

    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8; AES_KEY_SIZE] {
        &self.0
    }
}

/// Key manager holding the master key (KEK) and SVID signing key.
pub struct KeyManager {
    /// Master key for encrypting DEKs.
    master_key: SecretKey,
    /// Ed25519 signing key for SVIDs.
    signing_key: SigningKey,
    /// Public verifying key (derived from signing key).
    verifying_key: VerifyingKey,
}

impl KeyManager {
    /// Load keys from files.
    ///
    /// # Errors
    ///
    /// Returns an error if files cannot be read or contain invalid keys.
    pub fn from_files(master_key_path: &str, signing_key_path: &str) -> CryptoResult<Self> {
        // Read and decode master key (base64-encoded AES-256 key)
        let master_key_b64 = std::fs::read_to_string(master_key_path)?;
        let master_key_bytes = BASE64.decode(master_key_b64.trim())?;
        let master_key = SecretKey::from_bytes(&master_key_bytes)?;

        // Read and decode signing key (base64-encoded Ed25519 seed)
        let signing_key_b64 = std::fs::read_to_string(signing_key_path)?;
        let signing_key_bytes = BASE64.decode(signing_key_b64.trim())?;
        let signing_key_array: [u8; 32] = signing_key_bytes.try_into().map_err(|_| {
            CryptoError::InvalidKeyFormat("Ed25519 key must be 32 bytes".to_string())
        })?;
        let signing_key = SigningKey::from_bytes(&signing_key_array);
        let verifying_key = signing_key.verifying_key();

        Ok(Self {
            master_key,
            signing_key,
            verifying_key,
        })
    }

    /// Create a key manager with provided keys (for testing).
    #[cfg(test)]
    pub fn new(master_key: SecretKey, signing_key: SigningKey) -> Self {
        let verifying_key = signing_key.verifying_key();
        Self {
            master_key,
            signing_key,
            verifying_key,
        }
    }

    /// Get the signing key for SVID generation.
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Get the verifying key for SVID validation.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// Generate a new DEK and encrypt it with the master key.
    ///
    /// Returns (encrypted_dek, nonce, plaintext_dek).
    pub fn generate_dek(&self) -> CryptoResult<(Vec<u8>, Vec<u8>, SecretKey)> {
        let dek = SecretKey::generate();
        let (encrypted, nonce) = self.encrypt_with_master(dek.as_bytes())?;
        Ok((encrypted, nonce, dek))
    }

    /// Decrypt a DEK using the master key.
    pub fn decrypt_dek(&self, encrypted_dek: &[u8], nonce: &[u8]) -> CryptoResult<SecretKey> {
        let decrypted = self.decrypt_with_master(encrypted_dek, nonce)?;
        SecretKey::from_bytes(&decrypted)
    }

    /// Encrypt data with a DEK.
    pub fn encrypt_secret(
        &self,
        plaintext: &[u8],
        dek: &SecretKey,
    ) -> CryptoResult<(Vec<u8>, Vec<u8>)> {
        encrypt_aes_gcm(plaintext, dek.as_bytes())
    }

    /// Decrypt data with a DEK.
    pub fn decrypt_secret(
        &self,
        ciphertext: &[u8],
        nonce: &[u8],
        dek: &SecretKey,
    ) -> CryptoResult<Vec<u8>> {
        decrypt_aes_gcm(ciphertext, nonce, dek.as_bytes())
    }

    /// Encrypt data with the master key.
    fn encrypt_with_master(&self, plaintext: &[u8]) -> CryptoResult<(Vec<u8>, Vec<u8>)> {
        encrypt_aes_gcm(plaintext, self.master_key.as_bytes())
    }

    /// Decrypt data with the master key.
    fn decrypt_with_master(&self, ciphertext: &[u8], nonce: &[u8]) -> CryptoResult<Vec<u8>> {
        decrypt_aes_gcm(ciphertext, nonce, self.master_key.as_bytes())
    }
}

/// Encrypt data using AES-256-GCM.
fn encrypt_aes_gcm(plaintext: &[u8], key: &[u8; AES_KEY_SIZE]) -> CryptoResult<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key).expect("key size is correct");

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CryptoError::EncryptionFailed)?;

    Ok((ciphertext, nonce_bytes.to_vec()))
}

/// Decrypt data using AES-256-GCM.
fn decrypt_aes_gcm(
    ciphertext: &[u8],
    nonce: &[u8],
    key: &[u8; AES_KEY_SIZE],
) -> CryptoResult<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).expect("key size is correct");
    let nonce = Nonce::from_slice(nonce);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptionFailed)?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_key_generate_and_use() {
        let key = SecretKey::generate();
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = SecretKey::generate();
        let plaintext = b"secret data";

        let (ciphertext, nonce) = encrypt_aes_gcm(plaintext, key.as_bytes()).unwrap();
        let decrypted = decrypt_aes_gcm(&ciphertext, &nonce, key.as_bytes()).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn dek_generation_and_decryption() {
        let master_key = SecretKey::generate();
        let signing_key = SigningKey::generate(&mut OsRng);
        let km = KeyManager::new(master_key, signing_key);

        let (encrypted_dek, nonce, original_dek) = km.generate_dek().unwrap();
        let decrypted_dek = km.decrypt_dek(&encrypted_dek, &nonce).unwrap();

        assert_eq!(original_dek.as_bytes(), decrypted_dek.as_bytes());
    }

    #[test]
    fn secret_encryption_roundtrip() {
        let master_key = SecretKey::generate();
        let signing_key = SigningKey::generate(&mut OsRng);
        let km = KeyManager::new(master_key, signing_key);

        let (_, _, dek) = km.generate_dek().unwrap();
        let plaintext = b"my secret value";

        let (ciphertext, nonce) = km.encrypt_secret(plaintext, &dek).unwrap();
        let decrypted = km.decrypt_secret(&ciphertext, &nonce, &dek).unwrap();

        assert_eq!(decrypted, plaintext);
    }
}

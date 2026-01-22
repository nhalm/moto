//! SSH server integration for garage access.
//!
//! Manages the SSH server configuration and authorized keys for the garage pod.
//! The SSH server listens on the `WireGuard` overlay IP and accepts connections
//! only from authenticated tunnel peers.
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │  SSH Server (in garage pod)                                          │
//! │  ├── ListenAddress: <overlay_ip>:22                                  │
//! │  ├── User: moto (non-root)                                           │
//! │  ├── AuthorizedKeysFile: /home/moto/.ssh/authorized_keys             │
//! │  └── PubkeyAuthentication: yes, PasswordAuthentication: no           │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Injection
//!
//! When a garage is created, moto-club injects the user's SSH public key into
//! the garage's `authorized_keys` file. This module provides utilities to manage
//! those keys.
//!
//! # Example
//!
//! ```ignore
//! use moto_garage_wgtunnel::ssh::{AuthorizedKeys, SshConfig, SshPublicKey};
//!
//! // Parse and validate SSH public key
//! let key = SshPublicKey::parse("ssh-ed25519 AAAA... user@host")?;
//!
//! // Manage authorized keys
//! let mut keys = AuthorizedKeys::new();
//! keys.add(key);
//! keys.write_to_file("/home/moto/.ssh/authorized_keys").await?;
//!
//! // Configure SSH server
//! let config = SshConfig::builder()
//!     .listen_address("[fd00:moto:1::1]:22".parse()?)
//!     .user("moto")
//!     .build();
//! ```

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Default SSH port.
pub const DEFAULT_SSH_PORT: u16 = 22;

/// Default SSH user for garage access.
pub const DEFAULT_SSH_USER: &str = "moto";

/// Default path for authorized keys file.
pub const DEFAULT_AUTHORIZED_KEYS_PATH: &str = "/home/moto/.ssh/authorized_keys";

/// Default shell for SSH sessions.
pub const DEFAULT_SHELL: &str = "/bin/bash";

/// Error type for SSH operations.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    /// Invalid SSH public key format.
    #[error("invalid SSH public key: {0}")]
    InvalidPublicKey(String),

    /// IO error reading or writing files.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),
}

/// Result type for SSH operations.
pub type Result<T> = std::result::Result<T, SshError>;

/// SSH public key types supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyType {
    /// RSA key (ssh-rsa).
    Rsa,
    /// Ed25519 key (ssh-ed25519).
    Ed25519,
    /// ECDSA key (ecdsa-sha2-nistp256, etc.).
    Ecdsa,
}

impl KeyType {
    /// Parse a key type from its SSH prefix.
    ///
    /// # Errors
    ///
    /// Returns error if the prefix is not recognized.
    pub fn from_prefix(prefix: &str) -> Result<Self> {
        match prefix {
            "ssh-rsa" => Ok(Self::Rsa),
            "ssh-ed25519" => Ok(Self::Ed25519),
            s if s.starts_with("ecdsa-sha2-") => Ok(Self::Ecdsa),
            _ => Err(SshError::InvalidPublicKey(format!(
                "unknown key type prefix: {prefix}"
            ))),
        }
    }

    /// Get the SSH prefix for this key type.
    #[must_use]
    pub const fn prefix(&self) -> &'static str {
        match self {
            Self::Rsa => "ssh-rsa",
            Self::Ed25519 => "ssh-ed25519",
            Self::Ecdsa => "ecdsa-sha2-nistp256",
        }
    }
}

impl fmt::Display for KeyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.prefix())
    }
}

/// A parsed SSH public key.
///
/// SSH public keys are in the format: `<type> <base64-data> [comment]`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SshPublicKey {
    /// Key type (RSA, Ed25519, ECDSA).
    key_type: KeyType,

    /// Base64-encoded key data.
    key_data: String,

    /// Optional comment (typically user@host).
    comment: Option<String>,
}

impl SshPublicKey {
    /// Parse an SSH public key from its string representation.
    ///
    /// Expects format: `<type> <base64-data> [comment]`
    ///
    /// # Errors
    ///
    /// Returns error if the key format is invalid.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();

        let parts: Vec<&str> = s.splitn(3, ' ').collect();
        if parts.len() < 2 {
            return Err(SshError::InvalidPublicKey(
                "expected format: <type> <base64-data> [comment]".to_string(),
            ));
        }

        let key_type = KeyType::from_prefix(parts[0])?;
        let key_data = parts[1].to_string();

        // Validate base64 encoding (basic check)
        if key_data.is_empty() {
            return Err(SshError::InvalidPublicKey(
                "key data cannot be empty".to_string(),
            ));
        }

        // Base64 should only contain valid characters
        if !key_data
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        {
            return Err(SshError::InvalidPublicKey(
                "invalid base64 characters in key data".to_string(),
            ));
        }

        let comment = parts.get(2).map(|s| (*s).to_string());

        Ok(Self {
            key_type,
            key_data,
            comment,
        })
    }

    /// Create an SSH public key from components.
    #[must_use]
    pub const fn new(key_type: KeyType, key_data: String, comment: Option<String>) -> Self {
        Self {
            key_type,
            key_data,
            comment,
        }
    }

    /// Get the key type.
    #[must_use]
    pub const fn key_type(&self) -> KeyType {
        self.key_type
    }

    /// Get the base64-encoded key data.
    #[must_use]
    pub fn key_data(&self) -> &str {
        &self.key_data
    }

    /// Get the optional comment.
    #[must_use]
    pub fn comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    /// Set or replace the comment.
    pub fn set_comment(&mut self, comment: impl Into<String>) {
        self.comment = Some(comment.into());
    }

    /// Remove the comment.
    pub fn clear_comment(&mut self) {
        self.comment = None;
    }

    /// Format the key for an `authorized_keys` file.
    #[must_use]
    pub fn to_authorized_keys_line(&self) -> String {
        self.comment.as_ref().map_or_else(
            || format!("{} {}", self.key_type.prefix(), self.key_data),
            |comment| format!("{} {} {}", self.key_type.prefix(), self.key_data, comment),
        )
    }

    /// Compute a fingerprint for display purposes.
    ///
    /// Note: This is a simplified fingerprint (first 16 chars of key data).
    /// A proper implementation would compute SHA256 of the decoded key.
    #[must_use]
    pub fn fingerprint_short(&self) -> String {
        let short = if self.key_data.len() > 16 {
            &self.key_data[..16]
        } else {
            &self.key_data
        };
        format!("{}:{}...", self.key_type.prefix(), short)
    }
}

impl fmt::Display for SshPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_authorized_keys_line())
    }
}

/// Collection of authorized SSH public keys.
///
/// Manages the set of keys that can authenticate to the garage SSH server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthorizedKeys {
    keys: Vec<SshPublicKey>,
}

impl AuthorizedKeys {
    /// Create an empty authorized keys collection.
    #[must_use]
    pub const fn new() -> Self {
        Self { keys: Vec::new() }
    }

    /// Create from a list of keys.
    #[must_use]
    pub const fn from_keys(keys: Vec<SshPublicKey>) -> Self {
        Self { keys }
    }

    /// Parse authorized keys from file contents.
    ///
    /// Ignores empty lines and lines starting with `#` (comments).
    ///
    /// # Errors
    ///
    /// Returns error if any key line fails to parse.
    pub fn parse(contents: &str) -> Result<Self> {
        let mut keys = Vec::new();

        for line in contents.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            keys.push(SshPublicKey::parse(line)?);
        }

        Ok(Self { keys })
    }

    /// Read authorized keys from a file.
    ///
    /// # Errors
    ///
    /// Returns error if the file cannot be read or parsed.
    pub async fn read_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let contents = tokio::fs::read_to_string(path.as_ref()).await?;
        Self::parse(&contents)
    }

    /// Write authorized keys to a file.
    ///
    /// Creates parent directories if they don't exist.
    /// Sets file permissions to 0600 (owner read/write only).
    ///
    /// # Errors
    ///
    /// Returns error if the file cannot be written.
    pub async fn write_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let contents = self.to_string();
        tokio::fs::write(path, contents.as_bytes()).await?;

        // Set permissions to 0600 (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(path, permissions).await?;
        }

        Ok(())
    }

    /// Get the number of keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Check if there are no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Get an iterator over the keys.
    pub fn iter(&self) -> impl Iterator<Item = &SshPublicKey> {
        self.keys.iter()
    }

    /// Add a key to the collection.
    ///
    /// Duplicates are allowed (same key can appear multiple times).
    pub fn add(&mut self, key: SshPublicKey) {
        self.keys.push(key);
    }

    /// Add a key only if it's not already present.
    ///
    /// Returns `true` if the key was added, `false` if it already existed.
    pub fn add_unique(&mut self, key: SshPublicKey) -> bool {
        if self.contains(&key) {
            return false;
        }
        self.keys.push(key);
        true
    }

    /// Check if a key is in the collection.
    #[must_use]
    pub fn contains(&self, key: &SshPublicKey) -> bool {
        self.keys.iter().any(|k| k.key_data == key.key_data)
    }

    /// Remove a key from the collection.
    ///
    /// Returns `true` if a key was removed.
    pub fn remove(&mut self, key: &SshPublicKey) -> bool {
        let initial_len = self.keys.len();
        self.keys.retain(|k| k.key_data != key.key_data);
        self.keys.len() < initial_len
    }

    /// Remove all keys and return them.
    pub fn clear(&mut self) -> Vec<SshPublicKey> {
        std::mem::take(&mut self.keys)
    }
}

impl fmt::Display for AuthorizedKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for key in &self.keys {
            writeln!(f, "{}", key.to_authorized_keys_line())?;
        }
        Ok(())
    }
}

impl IntoIterator for AuthorizedKeys {
    type Item = SshPublicKey;
    type IntoIter = std::vec::IntoIter<SshPublicKey>;

    fn into_iter(self) -> Self::IntoIter {
        self.keys.into_iter()
    }
}

impl<'a> IntoIterator for &'a AuthorizedKeys {
    type Item = &'a SshPublicKey;
    type IntoIter = std::slice::Iter<'a, SshPublicKey>;

    fn into_iter(self) -> Self::IntoIter {
        self.keys.iter()
    }
}

impl FromIterator<SshPublicKey> for AuthorizedKeys {
    fn from_iter<T: IntoIterator<Item = SshPublicKey>>(iter: T) -> Self {
        Self {
            keys: iter.into_iter().collect(),
        }
    }
}

/// SSH server configuration.
///
/// Defines how the SSH server should be configured for garage access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    /// Address to listen on (typically overlay IP).
    listen_address: SocketAddr,

    /// User for SSH sessions.
    user: String,

    /// Path to authorized keys file.
    authorized_keys_path: PathBuf,

    /// Shell to use for sessions.
    shell: String,

    /// Whether password authentication is allowed (should always be false).
    password_auth: bool,

    /// Whether public key authentication is allowed (should always be true).
    pubkey_auth: bool,
}

impl SshConfig {
    /// Create a new SSH config builder.
    #[must_use]
    pub fn builder() -> SshConfigBuilder {
        SshConfigBuilder::default()
    }

    /// Create a config with default values for a given listen address.
    #[must_use]
    pub fn with_listen_address(addr: SocketAddr) -> Self {
        Self::builder().listen_address(addr).build()
    }

    /// Get the listen address.
    #[must_use]
    pub const fn listen_address(&self) -> SocketAddr {
        self.listen_address
    }

    /// Get the user.
    #[must_use]
    pub fn user(&self) -> &str {
        &self.user
    }

    /// Get the authorized keys path.
    #[must_use]
    pub fn authorized_keys_path(&self) -> &Path {
        &self.authorized_keys_path
    }

    /// Get the shell.
    #[must_use]
    pub fn shell(&self) -> &str {
        &self.shell
    }

    /// Check if password authentication is enabled.
    #[must_use]
    pub const fn password_auth(&self) -> bool {
        self.password_auth
    }

    /// Check if public key authentication is enabled.
    #[must_use]
    pub const fn pubkey_auth(&self) -> bool {
        self.pubkey_auth
    }

    /// Validate the configuration.
    ///
    /// # Errors
    ///
    /// Returns error if configuration is invalid.
    pub fn validate(&self) -> Result<()> {
        if self.user.is_empty() {
            return Err(SshError::Config("user cannot be empty".to_string()));
        }

        if self.shell.is_empty() {
            return Err(SshError::Config("shell cannot be empty".to_string()));
        }

        if !self.pubkey_auth {
            return Err(SshError::Config(
                "public key authentication must be enabled".to_string(),
            ));
        }

        if self.password_auth {
            return Err(SshError::Config(
                "password authentication must be disabled for security".to_string(),
            ));
        }

        Ok(())
    }

    /// Generate `sshd_config` format string.
    #[must_use]
    pub fn to_sshd_config(&self) -> String {
        use std::fmt::Write;

        let mut config = String::new();

        let _ = writeln!(config, "Port {}", self.listen_address.port());
        let _ = writeln!(config, "ListenAddress {}", self.listen_address.ip());
        let _ = writeln!(
            config,
            "PubkeyAuthentication {}",
            if self.pubkey_auth { "yes" } else { "no" }
        );
        let _ = writeln!(
            config,
            "PasswordAuthentication {}",
            if self.password_auth { "yes" } else { "no" }
        );
        let _ = writeln!(
            config,
            "AuthorizedKeysFile {}",
            self.authorized_keys_path.display()
        );

        config
    }
}

/// Builder for [`SshConfig`].
#[derive(Debug, Default)]
pub struct SshConfigBuilder {
    listen_address: Option<SocketAddr>,
    user: Option<String>,
    authorized_keys_path: Option<PathBuf>,
    shell: Option<String>,
}

impl SshConfigBuilder {
    /// Set the listen address.
    #[must_use]
    pub const fn listen_address(mut self, addr: SocketAddr) -> Self {
        self.listen_address = Some(addr);
        self
    }

    /// Set the user.
    #[must_use]
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Set the authorized keys path.
    #[must_use]
    pub fn authorized_keys_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.authorized_keys_path = Some(path.into());
        self
    }

    /// Set the shell.
    #[must_use]
    pub fn shell(mut self, shell: impl Into<String>) -> Self {
        self.shell = Some(shell.into());
        self
    }

    /// Build the configuration.
    ///
    /// # Panics
    ///
    /// Panics if `listen_address` is not set.
    #[must_use]
    pub fn build(self) -> SshConfig {
        SshConfig {
            listen_address: self
                .listen_address
                .expect("listen_address is required"),
            user: self.user.unwrap_or_else(|| DEFAULT_SSH_USER.to_string()),
            authorized_keys_path: self
                .authorized_keys_path
                .unwrap_or_else(|| PathBuf::from(DEFAULT_AUTHORIZED_KEYS_PATH)),
            shell: self.shell.unwrap_or_else(|| DEFAULT_SHELL.to_string()),
            password_auth: false,
            pubkey_auth: true,
        }
    }

    /// Try to build the configuration.
    ///
    /// # Errors
    ///
    /// Returns error if `listen_address` is not set.
    pub fn try_build(self) -> Result<SshConfig> {
        let listen_address = self
            .listen_address
            .ok_or_else(|| SshError::Config("listen_address is required".to_string()))?;

        Ok(SshConfig {
            listen_address,
            user: self.user.unwrap_or_else(|| DEFAULT_SSH_USER.to_string()),
            authorized_keys_path: self
                .authorized_keys_path
                .unwrap_or_else(|| PathBuf::from(DEFAULT_AUTHORIZED_KEYS_PATH)),
            shell: self.shell.unwrap_or_else(|| DEFAULT_SHELL.to_string()),
            password_auth: false,
            pubkey_auth: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_type_from_prefix() {
        assert_eq!(KeyType::from_prefix("ssh-rsa").unwrap(), KeyType::Rsa);
        assert_eq!(
            KeyType::from_prefix("ssh-ed25519").unwrap(),
            KeyType::Ed25519
        );
        assert_eq!(
            KeyType::from_prefix("ecdsa-sha2-nistp256").unwrap(),
            KeyType::Ecdsa
        );
        assert_eq!(
            KeyType::from_prefix("ecdsa-sha2-nistp384").unwrap(),
            KeyType::Ecdsa
        );

        assert!(KeyType::from_prefix("unknown").is_err());
    }

    #[test]
    fn key_type_prefix() {
        assert_eq!(KeyType::Rsa.prefix(), "ssh-rsa");
        assert_eq!(KeyType::Ed25519.prefix(), "ssh-ed25519");
        assert_eq!(KeyType::Ecdsa.prefix(), "ecdsa-sha2-nistp256");
    }

    #[test]
    fn key_type_display() {
        assert_eq!(format!("{}", KeyType::Ed25519), "ssh-ed25519");
    }

    #[test]
    fn ssh_public_key_parse_ed25519() {
        let key_str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBVqP user@host";
        let key = SshPublicKey::parse(key_str).unwrap();

        assert_eq!(key.key_type(), KeyType::Ed25519);
        assert_eq!(key.key_data(), "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP");
        assert_eq!(key.comment(), Some("user@host"));
    }

    #[test]
    fn ssh_public_key_parse_rsa() {
        let key_str = "ssh-rsa AAAAB3NzaC1yc2EAAAA";
        let key = SshPublicKey::parse(key_str).unwrap();

        assert_eq!(key.key_type(), KeyType::Rsa);
        assert_eq!(key.key_data(), "AAAAB3NzaC1yc2EAAAA");
        assert_eq!(key.comment(), None);
    }

    #[test]
    fn ssh_public_key_parse_with_spaces_in_comment() {
        let key_str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBVqP my key with spaces";
        let key = SshPublicKey::parse(key_str).unwrap();

        assert_eq!(key.comment(), Some("my key with spaces"));
    }

    #[test]
    fn ssh_public_key_parse_invalid() {
        // Missing key data
        assert!(SshPublicKey::parse("ssh-ed25519").is_err());

        // Unknown key type
        assert!(SshPublicKey::parse("unknown-type AAAAA").is_err());

        // Empty key data
        assert!(SshPublicKey::parse("ssh-ed25519 ").is_err());

        // Invalid base64 characters
        assert!(SshPublicKey::parse("ssh-ed25519 invalid!chars").is_err());
    }

    #[test]
    fn ssh_public_key_to_authorized_keys_line() {
        let key = SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            Some("user@host".to_string()),
        );

        assert_eq!(
            key.to_authorized_keys_line(),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBVqP user@host"
        );

        let key_no_comment = SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            None,
        );

        assert_eq!(
            key_no_comment.to_authorized_keys_line(),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBVqP"
        );
    }

    #[test]
    fn ssh_public_key_comment_operations() {
        let mut key = SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            None,
        );

        assert!(key.comment().is_none());

        key.set_comment("new comment");
        assert_eq!(key.comment(), Some("new comment"));

        key.clear_comment();
        assert!(key.comment().is_none());
    }

    #[test]
    fn ssh_public_key_fingerprint_short() {
        let key = SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            None,
        );

        let fp = key.fingerprint_short();
        assert!(fp.starts_with("ssh-ed25519:"));
        assert!(fp.ends_with("..."));
    }

    #[test]
    fn ssh_public_key_serde() {
        let key = SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            Some("user@host".to_string()),
        );

        let json = serde_json::to_string(&key).unwrap();
        let parsed: SshPublicKey = serde_json::from_str(&json).unwrap();

        assert_eq!(key, parsed);
    }

    #[test]
    fn authorized_keys_empty() {
        let keys = AuthorizedKeys::new();
        assert!(keys.is_empty());
        assert_eq!(keys.len(), 0);
    }

    #[test]
    fn authorized_keys_add() {
        let mut keys = AuthorizedKeys::new();
        let key = SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            None,
        );

        keys.add(key.clone());
        assert_eq!(keys.len(), 1);
        assert!(keys.contains(&key));
    }

    #[test]
    fn authorized_keys_add_unique() {
        let mut keys = AuthorizedKeys::new();
        let key = SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            None,
        );

        assert!(keys.add_unique(key.clone()));
        assert!(!keys.add_unique(key));
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn authorized_keys_remove() {
        let mut keys = AuthorizedKeys::new();
        let key = SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            None,
        );

        keys.add(key.clone());
        assert!(keys.remove(&key));
        assert!(keys.is_empty());
        assert!(!keys.remove(&key));
    }

    #[test]
    fn authorized_keys_parse() {
        let contents = r"
# This is a comment
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBVqP user@host
ssh-rsa AAAAB3NzaC1yc2EAAAA

# Another comment
        ";

        let keys = AuthorizedKeys::parse(contents).unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn authorized_keys_display() {
        let mut keys = AuthorizedKeys::new();
        keys.add(SshPublicKey::new(
            KeyType::Ed25519,
            "AAAAC3NzaC1lZDI1NTE5AAAAIBVqP".to_string(),
            Some("user@host".to_string()),
        ));
        keys.add(SshPublicKey::new(
            KeyType::Rsa,
            "AAAAB3NzaC1yc2EAAAA".to_string(),
            None,
        ));

        let output = keys.to_string();
        assert!(output.contains("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBVqP user@host"));
        assert!(output.contains("ssh-rsa AAAAB3NzaC1yc2EAAAA"));
    }

    #[test]
    fn authorized_keys_iterate() {
        let mut keys = AuthorizedKeys::new();
        keys.add(SshPublicKey::new(
            KeyType::Ed25519,
            "key1".to_string(),
            None,
        ));
        keys.add(SshPublicKey::new(
            KeyType::Ed25519,
            "key2".to_string(),
            None,
        ));

        let count = keys.iter().count();
        assert_eq!(count, 2);

        let collected: Vec<_> = keys.into_iter().collect();
        assert_eq!(collected.len(), 2);
    }

    #[test]
    fn authorized_keys_from_iterator() {
        let keys_vec = vec![
            SshPublicKey::new(KeyType::Ed25519, "key1".to_string(), None),
            SshPublicKey::new(KeyType::Ed25519, "key2".to_string(), None),
        ];

        let keys: AuthorizedKeys = keys_vec.into_iter().collect();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn ssh_config_builder() {
        let addr: SocketAddr = "[::1]:22".parse().unwrap();
        let config = SshConfig::builder()
            .listen_address(addr)
            .user("testuser")
            .shell("/bin/zsh")
            .authorized_keys_path("/custom/path")
            .build();

        assert_eq!(config.listen_address(), addr);
        assert_eq!(config.user(), "testuser");
        assert_eq!(config.shell(), "/bin/zsh");
        assert_eq!(
            config.authorized_keys_path(),
            Path::new("/custom/path")
        );
        assert!(!config.password_auth());
        assert!(config.pubkey_auth());
    }

    #[test]
    fn ssh_config_defaults() {
        let addr: SocketAddr = "[::1]:22".parse().unwrap();
        let config = SshConfig::builder().listen_address(addr).build();

        assert_eq!(config.user(), DEFAULT_SSH_USER);
        assert_eq!(config.shell(), DEFAULT_SHELL);
        assert_eq!(
            config.authorized_keys_path(),
            Path::new(DEFAULT_AUTHORIZED_KEYS_PATH)
        );
    }

    #[test]
    fn ssh_config_validate() {
        let addr: SocketAddr = "[::1]:22".parse().unwrap();

        // Valid config
        let config = SshConfig::builder().listen_address(addr).build();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn ssh_config_with_listen_address() {
        use std::net::{Ipv6Addr, SocketAddrV6};

        let ipv6: Ipv6Addr = "fd00:0:0:0:0:0:0:1".parse().unwrap();
        let addr = SocketAddr::V6(SocketAddrV6::new(ipv6, 22, 0, 0));
        let config = SshConfig::with_listen_address(addr);

        assert_eq!(config.listen_address(), addr);
        assert_eq!(config.user(), DEFAULT_SSH_USER);
    }

    #[test]
    fn ssh_config_to_sshd_config() {
        use std::net::{Ipv6Addr, SocketAddrV6};

        let ipv6: Ipv6Addr = "fd00:0:0:0:0:0:0:1".parse().unwrap();
        let addr = SocketAddr::V6(SocketAddrV6::new(ipv6, 22, 0, 0));
        let config = SshConfig::builder().listen_address(addr).build();

        let sshd_config = config.to_sshd_config();

        assert!(sshd_config.contains("Port 22"));
        assert!(sshd_config.contains("ListenAddress fd00::1"));
        assert!(sshd_config.contains("PubkeyAuthentication yes"));
        assert!(sshd_config.contains("PasswordAuthentication no"));
        assert!(sshd_config.contains("AuthorizedKeysFile /home/moto/.ssh/authorized_keys"));
    }

    #[test]
    fn ssh_config_builder_try_build() {
        // Missing listen_address
        let result = SshConfigBuilder::default().try_build();
        assert!(matches!(result, Err(SshError::Config(_))));

        // Valid
        let addr: SocketAddr = "[::1]:22".parse().unwrap();
        let result = SshConfigBuilder::default().listen_address(addr).try_build();
        assert!(result.is_ok());
    }

    #[test]
    fn error_display() {
        let err = SshError::InvalidPublicKey("test".to_string());
        assert!(err.to_string().contains("test"));

        let err = SshError::Config("config error".to_string());
        assert!(err.to_string().contains("config error"));
    }
}

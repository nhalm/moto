//! Keybox key store — fetches and caches provider API keys from keybox.
//!
//! The ai-proxy authenticates to keybox using its own SVID
//! (`spiffe://moto.local/service/ai-proxy`) and fetches API keys for each
//! provider. Keys are cached in memory with a configurable TTL to avoid
//! hitting keybox on every request.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use secrecy::{ExposeSecret, SecretString};
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

use moto_keybox_client::{KeyboxClient, Scope};

use crate::provider::Provider;

/// Trait for fetching provider API keys.
///
/// Abstracted behind a trait so tests can inject a mock key store
/// without requiring a real keybox instance.
pub trait KeyStore: Send + Sync {
    /// Gets the API key for a provider, returning `None` if not configured.
    fn get_key(
        &self,
        provider: Provider,
    ) -> impl std::future::Future<Output = Option<SecretString>> + Send;
}

/// Cached entry for a provider API key.
struct CachedKey {
    /// The secret API key.
    key: SecretString,
    /// When this cache entry was fetched.
    fetched_at: Instant,
}

/// Key store backed by keybox, with in-memory caching.
pub struct KeyboxKeyStore {
    /// The keybox client (authenticated with ai-proxy SVID).
    client: KeyboxClient,
    /// Cached keys per provider.
    cache: Arc<RwLock<HashMap<Provider, CachedKey>>>,
    /// Cache TTL.
    ttl: Duration,
}

impl KeyboxKeyStore {
    /// Creates a new key store with the given keybox client and cache TTL.
    #[must_use]
    pub fn new(client: KeyboxClient, ttl: Duration) -> Self {
        Self {
            client,
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl,
        }
    }

    /// Fetches a key from keybox (bypassing cache).
    async fn fetch_key(&self, provider: Provider) -> Option<SecretString> {
        let secret_name = provider.secret_name();
        debug!(provider = %provider, secret = secret_name, "fetching key from keybox");

        match self.client.get_secret(Scope::Service, secret_name).await {
            Ok(key) => {
                debug!(provider = %provider, "fetched key from keybox");
                Some(key)
            }
            Err(moto_keybox_client::Error::AccessDenied { .. }) => {
                warn!(provider = %provider, "key not found in keybox (provider not configured)");
                None
            }
            Err(e) => {
                error!(provider = %provider, error = %e, "failed to fetch key from keybox");
                None
            }
        }
    }
}

impl KeyStore for KeyboxKeyStore {
    async fn get_key(&self, provider: Provider) -> Option<SecretString> {
        // Check cache first.
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&provider)
                && entry.fetched_at.elapsed() < self.ttl
            {
                return Some(SecretString::from(entry.key.expose_secret().to_string()));
            }
        }

        // Cache miss or expired — fetch from keybox.
        let key = self.fetch_key(provider).await?;

        // Update cache.
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                provider,
                CachedKey {
                    key: SecretString::from(key.expose_secret().to_string()),
                    fetched_at: Instant::now(),
                },
            );
        }

        Some(key)
    }
}

/// Returns whether any provider key is cached (used by readiness check).
pub async fn has_cached_keys(store: &impl KeyStore) -> bool {
    // Try each provider — if any returns a key, we're ready.
    for provider in Provider::ALL {
        if store.get_key(provider).await.is_some() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock key store for testing.
    struct MockKeyStore {
        keys: HashMap<Provider, SecretString>,
    }

    impl MockKeyStore {
        fn new() -> Self {
            Self {
                keys: HashMap::new(),
            }
        }

        fn with_key(mut self, provider: Provider, key: &str) -> Self {
            self.keys
                .insert(provider, SecretString::from(key.to_string()));
            self
        }
    }

    impl KeyStore for MockKeyStore {
        async fn get_key(&self, provider: Provider) -> Option<SecretString> {
            self.keys
                .get(&provider)
                .map(|k| SecretString::from(k.expose_secret().to_string()))
        }
    }

    #[tokio::test]
    async fn mock_key_store_returns_configured_keys() {
        let store = MockKeyStore::new()
            .with_key(Provider::Anthropic, "sk-ant-test")
            .with_key(Provider::OpenAi, "sk-test");

        let key = store.get_key(Provider::Anthropic).await;
        assert!(key.is_some());
        assert_eq!(key.unwrap().expose_secret(), "sk-ant-test");

        let key = store.get_key(Provider::OpenAi).await;
        assert!(key.is_some());

        let key = store.get_key(Provider::Gemini).await;
        assert!(key.is_none());
    }

    #[tokio::test]
    async fn has_cached_keys_returns_true_when_key_exists() {
        let store = MockKeyStore::new().with_key(Provider::Anthropic, "sk-test");
        assert!(has_cached_keys(&store).await);
    }

    #[tokio::test]
    async fn has_cached_keys_returns_false_when_empty() {
        let store = MockKeyStore::new();
        assert!(!has_cached_keys(&store).await);
    }
}

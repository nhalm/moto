//! Provider registry — maps route prefixes to upstream AI providers.

use serde::Deserialize;

/// Known AI provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    /// Anthropic (Claude models).
    Anthropic,
    /// `OpenAI` (GPT, o1, o3 models).
    OpenAi,
    /// Google Gemini.
    Gemini,
}

impl Provider {
    /// Provider name as used in logs and error messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
        }
    }

    /// Upstream base URL (with trailing slash).
    #[must_use]
    pub const fn upstream_base(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com/",
            Self::OpenAi => "https://api.openai.com/",
            Self::Gemini => "https://generativelanguage.googleapis.com/",
        }
    }

    /// Keybox secret name for this provider (e.g. `ai-proxy/anthropic`).
    #[must_use]
    pub const fn secret_name(self) -> &'static str {
        match self {
            Self::Anthropic => "ai-proxy/anthropic",
            Self::OpenAi => "ai-proxy/openai",
            Self::Gemini => "ai-proxy/gemini",
        }
    }

    /// Auth header name for upstream requests.
    #[must_use]
    pub const fn auth_header(self) -> &'static str {
        match self {
            Self::Anthropic => "x-api-key",
            Self::OpenAi => "authorization",
            Self::Gemini => "x-goog-api-key",
        }
    }

    /// Formats the auth header value for upstream requests.
    #[must_use]
    pub fn auth_value(self, key: &str) -> String {
        match self {
            Self::OpenAi => format!("Bearer {key}"),
            Self::Anthropic | Self::Gemini => key.to_string(),
        }
    }

    /// All known providers.
    pub const ALL: [Self; 3] = [Self::Anthropic, Self::OpenAi, Self::Gemini];

    /// Resolves a provider from a model name based on prefix matching.
    ///
    /// Returns `None` if the model prefix is not recognized.
    #[must_use]
    pub fn from_model(model: &str) -> Option<Self> {
        let model = model.to_lowercase();
        if model.starts_with("claude-") {
            Some(Self::Anthropic)
        } else if model.starts_with("gpt-")
            || model.starts_with("o1-")
            || model.starts_with("o3-")
            || model.starts_with("chatgpt-")
        {
            Some(Self::OpenAi)
        } else if model.starts_with("gemini-") {
            Some(Self::Gemini)
        } else {
            None
        }
    }

    /// Returns the upstream path for the unified `/v1/chat/completions` endpoint.
    ///
    /// `OpenAI` uses its native path, Gemini uses its `OpenAI`-compat endpoint.
    /// Anthropic requires translation (handled separately).
    #[must_use]
    pub const fn unified_chat_path(self) -> &'static str {
        match self {
            Self::OpenAi => "v1/chat/completions",
            Self::Gemini => "v1beta/openai/chat/completions",
            Self::Anthropic => "v1/messages",
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Resolved provider information for routing and forwarding.
///
/// Produced by either a built-in `Provider` or a `CustomMapping`.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    /// Provider name (for logs, errors, display).
    pub name: String,
    /// Upstream base URL (with trailing slash).
    pub upstream_base: String,
    /// Auth header name for upstream requests.
    pub auth_header: String,
    /// Auth value prefix (e.g., `"Bearer "` or `""`).
    pub auth_prefix: String,
    /// Keybox secret name (e.g., `ai-proxy/anthropic`).
    pub secret_name: String,
    /// Whether this is the Anthropic provider (needs translation + `anthropic-version` header).
    pub is_anthropic: bool,
    /// Upstream path for unified chat endpoint.
    pub chat_path: String,
}

impl ProviderInfo {
    /// Formats the auth header value for upstream requests.
    #[must_use]
    pub fn auth_value(&self, key: &str) -> String {
        format!("{}{key}", self.auth_prefix)
    }
}

impl Provider {
    /// Converts this built-in provider to a `ProviderInfo`.
    #[must_use]
    pub fn info(self) -> ProviderInfo {
        ProviderInfo {
            name: self.name().to_string(),
            upstream_base: self.upstream_base().to_string(),
            auth_header: self.auth_header().to_string(),
            auth_prefix: match self {
                Self::OpenAi => "Bearer ".to_string(),
                Self::Anthropic | Self::Gemini => String::new(),
            },
            secret_name: self.secret_name().to_string(),
            is_anthropic: self == Self::Anthropic,
            chat_path: self.unified_chat_path().to_string(),
        }
    }
}

/// Custom model prefix → provider mapping from `MOTO_AI_PROXY_MODEL_MAP`.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomMapping {
    /// Model name prefix to match (e.g., `"mistral-"`).
    pub prefix: String,
    /// Provider name (used for secret lookup as `ai-proxy/{provider}`).
    pub provider: String,
    /// Upstream base URL (with trailing slash).
    pub upstream: String,
    /// Auth header name (e.g., `"Authorization"`).
    pub auth_header: String,
    /// Auth value prefix (e.g., `"Bearer "`).
    pub auth_prefix: String,
}

impl CustomMapping {
    fn to_info(&self) -> ProviderInfo {
        ProviderInfo {
            name: self.provider.clone(),
            upstream_base: self.upstream.clone(),
            auth_header: self.auth_header.clone(),
            auth_prefix: self.auth_prefix.clone(),
            secret_name: format!("ai-proxy/{}", self.provider),
            is_anthropic: false,
            chat_path: "v1/chat/completions".to_string(),
        }
    }
}

/// Model router: resolves model names to provider info.
///
/// Checks custom mappings first (from `MOTO_AI_PROXY_MODEL_MAP`),
/// then falls back to built-in provider prefix matching.
#[derive(Debug, Clone, Default)]
pub struct ModelRouter {
    custom: Vec<CustomMapping>,
}

impl ModelRouter {
    /// Creates a new model router, optionally parsing custom mappings from JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if `model_map_json` contains invalid JSON.
    pub fn new(model_map_json: Option<&str>) -> Result<Self, serde_json::Error> {
        let custom = match model_map_json {
            Some(json) => serde_json::from_str(json)?,
            None => Vec::new(),
        };
        Ok(Self { custom })
    }

    /// Resolves a model name to provider info.
    ///
    /// Checks custom mappings first, then built-in providers.
    #[must_use]
    pub fn resolve(&self, model: &str) -> Option<ProviderInfo> {
        let lower = model.to_lowercase();
        for mapping in &self.custom {
            if lower.starts_with(&mapping.prefix) {
                return Some(mapping.to_info());
            }
        }
        Provider::from_model(model).map(Provider::info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_model_routes_claude_to_anthropic() {
        assert_eq!(
            Provider::from_model("claude-sonnet-4-20250514"),
            Some(Provider::Anthropic)
        );
        assert_eq!(
            Provider::from_model("claude-3-haiku-20240307"),
            Some(Provider::Anthropic)
        );
    }

    #[test]
    fn from_model_routes_gpt_to_openai() {
        assert_eq!(Provider::from_model("gpt-4o"), Some(Provider::OpenAi));
        assert_eq!(Provider::from_model("gpt-4o-mini"), Some(Provider::OpenAi));
    }

    #[test]
    fn from_model_routes_o1_o3_chatgpt_to_openai() {
        assert_eq!(Provider::from_model("o1-preview"), Some(Provider::OpenAi));
        assert_eq!(Provider::from_model("o3-mini"), Some(Provider::OpenAi));
        assert_eq!(
            Provider::from_model("chatgpt-4o-latest"),
            Some(Provider::OpenAi)
        );
    }

    #[test]
    fn from_model_routes_gemini_to_gemini() {
        assert_eq!(
            Provider::from_model("gemini-1.5-pro"),
            Some(Provider::Gemini)
        );
        assert_eq!(
            Provider::from_model("gemini-2.0-flash"),
            Some(Provider::Gemini)
        );
    }

    #[test]
    fn from_model_returns_none_for_unknown() {
        assert_eq!(Provider::from_model("mistral-large"), None);
        assert_eq!(Provider::from_model("llama-3"), None);
        assert_eq!(Provider::from_model(""), None);
    }

    #[test]
    fn from_model_is_case_insensitive() {
        assert_eq!(
            Provider::from_model("Claude-Sonnet-4"),
            Some(Provider::Anthropic)
        );
        assert_eq!(Provider::from_model("GPT-4o"), Some(Provider::OpenAi));
        assert_eq!(
            Provider::from_model("GEMINI-2.0-flash"),
            Some(Provider::Gemini)
        );
    }

    #[test]
    fn unified_chat_path_correct_per_provider() {
        assert_eq!(Provider::OpenAi.unified_chat_path(), "v1/chat/completions");
        assert_eq!(
            Provider::Gemini.unified_chat_path(),
            "v1beta/openai/chat/completions"
        );
        assert_eq!(Provider::Anthropic.unified_chat_path(), "v1/messages");
    }

    #[test]
    fn provider_info_auth_value_with_prefix() {
        let info = Provider::OpenAi.info();
        assert_eq!(info.auth_value("sk-test"), "Bearer sk-test");
    }

    #[test]
    fn provider_info_auth_value_without_prefix() {
        let info = Provider::Anthropic.info();
        assert_eq!(info.auth_value("sk-ant-test"), "sk-ant-test");
    }

    #[test]
    fn provider_info_anthropic_flag() {
        assert!(Provider::Anthropic.info().is_anthropic);
        assert!(!Provider::OpenAi.info().is_anthropic);
        assert!(!Provider::Gemini.info().is_anthropic);
    }

    #[test]
    fn model_router_resolves_builtin_providers() {
        let router = ModelRouter::default();
        let info = router.resolve("claude-sonnet-4-20250514").unwrap();
        assert_eq!(info.name, "anthropic");
        assert!(info.is_anthropic);

        let info = router.resolve("gpt-4o").unwrap();
        assert_eq!(info.name, "openai");

        let info = router.resolve("gemini-2.0-flash").unwrap();
        assert_eq!(info.name, "gemini");
    }

    #[test]
    fn model_router_returns_none_for_unknown() {
        let router = ModelRouter::default();
        assert!(router.resolve("mistral-large").is_none());
    }

    #[test]
    fn model_router_custom_mapping_takes_priority() {
        let json = r#"[{"prefix": "mistral-", "provider": "mistral", "upstream": "https://api.mistral.ai/", "auth_header": "Authorization", "auth_prefix": "Bearer "}]"#;
        let router = ModelRouter::new(Some(json)).unwrap();

        let info = router.resolve("mistral-large").unwrap();
        assert_eq!(info.name, "mistral");
        assert_eq!(info.upstream_base, "https://api.mistral.ai/");
        assert_eq!(info.auth_header, "Authorization");
        assert_eq!(info.auth_value("sk-test"), "Bearer sk-test");
        assert_eq!(info.secret_name, "ai-proxy/mistral");
        assert!(!info.is_anthropic);
        assert_eq!(info.chat_path, "v1/chat/completions");
    }

    #[test]
    fn model_router_custom_does_not_shadow_builtin() {
        let json = r#"[{"prefix": "mistral-", "provider": "mistral", "upstream": "https://api.mistral.ai/", "auth_header": "Authorization", "auth_prefix": "Bearer "}]"#;
        let router = ModelRouter::new(Some(json)).unwrap();

        // Built-in providers still work.
        let info = router.resolve("claude-sonnet-4-20250514").unwrap();
        assert_eq!(info.name, "anthropic");
    }

    #[test]
    fn model_router_empty_json_array() {
        let router = ModelRouter::new(Some("[]")).unwrap();
        assert!(router.resolve("mistral-large").is_none());
        assert!(router.resolve("gpt-4o").is_some());
    }

    #[test]
    fn model_router_invalid_json_returns_error() {
        assert!(ModelRouter::new(Some("not json")).is_err());
    }
}

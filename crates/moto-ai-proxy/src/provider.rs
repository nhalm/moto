//! Provider registry — maps route prefixes to upstream AI providers.

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
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

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
}

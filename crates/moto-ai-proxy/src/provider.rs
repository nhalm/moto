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
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

//! moto-ai-proxy: AI provider reverse proxy for the moto platform.
//!
//! Routes requests from garages to AI providers (Anthropic, `OpenAI`, Gemini),
//! injecting API credentials from keybox so garages never see real API keys.

pub mod config;

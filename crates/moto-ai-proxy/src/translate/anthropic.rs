//! `OpenAI` → Anthropic request translation.
//!
//! Converts `OpenAI` `chat/completions` request format to Anthropic `messages` format.
//! System messages are extracted from the messages array and placed into the
//! top-level `system` parameter.

use serde_json::Value;

/// Translates an `OpenAI` chat completion request body into an Anthropic messages request body.
///
/// Field mapping:
/// - `model` → `model` (unchanged)
/// - `messages` → `messages` (system messages extracted to top-level `system`)
/// - `max_tokens` → `max_tokens` (direct)
/// - `temperature` → `temperature` (direct)
/// - `top_p` → `top_p` (direct)
/// - `stream` → `stream` (direct)
/// - `stop` → `stop_sequences` (rename)
/// - `tools` → `tools` (compatible schema)
/// - `tool_choice` → `tool_choice` (format translation)
#[must_use]
pub fn translate_request(openai_body: &Value) -> Value {
    let mut anthropic = serde_json::Map::new();

    // Pass through model unchanged.
    if let Some(model) = openai_body.get("model") {
        anthropic.insert("model".into(), model.clone());
    }

    // Extract system messages and separate from user/assistant messages.
    if let Some(Value::Array(messages)) = openai_body.get("messages") {
        let (system_parts, non_system): (Vec<_>, Vec<_>) = messages
            .iter()
            .partition(|m| m.get("role").and_then(Value::as_str) == Some("system"));

        // Build top-level system parameter from system messages.
        if !system_parts.is_empty() {
            let system_texts: Vec<&str> = system_parts
                .iter()
                .filter_map(|m| m.get("content").and_then(Value::as_str))
                .collect();
            if system_texts.len() == 1 {
                anthropic.insert("system".into(), Value::String(system_texts[0].to_string()));
            } else if system_texts.len() > 1 {
                anthropic.insert("system".into(), Value::String(system_texts.join("\n\n")));
            }
        }

        // Pass through non-system messages.
        anthropic.insert(
            "messages".into(),
            Value::Array(non_system.into_iter().cloned().collect()),
        );
    }

    // Direct mappings (pass through unchanged).
    for field in &["max_tokens", "temperature", "top_p", "stream"] {
        if let Some(v) = openai_body.get(*field) {
            anthropic.insert((*field).to_string(), v.clone());
        }
    }

    // Rename: stop → stop_sequences.
    if let Some(stop) = openai_body.get("stop") {
        // OpenAI accepts string or array; Anthropic expects array.
        let stop_sequences = match stop {
            Value::String(s) => Value::Array(vec![Value::String(s.clone())]),
            arr @ Value::Array(_) => arr.clone(),
            _ => stop.clone(),
        };
        anthropic.insert("stop_sequences".into(), stop_sequences);
    }

    // Tools: pass through (schemas are compatible).
    if let Some(tools) = openai_body.get("tools") {
        anthropic.insert("tools".into(), translate_tools(tools));
    }

    // Tool choice: translate format differences.
    if let Some(tool_choice) = openai_body.get("tool_choice")
        && let Some(translated) = translate_tool_choice(tool_choice)
    {
        anthropic.insert("tool_choice".into(), translated);
    }

    Value::Object(anthropic)
}

/// Translates `OpenAI` tools format to Anthropic tools format.
///
/// `OpenAI` wraps each tool as `{"type": "function", "function": {"name": ..., "description": ..., "parameters": ...}}`.
/// Anthropic uses `{"name": ..., "description": ..., "input_schema": ...}`.
fn translate_tools(tools: &Value) -> Value {
    match tools {
        Value::Array(tools) => Value::Array(
            tools
                .iter()
                .filter_map(|tool| {
                    let func = tool.get("function")?;
                    let mut anthropic_tool = serde_json::Map::new();
                    if let Some(name) = func.get("name") {
                        anthropic_tool.insert("name".into(), name.clone());
                    }
                    if let Some(desc) = func.get("description") {
                        anthropic_tool.insert("description".into(), desc.clone());
                    }
                    if let Some(params) = func.get("parameters") {
                        anthropic_tool.insert("input_schema".into(), params.clone());
                    }
                    Some(Value::Object(anthropic_tool))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Translates `OpenAI` `tool_choice` to Anthropic format.
///
/// `OpenAI`: `"auto"`, `"none"`, `"required"`, or `{"type": "function", "function": {"name": "..."}}`
/// Anthropic: `{"type": "auto"}`, `{"type": "any"}`, or `{"type": "tool", "name": "..."}`
fn translate_tool_choice(choice: &Value) -> Option<Value> {
    match choice {
        Value::String(s) => match s.as_str() {
            "auto" => Some(serde_json::json!({"type": "auto"})),
            "required" => Some(serde_json::json!({"type": "any"})),
            // "none" and unknown strings: Anthropic has no equivalent; omit tool_choice.
            _ => None,
        },
        Value::Object(obj) => {
            // {"type": "function", "function": {"name": "tool_name"}}
            // → {"type": "tool", "name": "tool_name"}
            let name = obj
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)?;
            Some(serde_json::json!({"type": "tool", "name": name}))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn basic_request_translation() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 1024,
            "temperature": 0.7,
            "stream": false
        });

        let result = translate_request(&openai);
        assert_eq!(result["model"], "claude-sonnet-4-20250514");
        assert_eq!(result["max_tokens"], 1024);
        assert_eq!(result["temperature"], 0.7);
        assert_eq!(result["stream"], false);

        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello");

        // No system field when no system messages.
        assert!(result.get("system").is_none());
    }

    #[test]
    fn system_message_extraction() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = translate_request(&openai);
        assert_eq!(result["system"], "You are helpful.");

        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn multiple_system_messages_concatenated() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "system", "content": "First instruction."},
                {"role": "system", "content": "Second instruction."},
                {"role": "user", "content": "Hello"}
            ]
        });

        let result = translate_request(&openai);
        assert_eq!(
            result["system"],
            "First instruction.\n\nSecond instruction."
        );
    }

    #[test]
    fn stop_string_becomes_array() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "stop": "END"
        });

        let result = translate_request(&openai);
        assert_eq!(result["stop_sequences"], json!(["END"]));
    }

    #[test]
    fn stop_array_passed_through() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "stop": ["END", "STOP"]
        });

        let result = translate_request(&openai);
        assert_eq!(result["stop_sequences"], json!(["END", "STOP"]));
    }

    #[test]
    fn top_p_passed_through() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "top_p": 0.9
        });

        let result = translate_request(&openai);
        assert_eq!(result["top_p"], 0.9);
    }

    #[test]
    fn tools_translated_from_openai_to_anthropic() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather for a location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {"type": "string"}
                            },
                            "required": ["location"]
                        }
                    }
                }
            ]
        });

        let result = translate_request(&openai);
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["description"], "Get weather for a location");
        assert!(tools[0]["input_schema"]["properties"]["location"].is_object());
        // OpenAI wrapping should be removed.
        assert!(tools[0].get("type").is_none());
        assert!(tools[0].get("function").is_none());
    }

    #[test]
    fn tool_choice_auto_translated() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "auto"
        });
        let result = translate_request(&openai);
        assert_eq!(result["tool_choice"], json!({"type": "auto"}));
    }

    #[test]
    fn tool_choice_required_becomes_any() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "required"
        });
        let result = translate_request(&openai);
        assert_eq!(result["tool_choice"], json!({"type": "any"}));
    }

    #[test]
    fn tool_choice_none_omitted() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": "none"
        });
        let result = translate_request(&openai);
        assert!(result.get("tool_choice").is_none());
    }

    #[test]
    fn tool_choice_specific_function() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "tool_choice": {
                "type": "function",
                "function": {"name": "get_weather"}
            }
        });
        let result = translate_request(&openai);
        assert_eq!(
            result["tool_choice"],
            json!({"type": "tool", "name": "get_weather"})
        );
    }

    #[test]
    fn provider_specific_fields_stripped() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{"role": "user", "content": "Hi"}],
            "logprobs": true,
            "frequency_penalty": 0.5,
            "presence_penalty": 0.3
        });
        let result = translate_request(&openai);
        // Provider-specific fields should not appear in the translated output.
        assert!(result.get("logprobs").is_none());
        assert!(result.get("frequency_penalty").is_none());
        assert!(result.get("presence_penalty").is_none());
    }
}

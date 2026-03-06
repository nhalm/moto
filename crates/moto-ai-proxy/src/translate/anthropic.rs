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

        // Translate non-system messages, handling tool_calls and tool role.
        let translated = translate_messages(&non_system);
        anthropic.insert("messages".into(), Value::Array(translated));
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

/// Translates `OpenAI` messages to Anthropic message format.
///
/// Handles:
/// - Assistant messages with `tool_calls` → Anthropic `tool_use` content blocks
/// - `tool` role messages → Anthropic `user` messages with `tool_result` content blocks
/// - Consecutive `tool` messages merged into a single `user` message
/// - Regular messages passed through unchanged
fn translate_messages(messages: &[&Value]) -> Vec<Value> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        let msg = messages[i];
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");

        match role {
            "assistant" if msg.get("tool_calls").is_some() => {
                result.push(translate_assistant_tool_calls(msg));
                i += 1;
            }
            "tool" => {
                // Merge consecutive tool messages into a single user message.
                let mut tool_results = Vec::new();
                while i < messages.len()
                    && messages[i].get("role").and_then(Value::as_str) == Some("tool")
                {
                    tool_results.push(translate_tool_result(messages[i]));
                    i += 1;
                }
                result.push(serde_json::json!({
                    "role": "user",
                    "content": tool_results
                }));
            }
            _ => {
                result.push(msg.clone());
                i += 1;
            }
        }
    }

    result
}

/// Translates an `OpenAI` assistant message with `tool_calls` to Anthropic format.
///
/// `OpenAI`: `{"role": "assistant", "content": "text", "tool_calls": [...]}`
/// Anthropic: `{"role": "assistant", "content": [{"type": "text", ...}, {"type": "tool_use", ...}]}`
fn translate_assistant_tool_calls(msg: &Value) -> Value {
    let mut content_blocks = Vec::new();

    // Include text content block if present and non-empty.
    if let Some(text) = msg.get("content").and_then(Value::as_str)
        && !text.is_empty()
    {
        content_blocks.push(serde_json::json!({"type": "text", "text": text}));
    }

    // Convert each tool_call to a tool_use content block.
    if let Some(Value::Array(tool_calls)) = msg.get("tool_calls") {
        for tc in tool_calls {
            let func = tc.get("function");
            let id = tc.get("id").and_then(Value::as_str).unwrap_or("");
            let name = func
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = func
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let input: Value = serde_json::from_str(arguments)
                .unwrap_or_else(|_| Value::Object(serde_json::Map::default()));

            content_blocks.push(serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }));
        }
    }

    serde_json::json!({
        "role": "assistant",
        "content": content_blocks
    })
}

/// Translates an `OpenAI` `tool` role message to an Anthropic `tool_result` content block.
///
/// `OpenAI`: `{"role": "tool", "tool_call_id": "call_abc", "content": "result"}`
/// Anthropic: `{"type": "tool_result", "tool_use_id": "call_abc", "content": "result"}`
fn translate_tool_result(msg: &Value) -> Value {
    let tool_use_id = msg
        .get("tool_call_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let content = msg.get("content").and_then(Value::as_str).unwrap_or("");

    serde_json::json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": content
    })
}

/// Translates an Anthropic non-streaming response into an `OpenAI` chat completion response.
///
/// Field mapping:
/// - `id` → `id` (passed through)
/// - `content[*].text` → `choices[0].message.content` (text blocks concatenated)
/// - `model` → `model` (passed through)
/// - `stop_reason` → `choices[0].finish_reason` (mapped: `end_turn`→`stop`, `max_tokens`→`length`, `tool_use`→`tool_calls`)
/// - `usage.input_tokens` → `usage.prompt_tokens` (rename)
/// - `usage.output_tokens` → `usage.completion_tokens` (rename)
#[must_use]
pub fn translate_response(anthropic_body: &Value) -> Value {
    let mut openai = serde_json::Map::new();

    // Pass through id.
    if let Some(id) = anthropic_body.get("id") {
        openai.insert("id".into(), id.clone());
    }

    openai.insert("object".into(), Value::String("chat.completion".into()));

    // Pass through model.
    if let Some(model) = anthropic_body.get("model") {
        openai.insert("model".into(), model.clone());
    }

    // Build choices[0].message from content blocks.
    let content = extract_text_content(anthropic_body);
    let tool_calls = extract_tool_calls(anthropic_body);
    let finish_reason = translate_stop_reason(anthropic_body);

    let mut message = serde_json::Map::new();
    message.insert("role".into(), Value::String("assistant".into()));

    if tool_calls.is_empty() {
        message.insert("content".into(), Value::String(content));
    } else {
        // When tool calls are present, content is null if empty, or the text.
        if content.is_empty() {
            message.insert("content".into(), Value::Null);
        } else {
            message.insert("content".into(), Value::String(content));
        }
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }

    let mut choice = serde_json::Map::new();
    choice.insert("index".into(), Value::Number(0.into()));
    choice.insert("message".into(), Value::Object(message));
    choice.insert("finish_reason".into(), finish_reason);

    openai.insert("choices".into(), Value::Array(vec![Value::Object(choice)]));

    // Translate usage: input_tokens → prompt_tokens, output_tokens → completion_tokens.
    if let Some(Value::Object(usage)) = anthropic_body.get("usage") {
        let mut openai_usage = serde_json::Map::new();
        if let Some(input) = usage.get("input_tokens") {
            openai_usage.insert("prompt_tokens".into(), input.clone());
        }
        if let Some(output) = usage.get("output_tokens") {
            openai_usage.insert("completion_tokens".into(), output.clone());
        }
        // Add total_tokens for OpenAI compat.
        let total = usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + usage
                .get("output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
        openai_usage.insert("total_tokens".into(), Value::Number(total.into()));
        openai.insert("usage".into(), Value::Object(openai_usage));
    }

    Value::Object(openai)
}

/// A parsed SSE event with its event type and data payload.
pub struct SseEvent {
    /// The `event:` field (e.g., `message_start`, `content_block_delta`).
    pub event_type: String,
    /// The `data:` field (raw JSON string).
    pub data: String,
}

/// Parses a raw SSE event block (lines between double newlines) into an `SseEvent`.
///
/// Returns `None` if the block has no `data:` field.
#[must_use]
pub fn parse_sse_event(raw: &str) -> Option<SseEvent> {
    let mut event_type = String::new();
    let mut data_parts = Vec::new();

    for line in raw.lines() {
        if let Some(val) = line.strip_prefix("event:") {
            event_type = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("data:") {
            data_parts.push(val.trim().to_string());
        }
    }

    if data_parts.is_empty() {
        return None;
    }

    Some(SseEvent {
        event_type,
        data: data_parts.join("\n"),
    })
}

/// Stateful streaming translator that converts Anthropic SSE events to `OpenAI` format.
///
/// Tracks tool call state across events to properly index tool calls in the
/// `OpenAI` streaming format.
pub struct StreamingTranslator {
    /// Number of tool calls seen so far (used for `OpenAI` `tool_calls` array index).
    tool_call_index: usize,
}

impl StreamingTranslator {
    /// Creates a new streaming translator.
    #[must_use]
    pub const fn new() -> Self {
        Self { tool_call_index: 0 }
    }

    /// Translates an Anthropic SSE event into an `OpenAI` SSE chunk string.
    ///
    /// Returns the raw SSE text to emit (including `data: ` prefix and trailing newlines),
    /// or `None` if the event should be silently dropped.
    ///
    /// Mapping:
    /// - `message_start` → initial chunk with `role: "assistant"`
    /// - `content_block_start` (`tool_use`) → initial tool call chunk with function name
    /// - `content_block_delta` (text) → `choices[0].delta.content`
    /// - `content_block_delta` (`input_json_delta`) → `choices[0].delta.tool_calls` argument delta
    /// - `message_delta` (`stop_reason`) → `choices[0].finish_reason`
    /// - `message_stop` → `data: [DONE]`
    /// - Other events (ping, `content_block_stop`) → dropped
    pub fn translate_event(&mut self, event: &SseEvent) -> Option<String> {
        match event.event_type.as_str() {
            "message_start" => translate_message_start(event),
            "content_block_start" => self.translate_block_start(event),
            "content_block_delta" => self.translate_block_delta(event),
            "message_delta" => translate_message_delta(event),
            "message_stop" => Some("data: [DONE]\n\n".to_string()),
            // Drop events we don't translate (ping, content_block_stop).
            _ => None,
        }
    }

    fn translate_block_start(&mut self, event: &SseEvent) -> Option<String> {
        let data: Value = serde_json::from_str(&event.data).ok()?;
        let block = data.get("content_block")?;
        let block_type = block.get("type").and_then(Value::as_str)?;

        if block_type != "tool_use" {
            return None;
        }

        let id = block.get("id").and_then(Value::as_str).unwrap_or("");
        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
        let index = self.tool_call_index;
        self.tool_call_index += 1;

        let chunk = serde_json::json!({
            "id": "chatcmpl-stream",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": index,
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": ""}
                    }]
                },
                "finish_reason": null
            }]
        });
        Some(format!("data: {chunk}\n\n"))
    }

    fn translate_block_delta(&self, event: &SseEvent) -> Option<String> {
        let data: Value = serde_json::from_str(&event.data).ok()?;
        let delta = data.get("delta")?;
        let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or("");

        match delta_type {
            "text_delta" => {
                let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                let chunk = serde_json::json!({
                    "id": "chatcmpl-stream",
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {"content": text},
                        "finish_reason": null
                    }]
                });
                Some(format!("data: {chunk}\n\n"))
            }
            "input_json_delta" => {
                let partial = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let index = self.tool_call_index.saturating_sub(1);
                let chunk = serde_json::json!({
                    "id": "chatcmpl-stream",
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": index,
                                "function": {"arguments": partial}
                            }]
                        },
                        "finish_reason": null
                    }]
                });
                Some(format!("data: {chunk}\n\n"))
            }
            _ => None,
        }
    }
}

impl Default for StreamingTranslator {
    fn default() -> Self {
        Self::new()
    }
}

/// Translates an Anthropic `message_start` SSE event to `OpenAI` initial chunk.
fn translate_message_start(event: &SseEvent) -> Option<String> {
    let data: Value = serde_json::from_str(&event.data).ok()?;
    let message = data.get("message");

    let id = message
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl-stream");
    let model = message
        .and_then(|m| m.get("model"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let chunk = serde_json::json!({
        "id": id,
        "object": "chat.completion.chunk",
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant", "content": ""},
            "finish_reason": null
        }]
    });
    Some(format!("data: {chunk}\n\n"))
}

/// Translates an Anthropic `message_delta` SSE event to `OpenAI` finish reason chunk.
fn translate_message_delta(event: &SseEvent) -> Option<String> {
    let data: Value = serde_json::from_str(&event.data).ok()?;
    let delta = data.get("delta")?;
    let stop_reason = delta.get("stop_reason").and_then(Value::as_str)?;

    let finish_reason = match stop_reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        other => other,
    };

    let chunk = serde_json::json!({
        "id": "chatcmpl-stream",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": finish_reason
        }]
    });
    Some(format!("data: {chunk}\n\n"))
}

/// Extracts and concatenates text from Anthropic content blocks.
fn extract_text_content(body: &Value) -> String {
    match body.get("content") {
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(Value::as_str) == Some("text") {
                    block.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// Extracts `tool_use` blocks from Anthropic content and converts to `OpenAI` `tool_calls` format.
///
/// Anthropic: `{"type": "tool_use", "id": "tu_1", "name": "get_weather", "input": {...}}`
/// `OpenAI`: `{"id": "tu_1", "type": "function", "function": {"name": "get_weather", "arguments": "..."}}`
fn extract_tool_calls(body: &Value) -> Vec<Value> {
    match body.get("content") {
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    return None;
                }
                let id = block.get("id").and_then(Value::as_str).unwrap_or("");
                let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                let default_input = Value::Object(serde_json::Map::default());
                let input = block.get("input").unwrap_or(&default_input);
                let arguments = serde_json::to_string(input).unwrap_or_default();

                Some(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments
                    }
                }))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Maps Anthropic `stop_reason` to `OpenAI` `finish_reason`.
fn translate_stop_reason(body: &Value) -> Value {
    match body.get("stop_reason").and_then(Value::as_str) {
        Some("end_turn") => Value::String("stop".into()),
        Some("max_tokens") => Value::String("length".into()),
        Some("tool_use") => Value::String("tool_calls".into()),
        Some(other) => Value::String(other.into()),
        None => Value::Null,
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

    #[test]
    fn basic_response_translation() {
        let anthropic = json!({
            "id": "msg_123",
            "type": "message",
            "content": [{"type": "text", "text": "Hello world"}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 25
            }
        });

        let result = translate_response(&anthropic);
        assert_eq!(result["id"], "msg_123");
        assert_eq!(result["object"], "chat.completion");
        assert_eq!(result["model"], "claude-sonnet-4-20250514");
        assert_eq!(result["choices"][0]["index"], 0);
        assert_eq!(result["choices"][0]["message"]["role"], "assistant");
        assert_eq!(result["choices"][0]["message"]["content"], "Hello world");
        assert_eq!(result["choices"][0]["finish_reason"], "stop");
        assert_eq!(result["usage"]["prompt_tokens"], 10);
        assert_eq!(result["usage"]["completion_tokens"], 25);
        assert_eq!(result["usage"]["total_tokens"], 35);
    }

    #[test]
    fn stop_reason_max_tokens_maps_to_length() {
        let anthropic = json!({
            "id": "msg_456",
            "content": [{"type": "text", "text": "Truncated"}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "max_tokens"
        });

        let result = translate_response(&anthropic);
        assert_eq!(result["choices"][0]["finish_reason"], "length");
    }

    #[test]
    fn stop_reason_tool_use_maps_to_tool_calls() {
        let anthropic = json!({
            "id": "msg_789",
            "content": [{"type": "text", "text": ""}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "tool_use"
        });

        let result = translate_response(&anthropic);
        assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn multiple_text_blocks_concatenated() {
        let anthropic = json!({
            "id": "msg_multi",
            "content": [
                {"type": "text", "text": "Hello "},
                {"type": "text", "text": "world"}
            ],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn"
        });

        let result = translate_response(&anthropic);
        assert_eq!(result["choices"][0]["message"]["content"], "Hello world");
    }

    #[test]
    fn missing_usage_omitted() {
        let anthropic = json!({
            "id": "msg_no_usage",
            "content": [{"type": "text", "text": "Hi"}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn"
        });

        let result = translate_response(&anthropic);
        assert!(result.get("usage").is_none());
    }

    #[test]
    fn streaming_message_start_translates_to_initial_chunk() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "message_start".into(),
            data: r#"{"type":"message_start","message":{"id":"msg_123","model":"claude-sonnet-4-20250514","role":"assistant"}}"#.into(),
        };
        let result = t.translate_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed["object"], "chat.completion.chunk");
        assert_eq!(parsed["id"], "msg_123");
        assert_eq!(parsed["model"], "claude-sonnet-4-20250514");
        assert_eq!(parsed["choices"][0]["delta"]["role"], "assistant");
        assert!(parsed["choices"][0]["finish_reason"].is_null());
    }

    #[test]
    fn streaming_text_delta_translates_to_content_chunk() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "content_block_delta".into(),
            data: r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#.into(),
        };
        let result = t.translate_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed["choices"][0]["delta"]["content"], "Hello");
        assert!(parsed["choices"][0]["finish_reason"].is_null());
    }

    #[test]
    fn streaming_message_delta_translates_stop_reason() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "message_delta".into(),
            data: r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#.into(),
        };
        let result = t.translate_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn streaming_message_delta_max_tokens_maps_to_length() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "message_delta".into(),
            data: r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#.into(),
        };
        let result = t.translate_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed["choices"][0]["finish_reason"], "length");
    }

    #[test]
    fn streaming_message_stop_emits_done() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "message_stop".into(),
            data: r#"{"type":"message_stop"}"#.into(),
        };
        let result = t.translate_event(&event).unwrap();
        assert_eq!(result, "data: [DONE]\n\n");
    }

    #[test]
    fn streaming_ping_event_dropped() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "ping".into(),
            data: r#"{"type":"ping"}"#.into(),
        };
        assert!(t.translate_event(&event).is_none());
    }

    #[test]
    fn streaming_text_content_block_start_dropped() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "content_block_start".into(),
            data: r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.into(),
        };
        assert!(t.translate_event(&event).is_none());
    }

    #[test]
    fn parse_sse_event_basic() {
        let raw = "event: message_start\ndata: {\"type\":\"message_start\"}";
        let event = parse_sse_event(raw).unwrap();
        assert_eq!(event.event_type, "message_start");
        assert_eq!(event.data, "{\"type\":\"message_start\"}");
    }

    #[test]
    fn parse_sse_event_no_data_returns_none() {
        let raw = "event: ping";
        assert!(parse_sse_event(raw).is_none());
    }

    // --- Tool use request translation tests ---

    #[test]
    fn assistant_tool_calls_translated_to_anthropic_tool_use() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": "What's the weather?"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"NYC\"}"
                        }
                    }]
                },
                {"role": "tool", "tool_call_id": "call_abc", "content": "72°F, sunny"},
                {"role": "user", "content": "Thanks!"}
            ]
        });

        let result = translate_request(&openai);
        let messages = result["messages"].as_array().unwrap();

        // Message 0: user (unchanged)
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "What's the weather?");

        // Message 1: assistant with tool_use content blocks
        assert_eq!(messages[1]["role"], "assistant");
        let content = messages[1]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "tool_use");
        assert_eq!(content[0]["id"], "call_abc");
        assert_eq!(content[0]["name"], "get_weather");
        assert_eq!(content[0]["input"]["location"], "NYC");

        // Message 2: tool result merged into user message
        assert_eq!(messages[2]["role"], "user");
        let tool_results = messages[2]["content"].as_array().unwrap();
        assert_eq!(tool_results.len(), 1);
        assert_eq!(tool_results[0]["type"], "tool_result");
        assert_eq!(tool_results[0]["tool_use_id"], "call_abc");
        assert_eq!(tool_results[0]["content"], "72°F, sunny");

        // Message 3: user (unchanged)
        assert_eq!(messages[3]["role"], "user");
        assert_eq!(messages[3]["content"], "Thanks!");
    }

    #[test]
    fn consecutive_tool_messages_merged_into_single_user() {
        let openai = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": "Weather and time?"},
                {
                    "role": "assistant",
                    "content": "Let me check both.",
                    "tool_calls": [
                        {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"location\":\"NYC\"}"}},
                        {"id": "call_2", "type": "function", "function": {"name": "get_time", "arguments": "{\"timezone\":\"EST\"}"}}
                    ]
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "72°F"},
                {"role": "tool", "tool_call_id": "call_2", "content": "3:00 PM"}
            ]
        });

        let result = translate_request(&openai);
        let messages = result["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);

        // Assistant message has text + 2 tool_use blocks
        let assistant_content = messages[1]["content"].as_array().unwrap();
        assert_eq!(assistant_content.len(), 3); // text + 2 tool_use
        assert_eq!(assistant_content[0]["type"], "text");
        assert_eq!(assistant_content[0]["text"], "Let me check both.");
        assert_eq!(assistant_content[1]["type"], "tool_use");
        assert_eq!(assistant_content[2]["type"], "tool_use");

        // Both tool results merged into one user message
        let tool_results = messages[2]["content"].as_array().unwrap();
        assert_eq!(tool_results.len(), 2);
        assert_eq!(tool_results[0]["tool_use_id"], "call_1");
        assert_eq!(tool_results[1]["tool_use_id"], "call_2");
    }

    // --- Tool use response translation tests ---

    #[test]
    fn response_tool_use_translated_to_openai_tool_calls() {
        let anthropic = json!({
            "id": "msg_tool",
            "content": [
                {"type": "text", "text": "Let me check."},
                {"type": "tool_use", "id": "tu_1", "name": "get_weather", "input": {"location": "NYC"}}
            ],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "tool_use"
        });

        let result = translate_response(&anthropic);
        assert_eq!(result["choices"][0]["message"]["content"], "Let me check.");
        assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");

        let tool_calls = result["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["id"], "tu_1");
        assert_eq!(tool_calls[0]["type"], "function");
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
        let args: Value =
            serde_json::from_str(tool_calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["location"], "NYC");
    }

    #[test]
    fn response_tool_use_only_sets_content_null() {
        let anthropic = json!({
            "id": "msg_tool_only",
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "get_weather", "input": {"location": "NYC"}}
            ],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "tool_use"
        });

        let result = translate_response(&anthropic);
        assert!(result["choices"][0]["message"]["content"].is_null());
        assert_eq!(
            result["choices"][0]["message"]["tool_calls"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn response_multiple_tool_uses() {
        let anthropic = json!({
            "id": "msg_multi_tool",
            "content": [
                {"type": "tool_use", "id": "tu_1", "name": "get_weather", "input": {"location": "NYC"}},
                {"type": "tool_use", "id": "tu_2", "name": "get_time", "input": {"timezone": "EST"}}
            ],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "tool_use"
        });

        let result = translate_response(&anthropic);
        let tool_calls = result["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
        assert_eq!(tool_calls[1]["function"]["name"], "get_time");
    }

    // --- Streaming tool use tests ---

    #[test]
    fn streaming_tool_use_content_block_start() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "content_block_start".into(),
            data: r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"get_weather"}}"#.into(),
        };
        let result = t.translate_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        let tc = &parsed["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["index"], 0);
        assert_eq!(tc["id"], "tu_1");
        assert_eq!(tc["type"], "function");
        assert_eq!(tc["function"]["name"], "get_weather");
        assert_eq!(tc["function"]["arguments"], "");
    }

    #[test]
    fn streaming_input_json_delta() {
        let mut t = StreamingTranslator::new();
        // First emit a content_block_start to set up the index.
        let start = SseEvent {
            event_type: "content_block_start".into(),
            data: r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"get_weather"}}"#.into(),
        };
        t.translate_event(&start);

        let event = SseEvent {
            event_type: "content_block_delta".into(),
            data: r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"loc"}}"#.into(),
        };
        let result = t.translate_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        let tc = &parsed["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(tc["index"], 0);
        assert_eq!(tc["function"]["arguments"], "{\"loc");
    }

    #[test]
    fn streaming_multiple_tool_calls_indexed_correctly() {
        let mut t = StreamingTranslator::new();

        // First tool call
        let start1 = SseEvent {
            event_type: "content_block_start".into(),
            data: r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"get_weather"}}"#.into(),
        };
        let result1 = t.translate_event(&start1).unwrap();
        let parsed1: Value =
            serde_json::from_str(result1.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed1["choices"][0]["delta"]["tool_calls"][0]["index"], 0);

        // Second tool call
        let start2 = SseEvent {
            event_type: "content_block_start".into(),
            data: r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"tu_2","name":"get_time"}}"#.into(),
        };
        let result2 = t.translate_event(&start2).unwrap();
        let parsed2: Value =
            serde_json::from_str(result2.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed2["choices"][0]["delta"]["tool_calls"][0]["index"], 1);

        // Delta for second tool call
        let delta2 = SseEvent {
            event_type: "content_block_delta".into(),
            data: r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"tz"}}"#.into(),
        };
        let result_d2 = t.translate_event(&delta2).unwrap();
        let parsed_d2: Value =
            serde_json::from_str(result_d2.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(
            parsed_d2["choices"][0]["delta"]["tool_calls"][0]["index"],
            1
        );
    }

    #[test]
    fn streaming_tool_use_stop_reason() {
        let mut t = StreamingTranslator::new();
        let event = SseEvent {
            event_type: "message_delta".into(),
            data: r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#.into(),
        };
        let result = t.translate_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed["choices"][0]["finish_reason"], "tool_calls");
    }
}

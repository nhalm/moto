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
    let finish_reason = translate_stop_reason(anthropic_body);

    let mut message = serde_json::Map::new();
    message.insert("role".into(), Value::String("assistant".into()));
    message.insert("content".into(), Value::String(content));

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

/// Translates an Anthropic SSE event into an `OpenAI` SSE chunk string.
///
/// Returns the raw SSE text to emit (including `data: ` prefix and trailing newlines),
/// or `None` if the event should be silently dropped.
///
/// Mapping:
/// - `message_start` → initial chunk with `role: "assistant"`
/// - `content_block_delta` (text) → `choices[0].delta.content`
/// - `message_delta` (`stop_reason`) → `choices[0].finish_reason`
/// - `message_stop` → `data: [DONE]`
/// - Other events (ping, `content_block_start`, `content_block_stop`) → dropped
#[must_use]
pub fn translate_streaming_event(event: &SseEvent) -> Option<String> {
    match event.event_type.as_str() {
        "message_start" => {
            // Parse message_start to extract model and id if available.
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
        "content_block_delta" => {
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
                    // Tool use argument streaming — will be handled by tool_use translation task.
                    None
                }
                _ => None,
            }
        }
        "message_delta" => {
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
        "message_stop" => Some("data: [DONE]\n\n".to_string()),
        // Drop events we don't translate (ping, content_block_start, content_block_stop).
        _ => None,
    }
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
        let event = SseEvent {
            event_type: "message_start".into(),
            data: r#"{"type":"message_start","message":{"id":"msg_123","model":"claude-sonnet-4-20250514","role":"assistant"}}"#.into(),
        };
        let result = translate_streaming_event(&event).unwrap();
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
        let event = SseEvent {
            event_type: "content_block_delta".into(),
            data: r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#.into(),
        };
        let result = translate_streaming_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed["choices"][0]["delta"]["content"], "Hello");
        assert!(parsed["choices"][0]["finish_reason"].is_null());
    }

    #[test]
    fn streaming_message_delta_translates_stop_reason() {
        let event = SseEvent {
            event_type: "message_delta".into(),
            data: r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#.into(),
        };
        let result = translate_streaming_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed["choices"][0]["finish_reason"], "stop");
    }

    #[test]
    fn streaming_message_delta_max_tokens_maps_to_length() {
        let event = SseEvent {
            event_type: "message_delta".into(),
            data: r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#.into(),
        };
        let result = translate_streaming_event(&event).unwrap();
        let parsed: Value =
            serde_json::from_str(result.strip_prefix("data: ").unwrap().trim()).unwrap();
        assert_eq!(parsed["choices"][0]["finish_reason"], "length");
    }

    #[test]
    fn streaming_message_stop_emits_done() {
        let event = SseEvent {
            event_type: "message_stop".into(),
            data: r#"{"type":"message_stop"}"#.into(),
        };
        let result = translate_streaming_event(&event).unwrap();
        assert_eq!(result, "data: [DONE]\n\n");
    }

    #[test]
    fn streaming_ping_event_dropped() {
        let event = SseEvent {
            event_type: "ping".into(),
            data: r#"{"type":"ping"}"#.into(),
        };
        assert!(translate_streaming_event(&event).is_none());
    }

    #[test]
    fn streaming_content_block_start_dropped() {
        let event = SseEvent {
            event_type: "content_block_start".into(),
            data: r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.into(),
        };
        assert!(translate_streaming_event(&event).is_none());
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

    #[test]
    fn non_text_blocks_ignored() {
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
        // Only text blocks are extracted for message content.
        assert_eq!(result["choices"][0]["message"]["content"], "Let me check.");
    }
}

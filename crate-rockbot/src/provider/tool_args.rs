//! Tool-call arguments JSON repair engine.
//!
//! Shared by every AI provider (DeepSeek, OpenRouter, llama.cpp) and the
//! agent harness. Models occasionally emit tool call `arguments` documents
//! that are not parseable JSON:
//!
//! - **Truncated output** — a length-limited response (max_tokens / context
//!   cap) cuts the document mid-string, leaving an unterminated string value
//!   (Gitea issue #80: `[json.exception.parse_error.101] ... invalid string:
//!   missing closing quote` at column ~11057 for an 11K-char report argument).
//! - **Unescaped quotes** — long free-text values (Markdown reports embedding
//!   JSON code blocks) contain raw `"` characters that break the outer JSON
//!   structure.
//! - **Raw control characters** — literal newlines/tabs inside string values.
//!
//! The repair is string-state aware: it tracks in-string/escape state while
//! scanning, so quotes and braces *inside string values* are never misread as
//! JSON structure (the old brace/quote-parity heuristic silently produced
//! wrong JSON for embedded code blocks). Documents that cannot be repaired are
//! reset to `{}` — valid JSON, at worst a no-op tool call.

use tracing::warn;

use crate::error::RockBotError;
use crate::types::ChatMessage;

/// Repair malformed tool call `arguments` JSON into a parseable document.
///
/// Returns the input unchanged when it already parses. When the document ends
/// inside a string value (truncation point), closes the string (neutralizing
/// a trailing lone backslash), escapes raw control characters found inside
/// strings, balances braces/brackets outside strings in nesting order, then
/// validates. Falls back to `"{}"` when repair is impossible (e.g. unescaped
/// embedded quotes make the truncation point ambiguous).
pub fn repair_tool_args(name: &str, args: &str) -> String {
    if args.is_empty() {
        warn!("Tool call arguments for fn={name} are empty, resetting to {{}}");
        return "{}".to_string();
    }

    if serde_json::from_str::<serde_json::Value>(args).is_ok() {
        return args.to_string();
    }

    // Pass 1: copy the input, closing an unterminated string at EOF and
    // escaping raw control characters inside string values. String-state
    // tracking handles `\"` escapes, so embedded quotes that ARE escaped
    // stay inside their string. A backslash is buffered (pending) until its
    // escaped char is known: invalid escapes (e.g. a lone `\` before a raw
    // newline, or unescaped Windows paths) are emitted as literal backslashes
    // instead of being dropped.
    let mut fixed = String::with_capacity(args.len() + 8);
    let mut in_string = false;
    let mut pending_backslash = false;
    for c in args.chars() {
        if in_string {
            if pending_backslash {
                if matches!(c, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') {
                    fixed.push('\\');
                    fixed.push(c);
                } else if c.is_control() {
                    // `\` before a raw control char cannot be a valid escape —
                    // keep the backslash literal, then escape the control char.
                    fixed.push_str("\\\\");
                    fixed.push_str(escape_control(c));
                } else {
                    // Invalid escape sequence (e.g. `\q`) — keep the backslash
                    // literal by doubling it.
                    fixed.push_str("\\\\");
                    fixed.push(c);
                }
                pending_backslash = false;
            } else if c == '\\' {
                pending_backslash = true;
            } else if c == '"' {
                fixed.push(c);
                in_string = false;
            } else if c.is_control() {
                fixed.push_str(escape_control(c));
            } else {
                fixed.push(c);
            }
        } else {
            fixed.push(c);
            if c == '"' {
                in_string = true;
            }
        }
    }
    if in_string {
        if pending_backslash {
            // Trailing lone backslash inside a string — emit it literally so
            // the closing quote terminates the string.
            fixed.push_str("\\\\");
        }
        fixed.push('"');
    }

    // Pass 2: balance braces/brackets that sit outside strings, closing them
    // in nesting order (stack).
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for c in fixed.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '"' => in_string = true,
                '{' => stack.push('}'),
                '[' => stack.push(']'),
                '}' | ']' => {
                    if stack.last() == Some(&c) {
                        stack.pop();
                    }
                }
                _ => {}
            }
        }
    }
    for closer in stack.into_iter().rev() {
        fixed.push(closer);
    }

    if serde_json::from_str::<serde_json::Value>(&fixed).is_ok() {
        warn!(
            "Sanitized malformed tool call arguments for fn={name}: repaired truncated JSON ({} chars)",
            args.len()
        );
        fixed
    } else {
        warn!(
            "Tool call arguments for fn={name} are irrecoverably malformed ({} chars), resetting to {{}}",
            args.len()
        );
        "{}".to_string()
    }
}

fn escape_control(c: char) -> &'static str {
    match c {
        '\n' => "\\n",
        '\r' => "\\r",
        '\t' => "\\t",
        '\u{0008}' => "\\b",
        '\u{000C}' => "\\f",
        _ => "\\uFFFD",
    }
}

/// Repair every invalid `function.arguments` in a message list in place.
///
/// Returns the number of tool calls whose arguments were repaired. Valid
/// arguments are left untouched.
pub fn sanitize_messages_tool_calls(messages: &mut [ChatMessage]) -> usize {
    let mut repaired = 0;
    for msg in messages {
        let Some(tool_calls) = msg.tool_calls.as_mut() else {
            continue;
        };
        for tc in tool_calls {
            if serde_json::from_str::<serde_json::Value>(&tc.function.arguments).is_err() {
                tc.function.arguments =
                    repair_tool_args(&tc.function.name, &tc.function.arguments);
                repaired += 1;
            }
        }
    }
    repaired
}

/// Detect provider errors whose body indicates a tool-call arguments JSON
/// parse failure (as opposed to generic 4xx/5xx errors).
///
/// Matches both nlohmann/json (C++ backends, e.g. llama.cpp server and
/// DeepSeek's upstream) and serde_json error text: `parse_error.10x`,
/// "missing closing quote", "invalid string", "unterminated", combined with
/// tool-call keywords ("tool call", "arguments as json", ...).
pub fn is_tool_call_parse_error(e: &RockBotError) -> bool {
    let msg = match e {
        RockBotError::ServerError { body, .. } => body,
        RockBotError::InvalidRequest(msg) => msg,
        RockBotError::InvalidParameters(msg) => msg,
        RockBotError::Provider(msg) => msg,
        _ => return false,
    };
    let lower = msg.to_lowercase();
    let json_hint = lower.contains("parse_error")
        || lower.contains("parse error")
        || lower.contains("missing closing quote")
        || lower.contains("invalid string")
        || lower.contains("unterminated")
        || lower.contains("json.exception");
    let tool_hint = lower.contains("tool call")
        || lower.contains("tool_call")
        || lower.contains("function.arguments")
        || lower.contains("arguments as json");
    json_hint && tool_hint
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, ToolCall};

    fn assert_valid(repaired: &str) -> serde_json::Value {
        serde_json::from_str(repaired)
            .unwrap_or_else(|e| panic!("repair produced invalid JSON: {e}\nraw: {repaired}"))
    }

    #[test]
    fn test_valid_passthrough() {
        let args = r#"{"location":"Tokyo","units":"metric"}"#;
        assert_eq!(repair_tool_args("get_weather", args), args);
    }

    #[test]
    fn test_truncated_string_closed() {
        let repaired = repair_tool_args("save_knowledge", r#"{"content":"partially written"#);
        assert_valid(&repaired);
        assert!(repaired.starts_with(r#"{"content":"partially written"#));
        assert!(repaired.ends_with('}'));
    }

    #[test]
    fn test_truncated_string_with_escaped_quotes() {
        // Embedded escaped quotes + JSON-looking braces inside the string
        // must not confuse the scanner.
        let raw = r#"{"content":"FHIR report:
```json
{\"resourceType\": \"Patient\", \"id\": \"1\"}
```"#;
        let repaired = repair_tool_args("save_knowledge", raw);
        let value = assert_valid(&repaired);
        let content = value["content"].as_str().unwrap();
        assert!(content.contains("FHIR report"));
        assert!(content.contains("resourceType"));
    }

    #[test]
    fn test_truncated_mid_array() {
        let repaired = repair_tool_args("webdav", r#"{"files":["a.txt","b.txt""#);
        assert_valid(&repaired);
    }

    #[test]
    fn test_truncated_mid_object() {
        let repaired = repair_tool_args("calendar", r#"{"action":"list""#);
        assert_valid(&repaired);
        assert!(repaired.ends_with('}'));
    }

    #[test]
    fn test_trailing_backslash_neutralized() {
        let raw = "{\"content\":\"line one\\";
        let repaired = repair_tool_args("save_knowledge", raw);
        let value = assert_valid(&repaired);
        assert_eq!(value["content"].as_str().unwrap(), "line one\\");
    }

    #[test]
    fn test_backslash_before_raw_newline() {
        // `\` immediately before a raw newline inside a string: the backslash
        // must be kept literal and the newline escaped.
        let raw = "{\"content\":\"line one\\\nrest\"";
        let repaired = repair_tool_args("save_knowledge", raw);
        let value = assert_valid(&repaired);
        assert_eq!(value["content"].as_str().unwrap(), "line one\\\nrest");
    }

    #[test]
    fn test_invalid_escape_kept_literal() {
        // Windows-style path with unescaped backslashes: `\q` must not be
        // dropped, it is emitted as a literal backslash.
        let raw = r#"{"path":"C:\qemu\x64"#;
        let repaired = repair_tool_args("webdav", raw);
        let value = assert_valid(&repaired);
        assert_eq!(value["path"].as_str().unwrap(), r"C:\qemu\x64");
    }

    #[test]
    fn test_raw_newline_in_string_escaped() {
        let raw = "{\"content\":\"line1\nline2\"";
        let repaired = repair_tool_args("save_knowledge", raw);
        let value = assert_valid(&repaired);
        assert_eq!(value["content"].as_str().unwrap(), "line1\nline2");
    }

    #[test]
    fn test_irrecoverable_falls_back_to_empty_object() {
        // Unescaped embedded quotes make the truncation point ambiguous.
        let raw = r#"{"content":"The JSON is {"a": 1} and the rest"#;
        let repaired = repair_tool_args("save_knowledge", raw);
        assert_eq!(repaired, "{}");
    }

    #[test]
    fn test_empty_args() {
        assert_eq!(repair_tool_args("web_search", ""), "{}");
    }

    #[test]
    fn test_garbage_input() {
        assert_eq!(repair_tool_args("vision", "not json at all"), "{}");
    }

    #[test]
    fn test_sanitize_messages_tool_calls_counts() {
        let mut msgs = vec![
            ChatMessage::assistant_with_tool_calls(
                "",
                vec![
                    ToolCall::new("c1", "get_weather", r#"{"city":"Tokyo"}"#.to_string()),
                    ToolCall::new("c2", "save_knowledge", r#"{"content":"truncated"#.to_string()),
                ],
                None,
            ),
            ChatMessage::user("hi"),
        ];
        let count = sanitize_messages_tool_calls(&mut msgs);
        assert_eq!(count, 1);
        let tcs = msgs[0].tool_calls.as_ref().unwrap();
        assert_eq!(tcs[0].function.arguments, r#"{"city":"Tokyo"}"#);
        assert_valid(&tcs[1].function.arguments);
    }

    #[test]
    fn test_is_tool_call_parse_error_nlohmann() {
        let err = RockBotError::ServerError {
            status: 500,
            body: "Failed to parse tool call arguments as JSON:\n\
                   [json.exception.parse_error.101] parse error at line 1, column 11057:\n\
                   syntax error while parsing value - invalid string: missing closing quote"
                .to_string(),
        };
        assert!(is_tool_call_parse_error(&err));
    }

    #[test]
    fn test_is_tool_call_parse_error_generic_500_not_matched() {
        let err = RockBotError::ServerError {
            status: 500,
            body: "Internal server error".to_string(),
        };
        assert!(!is_tool_call_parse_error(&err));
    }

    #[test]
    fn test_is_tool_call_parse_error_invalid_request() {
        let err = RockBotError::InvalidRequest(
            "Failed to parse tool call arguments as JSON: parse error".into(),
        );
        assert!(is_tool_call_parse_error(&err));
    }

    #[test]
    fn test_is_tool_call_parse_error_other_variants() {
        assert!(!is_tool_call_parse_error(&RockBotError::AuthFailed("nope".into())));
        assert!(!is_tool_call_parse_error(&RockBotError::RateLimited { retry_after: None }));
    }
}

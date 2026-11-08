//! The chat-completions wire format: a request from the conversation, and a reply from the
//! response body. Servers differ in small ways (arguments as a JSON string or an object, missing
//! call ids, content as parts), so the reply side is lenient and the request side plain.

use serde_json::{Value, json};

use super::{Message, Reply, ToolCall, ToolSpec};

pub fn request(model: &str, messages: &[Message], tools: &[ToolSpec]) -> Value {
    let messages: Vec<Value> = messages.iter().map(message).collect();
    let mut body = json!({ "model": model, "messages": messages });
    if !tools.is_empty() {
        body["tools"] = tools.iter().map(ToolSpec::wire).collect();
        body["tool_choice"] = "auto".into();
    }
    body
}

fn message(m: &Message) -> Value {
    match m {
        Message::System(text) => json!({"role": "system", "content": text}),
        Message::User(text) => json!({"role": "user", "content": text}),
        Message::Tool { call, content } => {
            json!({"role": "tool", "tool_call_id": call, "content": content})
        }
        Message::Assistant(reply) => {
            let mut v = json!({"role": "assistant", "content": reply.content});
            // Some servers reject an empty `tool_calls` array.
            if !reply.calls.is_empty() {
                v["tool_calls"] = reply
                    .calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {"name": c.name, "arguments": c.arguments},
                        })
                    })
                    .collect();
            }
            v
        }
    }
}

/// The first choice's text and tool calls.
pub fn reply(body: &str) -> Result<Reply, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| {
        format!(
            "the reply is not JSON ({e}): {}",
            crate::tools::cut(body.into(), 500)
        )
    })?;
    let message = v.pointer("/choices/0/message").ok_or_else(|| {
        format!(
            "no choices in the reply: {}",
            crate::tools::cut(body.into(), 500)
        )
    })?;
    let content = text(message.get("content"));
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .filter_map(|(i, c)| call(i, c))
        .collect();
    Ok(Reply { content, calls })
}

fn call(i: usize, c: &Value) -> Option<ToolCall> {
    let function = c.get("function")?;
    let name = function.get("name")?.as_str()?.to_string();
    let arguments = match function.get("arguments") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => "{}".into(),
        Some(other) => other.to_string(),
    };
    let id = c
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map_or_else(|| format!("call_{i}"), str::to_string);
    Some(ToolCall {
        id,
        name,
        arguments,
    })
}

/// Content as a string, or as an array of `{text}` parts; `None` when empty.
fn text(v: Option<&Value>) -> Option<String> {
    let text = match v? {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect(),
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{reply, request};
    use crate::model::{Message, Reply, ToolCall, ToolSpec};

    #[test]
    fn tool_calls_are_read_with_string_or_object_arguments() {
        let body = json!({"choices": [{"message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [
                {"id": "a", "type": "function", "function": {"name": "read", "arguments": "{\"path\":\"x.c\"}"}},
                {"type": "function", "function": {"name": "glob", "arguments": {"pattern": "*.h"}}}
            ]
        }}]});
        let r = reply(&body.to_string()).unwrap();
        assert_eq!(r.content, None);
        assert_eq!(
            r.calls,
            [
                ToolCall {
                    id: "a".into(),
                    name: "read".into(),
                    arguments: "{\"path\":\"x.c\"}".into()
                },
                ToolCall {
                    id: "call_1".into(),
                    name: "glob".into(),
                    arguments: "{\"pattern\":\"*.h\"}".into()
                },
            ]
        );
    }

    #[test]
    fn text_replies_and_bad_bodies() {
        let r =
            reply(&json!({"choices": [{"message": {"content": [{"text": "hi"}]}}]}).to_string());
        assert_eq!(
            r.unwrap(),
            Reply {
                content: Some("hi".into()),
                calls: vec![]
            }
        );
        assert!(reply("<html>").unwrap_err().contains("not JSON"));
        assert!(reply("{\"error\": 1}").unwrap_err().contains("no choices"));
    }

    #[test]
    fn requests_carry_tools_and_round_trip_calls() {
        let call = ToolCall {
            id: "c1".into(),
            name: "read".into(),
            arguments: "{}".into(),
        };
        let messages = [
            Message::System("s".into()),
            Message::User("u".into()),
            Message::Assistant(Reply {
                content: None,
                calls: vec![call],
            }),
            Message::Tool {
                call: "c1".into(),
                content: "out".into(),
            },
            Message::Assistant(Reply {
                content: Some("t".into()),
                calls: vec![],
            }),
        ];
        let tools = [ToolSpec {
            name: "read",
            description: "d",
            parameters: json!({"type": "object"}),
        }];
        let body = request("m", &messages, &tools);
        assert_eq!(body["model"], "m");
        assert_eq!(body["tools"][0]["function"]["name"], "read");
        assert_eq!(
            body["messages"][2]["tool_calls"][0]["function"]["arguments"],
            "{}"
        );
        assert_eq!(
            body["messages"][3],
            json!({"role": "tool", "tool_call_id": "c1", "content": "out"})
        );
        assert!(body["messages"][4].get("tool_calls").is_none());
    }
}

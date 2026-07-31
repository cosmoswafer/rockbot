// ─── Mock Integration Tests: ACP Delegate ───────────────────────────────────
//
// DFD covered: _dfd/tools/acp-delegate.md
//
// The mock is an in-process ACP agent — a tokio task speaking NDJSON JSON-RPC
// over a duplex stream injected as the byte-stream transport via
// `AcpClient::with_component_factory` (no subprocess spawned). Scripted flows:
// initialize → session/new → session/prompt with agent_message_chunk updates,
// tool_call updates, permission requests, timeout + session/cancel, and
// transport death → transparent respawn.

use rockbot::acp::AcpClient;
use rockbot::config::AcpConfig;
use rockbot::error::RockBotError;
use rockbot::tool::Tool;
use rockbot::tools::AcpTool;
use rockbot::validated::BoundedUsize;

use agent_client_protocol::{ByteStreams, DynConnectTo};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, WriteHalf};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

// ─── Mock agent ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum MockEvent {
    Initialized,
    SessionNew { cwd: String },
    Prompt { text: String },
    Cancelled,
    PermissionOutcome { outcome: String, option_id: Option<String> },
}

#[derive(Clone)]
struct MockScript {
    /// Text chunks emitted as agent_message_chunk updates per prompt.
    chunks: Vec<String>,
    /// Emit one tool_call + completed tool_call_update per prompt.
    tool_calls: bool,
    /// Delay before responding to session/prompt (timeout test).
    prompt_response_delay: Option<Duration>,
    /// Send a session/request_permission mid-prompt and await its response.
    request_permission: bool,
    /// Connections with index < N close the transport on session/prompt
    /// (simulates subprocess death).
    die_on_prompt_before_conn: usize,
}

impl Default for MockScript {
    fn default() -> Self {
        Self {
            chunks: vec!["Hello".into(), " world".into()],
            tool_calls: false,
            prompt_response_delay: None,
            request_permission: false,
            die_on_prompt_before_conn: 0,
        }
    }
}

async fn write_json(w: &mut WriteHalf<DuplexStream>, v: Value) {
    let mut s = v.to_string();
    s.push('\n');
    let _ = w.write_all(s.as_bytes()).await;
    let _ = w.flush().await;
}

fn session_update(session_id: &str, update: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": { "sessionId": session_id, "update": update }
    })
}

async fn mock_agent(
    stream: DuplexStream,
    script: MockScript,
    conn_index: usize,
    events: mpsc::UnboundedSender<MockEvent>,
) {
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    let session_id = "ses_mock";

    while let Ok(Some(line)) = lines.next_line().await {
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        match method {
            "initialize" => {
                let id = msg["id"].clone();
                write_json(
                    &mut write,
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": 1,
                            "agentCapabilities": {},
                            "authMethods": [],
                            "agentInfo": { "name": "mock-acp-agent", "version": "0.1.0" }
                        }
                    }),
                )
                .await;
                let _ = events.send(MockEvent::Initialized);
            }
            "session/new" => {
                let id = msg["id"].clone();
                let cwd = msg["params"]["cwd"].as_str().unwrap_or("").to_string();
                write_json(
                    &mut write,
                    json!({ "jsonrpc": "2.0", "id": id, "result": { "sessionId": session_id } }),
                )
                .await;
                let _ = events.send(MockEvent::SessionNew { cwd });
            }
            "session/prompt" => {
                let id = msg["id"].clone();
                let text = msg["params"]["prompt"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let _ = events.send(MockEvent::Prompt { text });

                if conn_index < script.die_on_prompt_before_conn {
                    // Subprocess death: close the transport mid-turn.
                    return;
                }

                for (i, chunk) in script.chunks.iter().enumerate() {
                    write_json(
                        &mut write,
                        session_update(
                            session_id,
                            json!({
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": chunk },
                                "messageId": format!("msg_{i}")
                            }),
                        ),
                    )
                    .await;
                }

                if script.tool_calls {
                    write_json(
                        &mut write,
                        session_update(
                            session_id,
                            json!({
                                "sessionUpdate": "tool_call",
                                "toolCallId": "call_1",
                                "title": "bash",
                                "kind": "execute",
                                "status": "in_progress",
                                "rawInput": { "command": "ls -la" }
                            }),
                        ),
                    )
                    .await;
                    write_json(
                        &mut write,
                        session_update(
                            session_id,
                            json!({
                                "sessionUpdate": "tool_call_update",
                                "toolCallId": "call_1",
                                "status": "completed"
                            }),
                        ),
                    )
                    .await;
                }

                if script.request_permission {
                    let req_id = format!("perm_{conn_index}");
                    write_json(
                        &mut write,
                        json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "method": "session/request_permission",
                            "params": {
                                "sessionId": session_id,
                                "toolCall": { "toolCallId": "call_1", "status": "pending" },
                                "options": [
                                    { "optionId": "allow", "name": "Allow once", "kind": "allow_once" },
                                    { "optionId": "reject", "name": "Reject once", "kind": "reject_once" }
                                ]
                            }
                        }),
                    )
                    .await;
                    // Read until the client's permission response arrives.
                    while let Ok(Some(resp_line)) = lines.next_line().await {
                        let resp: Value = serde_json::from_str(&resp_line).unwrap_or(json!({}));
                        if resp.get("id").and_then(|i| i.as_str()) == Some(req_id.as_str()) {
                            let outcome = resp["result"]["outcome"]["outcome"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            let option_id = resp["result"]["outcome"]["optionId"]
                                .as_str()
                                .map(|s| s.to_string());
                            let _ = events.send(MockEvent::PermissionOutcome { outcome, option_id });
                            break;
                        }
                    }
                }

                if let Some(delay) = script.prompt_response_delay {
                    tokio::time::sleep(delay).await;
                }

                write_json(
                    &mut write,
                    json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" } }),
                )
                .await;
            }
            "session/cancel" => {
                let _ = events.send(MockEvent::Cancelled);
            }
            _ => {}
        }
    }
}

// ─── Harness ─────────────────────────────────────────────────────────────────

struct Harness {
    client: AcpClient,
    events: mpsc::UnboundedReceiver<MockEvent>,
    conn_count: Arc<AtomicUsize>,
}

fn make_harness(cfg: AcpConfig, script: MockScript) -> Harness {
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let conn_count = Arc::new(AtomicUsize::new(0));
    let cc = conn_count.clone();
    let factory = move || {
        let n = cc.fetch_add(1, Ordering::SeqCst);
        let (client_half, agent_half) = tokio::io::duplex(64 * 1024);
        tokio::spawn(mock_agent(agent_half, script.clone(), n, events_tx.clone()));
        let (read, write) = tokio::io::split(client_half);
        DynConnectTo::new(ByteStreams::new(write.compat_write(), read.compat()))
    };
    Harness {
        client: AcpClient::with_component_factory(cfg, factory),
        events: events_rx,
        conn_count,
    }
}

fn test_cfg() -> AcpConfig {
    AcpConfig {
        enabled: true,
        command: "mock-agent".into(),
        ..AcpConfig::default()
    }
}

/// Drain events until `pred` matches or the timeout elapses.
async fn wait_for_event(
    rx: &mut mpsc::UnboundedReceiver<MockEvent>,
    pred: impl Fn(&MockEvent) -> bool,
    timeout: Duration,
) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let ev = tokio::time::timeout(remaining, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("expected event not observed within {timeout:?}"));
        match ev {
            Some(ev) if pred(&ev) => return,
            Some(_) => continue,
            None => panic!("mock event channel closed unexpectedly"),
        }
    }
}

// ─── Tests: happy path (DFD 2a) ─────────────────────────────────────────────

#[tokio::test]
async fn test_acp_delegate_happy_path_aggregates_chunks_and_tool_calls() {
    let script = MockScript {
        tool_calls: true,
        ..MockScript::default()
    };
    let mut h = make_harness(test_cfg(), script);
    let tool = AcpTool::new(Arc::new(h.client));

    let result = tool
        .execute(r#"{"prompt": "do the thing"}"#)
        .await
        .expect("acp_delegate should succeed");

    assert!(
        result.contains("Hello world"),
        "expected aggregated chunk text, got: {result}"
    );
    assert!(
        result.contains("Tool calls:\n- bash: completed"),
        "expected tool-call summary, got: {result}"
    );
    assert!(
        result.contains("[stop_reason: end_turn]"),
        "expected stop-reason footer, got: {result}"
    );
    assert!(!result.contains("truncated"), "should not be truncated: {result}");

    // Session lifecycle events: one connection, lazy session create.
    wait_for_event(&mut h.events, |e| matches!(e, MockEvent::SessionNew { .. }), Duration::from_secs(2)).await;
    wait_for_event(&mut h.events, |e| matches!(e, MockEvent::Prompt { text } if text == "do the thing"), Duration::from_secs(2)).await;
    assert_eq!(h.conn_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_acp_session_reused_across_prompts() {
    let mut h = make_harness(test_cfg(), MockScript::default());

    h.client.prompt("first", 30).await.expect("first prompt");
    h.client.prompt("second", 30).await.expect("second prompt");

    // Only one session/new for two prompts — the session is reused.
    let mut session_news = 0;
    let mut prompts = 0;
    while let Ok(ev) = h.events.try_recv() {
        match ev {
            MockEvent::SessionNew { .. } => session_news += 1,
            MockEvent::Prompt { .. } => prompts += 1,
            _ => {}
        }
    }
    assert_eq!(session_news, 1, "session should be created once and reused");
    assert_eq!(prompts, 2);
    assert_eq!(h.conn_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_acp_output_truncated_at_max_response_chars() {
    let cfg = AcpConfig {
        max_response_chars: BoundedUsize::try_new(10).unwrap(),
        ..test_cfg()
    };
    let script = MockScript {
        chunks: vec!["Hello ".into(), "world, this is a long tail".into()],
        ..MockScript::default()
    };
    let h = make_harness(cfg, script);
    let tool = AcpTool::new(Arc::new(h.client));

    let result = tool
        .execute(r#"{"prompt": "say lots"}"#)
        .await
        .expect("acp_delegate should succeed");

    assert!(
        result.starts_with("Hello worl"),
        "text should be capped at 10 chars, got: {result}"
    );
    assert!(!result.contains("this is a long tail"), "tail must be cut: {result}");
    assert!(
        result.contains("[output truncated at 10 chars]"),
        "expected truncation footer, got: {result}"
    );
}

// ─── Tests: timeout → session/cancel (DFD 2c) ───────────────────────────────

#[tokio::test]
async fn test_acp_prompt_timeout_sends_session_cancel() {
    let script = MockScript {
        prompt_response_delay: Some(Duration::from_secs(2)),
        ..MockScript::default()
    };
    let mut h = make_harness(test_cfg(), script);

    let err = h
        .client
        .prompt("slow task", 1)
        .await
        .expect_err("prompt should time out");
    assert!(
        err.to_string().contains("timed out after 1s"),
        "unexpected error: {err}"
    );

    // The agent observes session/cancel after it finishes its delayed work.
    wait_for_event(&mut h.events, |e| matches!(e, MockEvent::Cancelled), Duration::from_secs(5)).await;
}

// ─── Tests: permission policy (DFD 2c) ──────────────────────────────────────

#[tokio::test]
async fn test_acp_permission_denied_by_default() {
    let script = MockScript {
        request_permission: true,
        ..MockScript::default()
    };
    let mut h = make_harness(test_cfg(), script);

    h.client.prompt("needs permission", 30).await.expect("prompt");

    wait_for_event(
        &mut h.events,
        |e| matches!(e, MockEvent::PermissionOutcome { outcome, option_id } if outcome == "cancelled" && option_id.is_none()),
        Duration::from_secs(2),
    )
    .await;
}

#[tokio::test]
async fn test_acp_permission_auto_approved_when_enabled() {
    let cfg = AcpConfig {
        auto_approve_permissions: true,
        ..test_cfg()
    };
    let script = MockScript {
        request_permission: true,
        ..MockScript::default()
    };
    let mut h = make_harness(cfg, script);

    h.client.prompt("needs permission", 30).await.expect("prompt");

    wait_for_event(
        &mut h.events,
        |e| matches!(e, MockEvent::PermissionOutcome { outcome, option_id } if outcome == "selected" && option_id.as_deref() == Some("allow")),
        Duration::from_secs(2),
    )
    .await;
}

// ─── Tests: subprocess death → transparent respawn (DFD 2c) ─────────────────

#[tokio::test]
async fn test_acp_transport_death_triggers_respawn_and_retry() {
    let script = MockScript {
        die_on_prompt_before_conn: 1, // first connection dies on prompt
        ..MockScript::default()
    };
    let mut h = make_harness(test_cfg(), script);

    let result = h
        .client
        .prompt("work", 30)
        .await
        .expect("prompt should succeed after transparent respawn");
    assert!(result.text.contains("Hello world"), "got: {}", result.text);

    // Two connections: the dead one and the respawned one.
    assert_eq!(h.conn_count.load(Ordering::SeqCst), 2);
    let mut inits = 0;
    let mut prompts = 0;
    while let Ok(ev) = h.events.try_recv() {
        match ev {
            MockEvent::Initialized => inits += 1,
            MockEvent::Prompt { .. } => prompts += 1,
            _ => {}
        }
    }
    assert_eq!(inits, 2, "both connections initialize");
    assert_eq!(prompts, 2, "prompt attempted on both connections");
}

#[tokio::test]
async fn test_acp_double_transport_death_surfaces_error() {
    let script = MockScript {
        die_on_prompt_before_conn: usize::MAX, // every connection dies on prompt
        ..MockScript::default()
    };
    let h = make_harness(test_cfg(), script);

    let err = h
        .client
        .prompt("work", 30)
        .await
        .expect_err("prompt should fail after one respawn retry");
    assert!(
        matches!(err, RockBotError::AcpTransportClosed(_)),
        "expected transport-closed error, got: {err}"
    );
    assert_eq!(h.conn_count.load(Ordering::SeqCst), 2, "exactly one respawn retry");
}

// ─── Tests: absolute session cwd ────────────────────────────────────────────

#[tokio::test]
async fn test_acp_session_new_sends_absolute_cwd() {
    let cfg = AcpConfig {
        session_cwd: ".".into(),
        ..test_cfg()
    };
    let mut h = make_harness(cfg, MockScript::default());

    h.client.prompt("hi", 30).await.expect("prompt");

    wait_for_event(
        &mut h.events,
        |e| matches!(e, MockEvent::SessionNew { cwd } if cwd.starts_with('/')),
        Duration::from_secs(2),
    )
    .await;
}

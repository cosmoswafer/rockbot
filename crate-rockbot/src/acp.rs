//! ACP (Agent Client Protocol) client — spawns an external ACP agent as a
//! subprocess over stdio (NDJSON JSON-RPC 2.0) and delegates task prompts to
//! it. See `_dfd/tools/acp-delegate.md`.
//!
//! All `agent-client-protocol` SDK usage is encapsulated in this module.
//!
//! Lifecycle: `AcpClient::new` is cheap and spawns nothing; the agent process
//! is started lazily on the first [`AcpClient::prompt`] call (so a missing
//! agent binary never blocks bot startup). A single ACP session is created
//! lazily and reused; prompts are serialized by an internal mutex because ACP
//! sessions process one prompt turn at a time. If the subprocess dies, one
//! transparent respawn + retry is attempted per call. [`AcpClient::shutdown`]
//! terminates the child (also guaranteed by `kill_on_drop`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, Implementation, InitializeRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionId, SessionNotification,
    SessionUpdate, StopReason, TextContent, ToolCallStatus,
};
use agent_client_protocol::{Agent, ByteStreams, ConnectTo, ConnectionTo, DynConnectTo};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, info, warn};

use crate::config::AcpConfig;
use crate::error::{Result, RockBotError};

/// How long to wait for the agent's `initialize` handshake after spawn.
const INIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Aggregated result of one prompt turn — DFD: `AcpPromptResult`.
#[derive(Debug)]
pub struct AcpPromptResult {
    /// Concatenated `agent_message_chunk` text plus one summary line per tool
    /// call; capped at `max_response_chars`.
    pub text: String,
    pub stop_reason: StopReason,
    /// Whether `text` hit the `max_response_chars` cap.
    pub truncated: bool,
}

/// Commands sent to the connection task (DFD 2b — command channel).
enum AcpCommand {
    Prompt {
        prompt: String,
        timeout: Duration,
        respond: oneshot::Sender<Result<AcpPromptResult>>,
    },
}

/// Handle to a running connection task.
struct ConnectionHandle {
    cmd_tx: mpsc::Sender<AcpCommand>,
    task: tokio::task::JoinHandle<()>,
}

struct ClientState {
    conn: Option<ConnectionHandle>,
}

/// Per-turn aggregation shared between the connection task (which resets and
/// reads it) and the `session/update` notification handler (which appends).
struct TurnState {
    text: String,
    /// tool_call_id → title, for naming tools in summary lines.
    tool_titles: HashMap<String, String>,
    /// One summary line per finished tool call, in arrival order.
    tool_lines: Vec<String>,
    truncated: bool,
    max_chars: usize,
}

type SharedTurn = Arc<Mutex<TurnState>>;

impl TurnState {
    fn new(max_chars: usize) -> Self {
        Self {
            text: String::new(),
            tool_titles: HashMap::new(),
            tool_lines: Vec::new(),
            truncated: false,
            max_chars,
        }
    }

    fn reset(&mut self) {
        self.text.clear();
        self.tool_titles.clear();
        self.tool_lines.clear();
        self.truncated = false;
    }

    fn push_text_chunk(&mut self, chunk: &str) {
        if self.text.len() >= self.max_chars {
            self.truncated = true;
            return;
        }
        self.text.push_str(chunk);
        if self.text.len() > self.max_chars {
            // Char-boundary-safe truncation.
            let mut end = self.max_chars;
            while !self.text.is_char_boundary(end) {
                end -= 1;
            }
            self.text.truncate(end);
            self.truncated = true;
        }
    }

    fn record_tool_call(&mut self, id: &str, title: &str, status: ToolCallStatus) {
        self.tool_titles.insert(id.to_string(), title.to_string());
        if let Some(status) = terminal_status(status) {
            self.tool_lines.push(format!("- {title}: {status}"));
        }
    }

    fn record_tool_update(&mut self, id: &str, title: Option<&str>, status: Option<ToolCallStatus>) {
        if let Some(t) = title {
            self.tool_titles.insert(id.to_string(), t.to_string());
        }
        if let Some(status) = status.and_then(terminal_status) {
            let title = self
                .tool_titles
                .get(id)
                .cloned()
                .unwrap_or_else(|| "tool".to_string());
            self.tool_lines.push(format!("- {title}: {status}"));
        }
    }

    fn build_result(&self, stop_reason: StopReason) -> AcpPromptResult {
        let mut text = self.text.clone();
        if !self.tool_lines.is_empty() {
            text.push_str("\n\nTool calls:\n");
            text.push_str(&self.tool_lines.join("\n"));
        }
        let mut truncated = self.truncated;
        if text.len() > self.max_chars {
            let mut end = self.max_chars;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            truncated = true;
        }
        AcpPromptResult {
            text,
            stop_reason,
            truncated,
        }
    }
}

fn terminal_status(status: ToolCallStatus) -> Option<&'static str> {
    match status {
        ToolCallStatus::Completed => Some("completed"),
        ToolCallStatus::Failed => Some("failed"),
        _ => None,
    }
}

/// Thin, type-safe wrapper around the ACP SDK client. See module docs.
pub struct AcpClient {
    cfg: AcpConfig,
    state: Mutex<ClientState>,
    /// Test-only transport injection. When set, (re)connections call this
    /// factory instead of spawning a subprocess.
    transport_factory: Option<
        Box<dyn Fn() -> DynConnectTo<agent_client_protocol::Client> + Send + Sync>,
    >,
}

impl AcpClient {
    /// Create a client from config. Spawns nothing — the agent process starts
    /// lazily on the first `prompt` call.
    pub fn new(cfg: AcpConfig) -> Self {
        Self {
            cfg,
            state: Mutex::new(ClientState { conn: None }),
            transport_factory: None,
        }
    }

    pub fn config(&self) -> &AcpConfig {
        &self.cfg
    }

    /// Test-only — construct a client that (re)connects over transport
    /// components produced by `factory` (e.g. `ByteStreams` over tokio duplex
    /// streams backed by an in-process mock agent) instead of spawning a
    /// subprocess. Like production, the first connection is lazy.
    pub fn with_component_factory<F>(cfg: AcpConfig, factory: F) -> Self
    where
        F: Fn() -> DynConnectTo<agent_client_protocol::Client> + Send + Sync + 'static,
    {
        Self {
            cfg,
            state: Mutex::new(ClientState { conn: None }),
            transport_factory: Some(Box::new(factory)),
        }
    }

    /// Send a task prompt to the agent and await the aggregated result.
    ///
    /// Serialized: only one prompt turn is in flight per client. On transport
    /// failure (dead subprocess) one transparent respawn + retry is attempted.
    pub async fn prompt(&self, prompt: &str, timeout_secs: u64) -> Result<AcpPromptResult> {
        let timeout = Duration::from_secs(timeout_secs);
        let mut state = self.state.lock().await;
        let mut last_err: Option<RockBotError> = None;

        for attempt in 1..=2u32 {
            let cmd_tx = self.ensure_connected(&mut state).await?;
            let (respond, rx) = oneshot::channel();
            let send_result = cmd_tx
                .send(AcpCommand::Prompt {
                    prompt: prompt.to_string(),
                    timeout,
                    respond,
                })
                .await;

            if let Err(e) = send_result {
                warn!("ACP connection task not reachable (attempt {attempt}): {e}");
                state.conn = None;
                last_err = Some(RockBotError::Acp("ACP connection task ended".into()));
                continue;
            }

            match rx.await {
                Ok(Err(RockBotError::AcpTransportClosed(e))) => {
                    warn!("ACP transport closed, respawning (attempt {attempt}): {e}");
                    state.conn = None;
                    last_err = Some(RockBotError::AcpTransportClosed(e));
                    continue;
                }
                Ok(result) => return result,
                Err(_) => {
                    warn!("ACP connection task dropped prompt responder (attempt {attempt})");
                    state.conn = None;
                    last_err = Some(RockBotError::Acp(
                        "ACP connection task died mid-prompt".into(),
                    ));
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            RockBotError::Acp("ACP prompt failed after respawn retry".into())
        }))
    }

    /// Gracefully terminate the agent subprocess. Idempotent.
    pub async fn shutdown(&self) {
        let handle = { self.state.lock().await.conn.take() };
        if let Some(handle) = handle {
            drop(handle.cmd_tx);
            let mut task = handle.task;
            if tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
            {
                // Mid-request or wedged — force-stop; kill_on_drop kills the child.
                task.abort();
            }
            debug!("ACP agent subprocess terminated");
        }
    }

    async fn ensure_connected(&self, state: &mut ClientState) -> Result<mpsc::Sender<AcpCommand>> {
        if state.conn.is_none() {
            let handle = match &self.transport_factory {
                Some(factory) => {
                    debug!("ACP connecting over injected transport factory");
                    let component = factory();
                    self.start_connection_task(component, None).await?
                }
                None => {
                    info!(
                        "Spawning ACP agent: {} {:?} (cwd={})",
                        self.cfg.command, self.cfg.args, self.cfg.cwd
                    );
                    self.spawn_connection().await?
                }
            };
            state.conn = Some(handle);
        }
        Ok(state
            .conn
            .as_ref()
            .expect("conn just set")
            .cmd_tx
            .clone())
    }

    /// Spawn the agent subprocess with an explicit env allowlist (never
    /// blanket-inheriting rockbot's environment) and `kill_on_drop` semantics.
    async fn spawn_connection(&self) -> Result<ConnectionHandle> {
        let mut cmd = tokio::process::Command::new(&self.cfg.command);
        cmd.args(&self.cfg.args)
            .env_clear()
            // PATH resolves the executable; HOME is needed by most agents for
            // their own config. Neither is a secret. Everything else comes
            // exclusively from the operator's `[acp].env` allowlist.
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .envs(&self.cfg.env)
            .current_dir(&self.cfg.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // Agent diagnostics flow into rockbot's log; avoids stderr-pipe
            // deadlock without a dedicated drain task.
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            RockBotError::Acp(format!(
                "failed to spawn ACP agent '{}': {e}",
                self.cfg.command
            ))
        })?;
        let child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| RockBotError::Acp("ACP agent stdin not piped".into()))?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| RockBotError::Acp("ACP agent stdout not piped".into()))?;

        let component = ByteStreams::new(child_stdin.compat_write(), child_stdout.compat());
        self.start_connection_task(component, Some(child)).await
    }

    /// Start the connection task over `component` and wait for the ACP
    /// `initialize` handshake to complete (DFD 2b).
    async fn start_connection_task<C>(
        &self,
        component: C,
        child: Option<tokio::process::Child>,
    ) -> Result<ConnectionHandle>
    where
        C: ConnectTo<agent_client_protocol::Client> + 'static,
    {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AcpCommand>(8);
        let (init_tx, init_rx) = oneshot::channel::<std::result::Result<String, String>>();
        let turn: SharedTurn = Arc::new(Mutex::new(TurnState::new(
            self.cfg.max_response_chars.as_usize(),
        )));
        let cfg = self.cfg.clone();

        let task = tokio::spawn(run_connection(component, child, cmd_rx, turn, cfg, init_tx));

        match tokio::time::timeout(INIT_TIMEOUT, init_rx).await {
            Ok(Ok(Ok(info))) => {
                info!("ACP agent initialized: {info}");
                Ok(ConnectionHandle { cmd_tx, task })
            }
            Ok(Ok(Err(e))) => {
                task.abort();
                Err(RockBotError::Acp(format!("ACP initialize failed: {e}")))
            }
            Ok(Err(_)) => {
                task.abort();
                Err(RockBotError::Acp(
                    "ACP connection task ended during initialize".into(),
                ))
            }
            Err(_) => {
                task.abort();
                Err(RockBotError::Acp(format!(
                    "ACP initialize timed out after {}s",
                    INIT_TIMEOUT.as_secs()
                )))
            }
        }
    }
}

/// The long-lived connection task (DFD 2b): owns the child process handle,
/// performs `initialize`, then serves prompt commands until the command
/// channel closes or the transport dies.
async fn run_connection<C>(
    component: C,
    child: Option<tokio::process::Child>,
    mut cmd_rx: mpsc::Receiver<AcpCommand>,
    turn: SharedTurn,
    cfg: AcpConfig,
    init_tx: oneshot::Sender<std::result::Result<String, String>>,
) where
    C: ConnectTo<agent_client_protocol::Client> + 'static,
{
    let auto_approve = cfg.auto_approve_permissions;
    let session_cwd = cfg.session_cwd.clone();
    let notif_turn = turn.clone();

    let result = agent_client_protocol::Client
        .builder()
        .name("rockbot")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                handle_session_update(&notif_turn, notification.update).await;
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _cx| {
                if auto_approve {
                    let chosen = request
                        .options
                        .iter()
                        .find(|o| {
                            matches!(
                                o.kind,
                                PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
                            )
                        })
                        .or(request.options.first())
                        .map(|o| o.option_id.clone());
                    match chosen {
                        Some(id) => {
                            debug!("ACP permission request auto-approved");
                            responder.respond(RequestPermissionResponse::new(
                                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
                            ))
                        }
                        None => responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        )),
                    }
                } else {
                    debug!("ACP permission request denied (auto_approve_permissions = false)");
                    responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(component, async move |cx: ConnectionTo<Agent>| {
            // Owned by this closure: when the connection scope ends, the child
            // is dropped and `kill_on_drop` terminates the agent process.
            let _child = child;

            let init = cx
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_info(Implementation::new("rockbot", env!("CARGO_PKG_VERSION"))),
                )
                .block_task()
                .await;

            let init = match init {
                Ok(init) => init,
                Err(e) => {
                    let _ = init_tx.send(Err(e.to_string()));
                    return Err(e);
                }
            };

            if !init.auth_methods.is_empty() {
                let methods: Vec<String> = init
                    .auth_methods
                    .iter()
                    .map(|m| m.id().to_string())
                    .collect();
                warn!(
                    "ACP agent advertises authMethods ({}) — continuing unauthenticated; \
                     an agent that requires auth will surface a protocol error on use",
                    methods.join(", ")
                );
            }

            let agent_desc = init
                .agent_info
                .map(|i| format!("{} v{}", i.name, i.version))
                .unwrap_or_else(|| "unknown agent".into());
            let _ = init_tx.send(Ok(agent_desc));

            let mut session_id: Option<SessionId> = None;
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    AcpCommand::Prompt {
                        prompt,
                        timeout,
                        respond,
                    } => {
                        let result =
                            handle_prompt(&cx, &mut session_id, &session_cwd, &turn, &prompt, timeout)
                                .await;
                        let _ = respond.send(result);
                    }
                }
            }
            Ok(())
        })
        .await;

    if let Err(e) = result {
        debug!("ACP connection task ended: {e}");
    }
}

/// One prompt turn (DFD 2a): lazy `session/new`, reset turn state, send
/// `session/prompt`, aggregate updates, enforce the timeout via
/// `session/cancel`.
async fn handle_prompt(
    cx: &ConnectionTo<Agent>,
    session_id: &mut Option<SessionId>,
    session_cwd: &str,
    turn: &SharedTurn,
    prompt: &str,
    timeout: Duration,
) -> Result<AcpPromptResult> {
    if session_id.is_none() {
        // session/new requires an absolute cwd.
        let cwd = std::path::absolute(session_cwd)
            .unwrap_or_else(|_| PathBuf::from(session_cwd));
        let resp = cx
            .send_request(NewSessionRequest::new(cwd))
            .block_task()
            .await
            .map_err(|e| acp_err("session/new failed", e))?;
        *session_id = Some(resp.session_id);
    }
    let sid = session_id.as_ref().expect("session_id just set").clone();

    turn.lock().await.reset();

    let prompt_fut = cx
        .send_request(PromptRequest::new(
            sid.clone(),
            vec![ContentBlock::Text(TextContent::new(prompt.to_string()))],
        ))
        .block_task();

    let prompt_resp = match tokio::time::timeout(timeout, prompt_fut).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => return Err(acp_err("session/prompt failed", e)),
        Err(_) => {
            if let Err(e) = cx.send_notification(CancelNotification::new(sid)) {
                debug!("failed to send session/cancel: {e}");
            }
            return Err(RockBotError::Acp(format!(
                "ACP prompt timed out after {}s (session/cancel sent)",
                timeout.as_secs()
            )));
        }
    };

    let st = turn.lock().await;
    Ok(st.build_result(prompt_resp.stop_reason))
}

/// Aggregate `session/update` notifications into the shared turn state (DFD
/// 2a — `agent_message_chunk` text; tool-call summary lines). Thought chunks,
/// plans, usage and command updates are ignored.
async fn handle_session_update(turn: &SharedTurn, update: SessionUpdate) {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(text) = chunk.content {
                turn.lock().await.push_text_chunk(&text.text);
            }
        }
        SessionUpdate::ToolCall(tc) => {
            turn.lock()
                .await
                .record_tool_call(&tc.tool_call_id.0, &tc.title, tc.status);
        }
        SessionUpdate::ToolCallUpdate(upd) => {
            turn.lock().await.record_tool_update(
                &upd.tool_call_id.0,
                upd.fields.title.as_deref(),
                upd.fields.status,
            );
        }
        _ => {}
    }
}

/// Map an SDK error, classifying transport-closed (dead subprocess) so
/// `prompt()` can trigger its respawn-and-retry path.
fn acp_err(context: &str, e: agent_client_protocol::Error) -> RockBotError {
    if agent_client_protocol::is_incoming_transport_closed(&e) {
        RockBotError::AcpTransportClosed(format!("{context}: {e}"))
    } else {
        RockBotError::Acp(format!("{context}: {e}"))
    }
}

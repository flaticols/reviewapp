use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, McpServer, McpServerStdio,
    NewSessionRequest, PermissionOptionKind, PlanEntryStatus, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, StopReason, TextContent,
    ToolCallStatus, ToolKind as AcpToolKind,
};
use agent_client_protocol::{Agent, ConnectionTo};
use futures::FutureExt as _;
use futures::channel::oneshot;

use crate::findings::{FindingsListener, Report, ReportedFinding};
use crate::options::{SessionChoices, agent_for};

/// What kind of work a tool call does, reduced to what the review card distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Search,
    Execute,
    Edit,
    Other,
}

/// Where a tool call stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

/// A tool call the agent started or updated.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolActivity {
    pub id: String,
    pub title: Option<String>,
    pub kind: Option<ToolKind>,
    pub status: Option<ToolStatus>,
    pub paths: Vec<PathBuf>,
    /// The command line, when the tool runs one, such as `git diff -- src/lib.rs`.
    pub command: Option<String>,
}

/// Where a step of the agent's plan stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

/// One step of the plan an agent shares while it works.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlanStep {
    pub content: String,
    pub status: PlanStatus,
}

/// The user's answer to a permission request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    AllowOnce,
    AllowAlways,
    Deny,
}

/// One option the agent offered.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PermissionChoice {
    pub id: String,
    pub label: String,
    pub decision: Decision,
}

/// The agent asks to run a tool, usually a command. Answer with [`Self::respond`];
/// dropping the prompt cancels the request.
#[derive(Debug)]
pub struct PermissionPrompt {
    title: String,
    command: Option<String>,
    choices: Vec<PermissionChoice>,
    responder: oneshot::Sender<Option<String>>,
}

impl PermissionPrompt {
    /// The tool call's title, such as `Run cargo test`.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The command line, when the tool runs one.
    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
    }

    /// The options the agent offered.
    pub fn choices(&self) -> &[PermissionChoice] {
        &self.choices
    }

    /// Whether the agent offered `decision`.
    pub fn offers(&self, decision: Decision) -> bool {
        self.choices
            .iter()
            .any(|choice| choice.decision == decision)
    }

    /// Sends the decision. Falls back to the closest offered option: an "always" answer
    /// becomes "once" when the agent offers no persistent option, and so on.
    pub fn respond(self, decision: Decision) {
        let fallback = match decision {
            Decision::AllowAlways => [Decision::AllowAlways, Decision::AllowOnce],
            Decision::AllowOnce => [Decision::AllowOnce, Decision::AllowAlways],
            Decision::Deny => [Decision::Deny, Decision::Deny],
        };
        let option = fallback.iter().find_map(|wanted| {
            self.choices
                .iter()
                .find(|choice| choice.decision == *wanted)
                .map(|choice| choice.id.clone())
        });
        self.responder.send(option).ok();
    }
}

/// How a review run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Cancelled,
    Refused,
    /// The agent hit a token or turn limit before finishing.
    Truncated,
}

/// Everything a run reports, in order.
#[derive(Debug)]
pub enum AgentEvent {
    /// The agent process started and answered `initialize`.
    Connected {
        name: Option<String>,
        version: Option<String>,
    },
    /// A chunk of the agent's reply.
    Message(String),
    /// A chunk of the agent's reasoning, for agents that share it.
    Thought(String),
    /// The agent's whole current plan, replacing the previous one.
    Plan(Vec<PlanStep>),
    /// A tool call started or changed.
    Tool(ToolActivity),
    /// The agent needs permission to continue.
    Permission(PermissionPrompt),
    /// The agent reported a finding through the findings tool.
    Finding(ReportedFinding),
    /// The run ended.
    Finished(RunOutcome),
    /// The run failed; the message explains why.
    Failed(String),
}

/// One review run.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ReviewRequest {
    /// The command line that starts the agent, such as `npx -y @agentclientprotocol/claude-agent-acp@latest`.
    pub command_line: String,
    /// Extra environment for the agent, such as a `PATH` that finds `node`.
    pub env: Vec<(String, String)>,
    /// The folder the agent works in: the pull request's worktree.
    pub cwd: PathBuf,
    pub prompt: String,
    /// The program that serves the findings MCP tool, normally Switchfold's own binary.
    pub findings_server: Option<PathBuf>,
    /// The model, effort and mode to switch the session to before prompting.
    pub choices: SessionChoices,
}

impl ReviewRequest {
    /// A run of `command_line` in `cwd` with `prompt`.
    pub fn new(command_line: impl Into<String>, cwd: PathBuf, prompt: impl Into<String>) -> Self {
        Self {
            command_line: command_line.into(),
            env: Vec::new(),
            cwd,
            prompt: prompt.into(),
            findings_server: None,
            choices: SessionChoices::default(),
        }
    }

    /// Switches the session's model, effort and mode before prompting.
    pub fn with_choices(mut self, choices: SessionChoices) -> Self {
        self.choices = choices;
        self
    }

    /// Serves `report_finding` with `program --findings-mcp <socket>`.
    pub fn with_findings_server(mut self, program: PathBuf) -> Self {
        self.findings_server = Some(program);
        self
    }

    /// Adds an environment variable for the agent process.
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }
}

/// Stops a run: the agent is asked to cancel, then its process group is killed.
#[derive(Debug)]
pub struct RunControl {
    cancel: Option<oneshot::Sender<()>>,
}

impl RunControl {
    /// Asks the agent to stop. A second call does nothing.
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.send(()).ok();
        }
    }
}

/// Starts a review run. Spawn the returned future on a background executor and read
/// events from the receiver until [`AgentEvent::Finished`] or [`AgentEvent::Failed`].
pub fn start_review(
    request: ReviewRequest,
) -> (
    impl Future<Output = ()> + Send + 'static,
    async_channel::Receiver<AgentEvent>,
    RunControl,
) {
    let (events, receiver) = async_channel::unbounded();
    let (cancel, cancelled) = oneshot::channel();
    let future = async move {
        let result = run(request, events.clone(), cancelled).await;
        let event = match result {
            Ok(outcome) => AgentEvent::Finished(outcome),
            Err(message) => AgentEvent::Failed(message),
        };
        events.send(event).await.ok();
    };
    (
        future,
        receiver,
        RunControl {
            cancel: Some(cancel),
        },
    )
}

fn tool_kind(kind: AcpToolKind) -> ToolKind {
    match kind {
        AcpToolKind::Read => ToolKind::Read,
        AcpToolKind::Search | AcpToolKind::Fetch => ToolKind::Search,
        AcpToolKind::Execute => ToolKind::Execute,
        AcpToolKind::Edit | AcpToolKind::Delete | AcpToolKind::Move => ToolKind::Edit,
        _ => ToolKind::Other,
    }
}

fn tool_status(status: ToolCallStatus) -> ToolStatus {
    match status {
        ToolCallStatus::Pending => ToolStatus::Pending,
        ToolCallStatus::InProgress => ToolStatus::Running,
        ToolCallStatus::Completed => ToolStatus::Completed,
        ToolCallStatus::Failed => ToolStatus::Failed,
        _ => ToolStatus::Running,
    }
}

/// The command a tool call would run, from its raw input when the agent sends one.
fn command_of(raw: Option<&serde_json::Value>) -> Option<String> {
    let raw = raw?;
    if let Some(command) = raw.get("command") {
        return match command {
            serde_json::Value::String(command) => Some(command.clone()),
            serde_json::Value::Array(parts) => Some(
                parts
                    .iter()
                    .filter_map(|part| part.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        };
    }
    raw.get("cmd")
        .and_then(|cmd| cmd.as_str())
        .map(str::to_string)
}

fn decision_of(kind: PermissionOptionKind) -> Decision {
    match kind {
        PermissionOptionKind::AllowOnce => Decision::AllowOnce,
        PermissionOptionKind::AllowAlways => Decision::AllowAlways,
        _ => Decision::Deny,
    }
}

async fn run(
    request: ReviewRequest,
    events: async_channel::Sender<AgentEvent>,
    cancelled: oneshot::Receiver<()>,
) -> Result<RunOutcome, String> {
    let agent = agent_for(&request.command_line, &request.env)?;

    let listener = match &request.findings_server {
        Some(_) => Some(
            FindingsListener::bind()
                .map_err(|error| format!("Couldn’t open the findings channel: {error}"))?,
        ),
        None => None,
    };
    let mcp_servers: Vec<McpServer> = match (&request.findings_server, &listener) {
        (Some(program), Some(listener)) => vec![McpServer::Stdio(
            McpServerStdio::new("switchfold", program.clone()).args(vec![
                "--findings-mcp".into(),
                listener.socket_path().display().to_string(),
            ]),
        )],
        _ => Vec::new(),
    };
    if let Some(listener) = listener {
        let findings = events.clone();
        listener.forward(move |report| {
            let event = match report {
                Report::Finding(finding) => AgentEvent::Finding(finding),
                Report::Plan { steps } => AgentEvent::Plan(
                    steps
                        .into_iter()
                        .map(|step| PlanStep {
                            content: step.step,
                            status: match step.status.as_str() {
                                "completed" => PlanStatus::Completed,
                                "in_progress" => PlanStatus::InProgress,
                                _ => PlanStatus::Pending,
                            },
                        })
                        .collect(),
                ),
            };
            findings.send_blocking(event).ok();
        });
    }

    let notification_events = events.clone();
    let permission_events = events.clone();
    // Tool calls of Switchfold's own MCP tools, by id: some agents title the permission
    // request generically and only the tool call itself names the tool.
    let own_calls: Arc<Mutex<HashSet<String>>> = Arc::default();
    let own_calls_seen = own_calls.clone();
    let cwd = request.cwd.clone();
    let prompt = request.prompt.clone();
    let connected_events = events.clone();
    let choices = request.choices.clone();

    agent_client_protocol::Client
        .builder()
        .name("switchfold")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                let named = match &notification.update {
                    SessionUpdate::ToolCall(call) => {
                        Some((call.tool_call_id.to_string(), Some(call.title.clone())))
                    }
                    SessionUpdate::ToolCallUpdate(update) => {
                        Some((update.tool_call_id.to_string(), update.fields.title.clone()))
                    }
                    _ => None,
                };
                if let Some((id, Some(title))) = named
                    && is_own_tool(&title)
                    && let Ok(mut own) = own_calls_seen.lock()
                {
                    own.insert(id);
                }
                let event = match notification.update {
                    SessionUpdate::AgentMessageChunk(chunk) => match chunk.content {
                        ContentBlock::Text(text) => Some(AgentEvent::Message(text.text)),
                        _ => None,
                    },
                    SessionUpdate::AgentThoughtChunk(chunk) => match chunk.content {
                        ContentBlock::Text(text) => Some(AgentEvent::Thought(text.text)),
                        _ => None,
                    },
                    SessionUpdate::Plan(plan) => Some(AgentEvent::Plan(
                        plan.entries
                            .into_iter()
                            .map(|entry| PlanStep {
                                content: entry.content,
                                status: match entry.status {
                                    PlanEntryStatus::Completed => PlanStatus::Completed,
                                    PlanEntryStatus::InProgress => PlanStatus::InProgress,
                                    _ => PlanStatus::Pending,
                                },
                            })
                            .collect(),
                    )),
                    SessionUpdate::ToolCall(call) => Some(AgentEvent::Tool(ToolActivity {
                        id: call.tool_call_id.to_string(),
                        command: command_of(call.raw_input.as_ref()),
                        title: Some(call.title),
                        kind: Some(tool_kind(call.kind)),
                        status: Some(tool_status(call.status)),
                        paths: call.locations.into_iter().map(|l| l.path).collect(),
                    })),
                    SessionUpdate::ToolCallUpdate(update) => Some(AgentEvent::Tool(ToolActivity {
                        id: update.tool_call_id.to_string(),
                        command: command_of(update.fields.raw_input.as_ref()),
                        title: update.fields.title,
                        kind: update.fields.kind.map(tool_kind),
                        status: update.fields.status.map(tool_status),
                        paths: update
                            .fields
                            .locations
                            .unwrap_or_default()
                            .into_iter()
                            .map(|l| l.path)
                            .collect(),
                    })),
                    _ => None,
                };
                if let Some(event) = event {
                    notification_events.send(event).await.ok();
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, connection| {
                let (sender, answer) = oneshot::channel();
                let prompt = PermissionPrompt {
                    title: request
                        .tool_call
                        .fields
                        .title
                        .clone()
                        .unwrap_or_else(|| "Run a tool".into()),
                    command: command_of(request.tool_call.fields.raw_input.as_ref()),
                    choices: request
                        .options
                        .iter()
                        .map(|option| PermissionChoice {
                            id: option.option_id.to_string(),
                            label: option.name.clone(),
                            decision: decision_of(option.kind),
                        })
                        .collect(),
                    responder: sender,
                };
                // Switchfold's own tools only hand findings and plans back to the app, so
                // some agents asking before each call (Codex does) shouldn't reach the user.
                let own_call = own_calls
                    .lock()
                    .is_ok_and(|own| own.contains(&request.tool_call.tool_call_id.to_string()));
                if own_call || is_own_tool(&prompt.title) {
                    let choice = [Decision::AllowAlways, Decision::AllowOnce]
                        .iter()
                        .find_map(|wanted| {
                            prompt
                                .choices
                                .iter()
                                .find(|choice| choice.decision == *wanted)
                        })
                        .map(|choice| choice.id.clone());
                    let outcome = match choice {
                        Some(option_id) => RequestPermissionOutcome::Selected(
                            SelectedPermissionOutcome::new(option_id),
                        ),
                        None => RequestPermissionOutcome::Cancelled,
                    };
                    return responder.respond(RequestPermissionResponse::new(outcome));
                }
                if permission_events
                    .send(AgentEvent::Permission(prompt))
                    .await
                    .is_err()
                {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }
                // The answer comes from the user, so wait for it off the dispatch loop:
                // the agent keeps streaming updates while the prompt is open.
                connection.spawn(async move {
                    let outcome = match answer.await {
                        Ok(Some(option_id)) => RequestPermissionOutcome::Selected(
                            SelectedPermissionOutcome::new(option_id),
                        ),
                        _ => RequestPermissionOutcome::Cancelled,
                    };
                    responder.respond(RequestPermissionResponse::new(outcome))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            let initialized = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            connected_events
                .send(AgentEvent::Connected {
                    name: initialized
                        .agent_info
                        .as_ref()
                        .map(|info| info.name.clone()),
                    version: initialized
                        .agent_info
                        .as_ref()
                        .map(|info| info.version.clone()),
                })
                .await
                .ok();
            let session = connection
                .send_request(NewSessionRequest::new(cwd).mcp_servers(mcp_servers))
                .block_task()
                .await?;
            let session_id = session.session_id;
            choices
                .apply(
                    &connection,
                    &session_id,
                    session.config_options.unwrap_or_default(),
                )
                .await?;
            let prompt_request = connection.send_request(PromptRequest::new(
                session_id.clone(),
                vec![ContentBlock::Text(TextContent::new(prompt))],
            ));
            let mut response = Box::pin(prompt_request.block_task().fuse());
            let mut cancelled = cancelled.fuse();
            let stop = futures::select! {
                response = response => response?.stop_reason,
                _ = cancelled => {
                    connection.send_notification(CancelNotification::new(session_id))?;
                    response.await?.stop_reason
                }
            };
            Ok(match stop {
                StopReason::EndTurn => RunOutcome::Completed,
                StopReason::Cancelled => RunOutcome::Cancelled,
                StopReason::Refusal => RunOutcome::Refused,
                _ => RunOutcome::Truncated,
            })
        })
        .await
        .map_err(|error| readable_error(&error.to_string()))
}

/// Whether a permission request is for one of Switchfold's MCP tools, as agents title
/// it: `Tool: switchfold/report_finding`, `mcp__switchfold__update_plan`.
fn is_own_tool(title: &str) -> bool {
    ["switchfold/", "mcp__switchfold__"]
        .iter()
        .any(|prefix| title.contains(prefix))
}

/// The human part of an agent error. Agents wrap API errors in JSON, sometimes twice:
/// `Internal error: {"message": "{\"error\":{\"message\":\"The model…\"}}"}`. Returns
/// the innermost message, or `text` itself when there's no JSON to unwrap.
pub fn readable_error(text: &str) -> String {
    fn innermost(value: &serde_json::Value) -> Option<String> {
        let message = value
            .get("error")
            .and_then(innermost)
            .or_else(|| value.get("message").and_then(|m| m.as_str()).map(unwrap))?;
        Some(message)
    }
    fn unwrap(text: &str) -> String {
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(value) => innermost(&value).unwrap_or_else(|| text.to_string()),
            Err(_) => text.to_string(),
        }
    }
    // The JSON can be an object or an object quoted as a string.
    let parsed = text
        .find(['{', '"'])
        .and_then(|start| serde_json::from_str::<serde_json::Value>(&text[start..]).ok());
    match parsed {
        Some(serde_json::Value::String(inner)) if inner.contains('{') => readable_error(&inner),
        Some(value) => innermost(&value).unwrap_or_else(|| text.trim().to_string()),
        None => text.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switchfold_tools_are_recognised_by_title() {
        assert!(is_own_tool("Tool: switchfold/report_finding"));
        assert!(is_own_tool("mcp__switchfold__update_plan"));
        assert!(!is_own_tool("Run cargo test"));
    }

    #[test]
    fn nested_api_errors_read_as_their_message() {
        let raw = "Internal error: {\n  \"message\": \"{\\\"type\\\":\\\"error\\\",\\\"status\\\":400,\\\"error\\\":{\\\"type\\\":\\\"invalid_request_error\\\",\\\"message\\\":\\\"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account.\\\"}}\",\n  \"codex_error_info\": \"other\"\n}";
        assert_eq!(
            readable_error(raw),
            "The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account."
        );
        assert_eq!(readable_error("connection closed"), "connection closed");
    }

    #[test]
    fn commands_come_from_raw_input() {
        let raw = serde_json::json!({ "command": ["cargo", "test", "-p", "review"] });
        assert_eq!(
            command_of(Some(&raw)).as_deref(),
            Some("cargo test -p review")
        );
        let raw = serde_json::json!({ "command": "cargo test" });
        assert_eq!(command_of(Some(&raw)).as_deref(), Some("cargo test"));
        assert_eq!(command_of(None), None);
    }

    #[test]
    fn permission_answers_fall_back_to_offered_options() {
        let (sender, mut receiver) = oneshot::channel();
        let prompt = PermissionPrompt {
            title: "Run".into(),
            command: None,
            choices: vec![
                PermissionChoice {
                    id: "once".into(),
                    label: "Allow".into(),
                    decision: Decision::AllowOnce,
                },
                PermissionChoice {
                    id: "no".into(),
                    label: "Reject".into(),
                    decision: Decision::Deny,
                },
            ],
            responder: sender,
        };
        assert!(!prompt.offers(Decision::AllowAlways));
        prompt.respond(Decision::AllowAlways);
        assert_eq!(receiver.try_recv().unwrap(), Some(Some("once".into())));
    }
}

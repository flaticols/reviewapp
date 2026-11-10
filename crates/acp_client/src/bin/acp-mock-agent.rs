//! A scripted ACP agent for tests and demos. Asked a question about selected lines, it
//! reads their file, answers and suggests a comment. For any other prompt it reads the changed files
//! the prompt lists, asks permission to run `cargo test`, reports one finding through
//! the first MCP server it was given, and ends the turn. Its sessions offer a mode, a
//! model, and an effort that only the `deep` model has, as Codex's do. The `unsupported`
//! model fails the prompt with a nested API error, as Codex does for models an account
//! can't use.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio as ProcessStdio};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, ContentChunk, Implementation, InitializeRequest,
    InitializeResponse, McpServer, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus, PromptRequest,
    PromptResponse, RequestPermissionRequest, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOption, SessionId, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, StopReason, TextContent,
    ToolCall, ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Agent, Stdio};
use serde_json::{Value, json};

const STEP: Duration = Duration::from_millis(150);

/// MCP servers from the latest `session/new`, used when the prompt arrives.
static SERVERS: std::sync::Mutex<Vec<McpServer>> = std::sync::Mutex::new(Vec::new());

/// The session's mode, model and effort.
static CONFIG: std::sync::Mutex<Config> = std::sync::Mutex::new(Config {
    mode: String::new(),
    model: String::new(),
    effort: String::new(),
});

struct Config {
    mode: String,
    model: String,
    effort: String,
}

impl Config {
    fn reset(&mut self) {
        self.mode = "auto".into();
        self.model = "fast".into();
        self.effort = String::new();
    }

    fn options(&self) -> Vec<SessionConfigOption> {
        let select = |id: &str, current: &str, values: &[&str], category| {
            SessionConfigOption::select(
                id.to_string(),
                id.to_string(),
                current.to_string(),
                values
                    .iter()
                    .map(|value| {
                        SessionConfigSelectOption::new(value.to_string(), value.to_string())
                    })
                    .collect::<Vec<_>>(),
            )
            .category(category)
        };
        let mut options = vec![
            select(
                "mode",
                &self.mode,
                &["auto", "default"],
                SessionConfigOptionCategory::Mode,
            ),
            select(
                "model",
                &self.model,
                &["fast", "deep", "unsupported"],
                SessionConfigOptionCategory::Model,
            ),
        ];
        if self.model == "deep" {
            options.push(select(
                "reasoning_effort",
                &self.effort,
                &["low", "high"],
                SessionConfigOptionCategory::ThoughtLevel,
            ));
        }
        options
    }

    fn set(&mut self, id: &str, value: &str) {
        match id {
            "mode" => self.mode = value.into(),
            "model" => {
                self.model = value.into();
                self.effort = if value == "deep" {
                    "low".into()
                } else {
                    String::new()
                };
            }
            "reasoning_effort" => self.effort = value.into(),
            _ => {}
        }
    }
}

fn prompt_text(request: &PromptRequest) -> String {
    request
        .prompt
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

/// Paths from the prompt's `- path (+a −b)` lines.
fn changed_files(prompt: &str) -> Vec<String> {
    prompt
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("- ")?;
            let (path, counts) = rest.rsplit_once(" (+")?;
            counts.contains('−').then(|| path.to_string())
        })
        .collect()
}

/// The file a question prompt asks about, from its first line: `… asks about lines 3–9
/// of `path`.` Review prompts have no such line.
fn question_file(prompt: &str) -> Option<String> {
    let first = prompt.lines().next()?;
    let (_, rest) = first.split_once(" asks about ")?;
    let (_, path) = rest.split_once(" of `")?;
    Some(path.split('`').next()?.to_string())
}

/// Calls tool `name` of a stdio MCP server, as an agent would.
fn call_tool(server: &McpServer, name: &str, arguments: Value) -> Result<(), String> {
    let McpServer::Stdio(stdio) = server else {
        return Err("only stdio MCP servers are supported".into());
    };
    let mut child = Command::new(&stdio.command)
        .args(&stdio.args)
        .stdin(ProcessStdio::piped())
        .stdout(ProcessStdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
    let mut call = |message: Value| -> Result<Option<Value>, String> {
        writeln!(stdin, "{message}").map_err(|e| e.to_string())?;
        if message.get("id").is_none() {
            return Ok(None);
        }
        let mut line = String::new();
        stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        serde_json::from_str(&line)
            .map(Some)
            .map_err(|e| e.to_string())
    };
    call(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"acp-mock-agent","version":"1"}}}),
    )?;
    call(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
    let result = call(
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
    )?;
    drop(stdin);
    child.wait().ok();
    match result {
        Some(response) if response["result"]["isError"] != json!(true) => Ok(()),
        other => Err(format!("{name} failed: {other:?}")),
    }
}

fn main() {
    let result = futures::executor::block_on(
        Agent
            .builder()
            .name("acp-mock-agent")
            .on_receive_request(
                async move |initialize: InitializeRequest, responder, _cx| {
                    responder.respond(
                        InitializeResponse::new(initialize.protocol_version)
                            .agent_capabilities(AgentCapabilities::new())
                            .agent_info(Implementation::new("acp-mock-agent", "1.0.0")),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: NewSessionRequest, responder, _cx| {
                    if let Ok(mut servers) = SERVERS.lock() {
                        *servers = request.mcp_servers.clone();
                    }
                    let options = CONFIG
                        .lock()
                        .map(|mut config| {
                            config.reset();
                            config.options()
                        })
                        .unwrap_or_default();
                    responder.respond(
                        NewSessionResponse::new(SessionId::new("mock-session"))
                            .config_options(options),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: SetSessionConfigOptionRequest, responder, _cx| {
                    let value = request
                        .value
                        .as_value_id()
                        .map(|value| value.to_string())
                        .unwrap_or_default();
                    let options = CONFIG
                        .lock()
                        .map(|mut config| {
                            config.set(&request.config_id.to_string(), &value);
                            config.options()
                        })
                        .unwrap_or_default();
                    responder.respond(SetSessionConfigOptionResponse::new(options))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |request: PromptRequest, responder, cx| {
                    let unsupported = CONFIG
                        .lock()
                        .map(|config| config.model == "unsupported")
                        .unwrap_or(false);
                    if unsupported {
                        return responder.respond_with_error(
                            agent_client_protocol::util::internal_error(
                                r#"{"message": "{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'unsupported' model is not supported when using this mock agent with a ChatGPT account.\"}}", "codex_error_info": "other"}"#,
                            ),
                        );
                    }
                    let connection = cx.clone();
                    cx.spawn(async move {
                        let session = request.session_id.clone();
                        let prompt = prompt_text(&request);
                        let servers: Vec<McpServer> =
                            SERVERS.lock().map(|servers| servers.clone()).unwrap_or_default();
                        if let Some(path) = question_file(&prompt) {
                            let read = |status| {
                                SessionNotification::new(
                                    session.clone(),
                                    SessionUpdate::ToolCall(
                                        ToolCall::new("read-question", format!("Read {path}"))
                                            .kind(ToolKind::Read)
                                            .status(status)
                                            .locations(vec![ToolCallLocation::new(path.clone())]),
                                    ),
                                )
                            };
                            connection.send_notification(read(ToolCallStatus::InProgress))?;
                            async_io::Timer::after(STEP).await;
                            connection.send_notification(read(ToolCallStatus::Completed))?;
                            for chunk in [
                                "**It keeps the two orders in step.** ",
                                "`apply_push` rebuilds `self.hunks` from the pushed commits, and the lines you selected re-anchor every open thread to its new position before the next frame reads them.\n\nWithout the second pass a thread on a moved line would point at whatever now sits at its old number.",
                            ] {
                                connection.send_notification(SessionNotification::new(
                                    session.clone(),
                                    SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                        ContentBlock::Text(TextContent::new(chunk)),
                                    )),
                                ))?;
                                async_io::Timer::after(STEP).await;
                            }
                            if let Some(server) = servers.first() {
                                call_tool(
                                    server,
                                    "suggest_comment",
                                    json!({ "body": "Could the re-anchoring move into `apply_push` itself, so no caller can forget it?" }),
                                )
                                .map_err(agent_client_protocol::util::internal_error)?;
                            }
                            return responder.respond(PromptResponse::new(StopReason::EndTurn));
                        }
                        let files = changed_files(&prompt);
                        let plan = |done: usize| {
                            let steps = ["Read the changed files", "Run the tests", "Report findings"];
                            SessionNotification::new(
                                session.clone(),
                                SessionUpdate::Plan(Plan::new(
                                    steps
                                        .iter()
                                        .enumerate()
                                        .map(|(ix, step)| {
                                            let status = if ix < done {
                                                PlanEntryStatus::Completed
                                            } else if ix == done {
                                                PlanEntryStatus::InProgress
                                            } else {
                                                PlanEntryStatus::Pending
                                            };
                                            PlanEntry::new(*step, PlanEntryPriority::Medium, status)
                                        })
                                        .collect(),
                                )),
                            )
                        };
                        connection.send_notification(plan(0))?;
                        for (ix, path) in files.iter().enumerate() {
                            let id = format!("read-{ix}");
                            connection.send_notification(SessionNotification::new(
                                session.clone(),
                                SessionUpdate::ToolCall(
                                    ToolCall::new(id.clone(), format!("Read {path}"))
                                        .kind(ToolKind::Read)
                                        .status(ToolCallStatus::InProgress)
                                        .locations(vec![ToolCallLocation::new(path.clone())]),
                                ),
                            ))?;
                            async_io::Timer::after(STEP).await;
                            connection.send_notification(SessionNotification::new(
                                session.clone(),
                                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                                    id,
                                    ToolCallUpdateFields::new()
                                        .status(ToolCallStatus::Completed)
                                        .locations(vec![ToolCallLocation::new(path.clone())]),
                                )),
                            ))?;
                        }
                        connection.send_notification(plan(1))?;
                        let command = "cargo test -p review --lib thread::";
                        let permission = connection
                            .send_request(RequestPermissionRequest::new(
                                session.clone(),
                                ToolCallUpdate::new(
                                    "run-tests",
                                    ToolCallUpdateFields::new()
                                        .title(format!("Run {command}"))
                                        .kind(ToolKind::Execute)
                                        .raw_input(json!({ "command": command })),
                                ),
                                vec![
                                    PermissionOption::new("allow", "Allow once", PermissionOptionKind::AllowOnce),
                                    PermissionOption::new("always", "Always allow", PermissionOptionKind::AllowAlways),
                                    PermissionOption::new("reject", "Reject", PermissionOptionKind::RejectOnce),
                                ],
                            ))
                            .block_task()
                            .await?;
                        let allowed = format!("{:?}", permission.outcome).contains("allow");
                        if let (Some(server), Some(path)) = (servers.first(), files.first()) {
                            call_tool(
                                server,
                                "report_finding",
                                json!({
                                    "path": path,
                                    "line": 31,
                                    "body": "Possible race: `apply_push` mutates `self.hunks` while `render` holds an iterator.",
                                }),
                            )
                            .map_err(agent_client_protocol::util::internal_error)?;
                        }
                        connection.send_notification(plan(3))?;
                        connection.send_notification(SessionNotification::new(
                            session.clone(),
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                                TextContent::new(format!(
                                    "Reviewed {} files; tests {}. {}",
                                    files.len(),
                                    if allowed { "ran" } else { "were not run" },
                                    CONFIG
                                        .lock()
                                        .map(|c| format!("mode={} model={} effort={}", c.mode, c.model, c.effort))
                                        .unwrap_or_default()
                                )),
                            ))),
                        ))?;
                        responder.respond(PromptResponse::new(StopReason::EndTurn))
                    })
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_to(Stdio::new()),
    );
    if let Err(error) = result {
        eprintln!("acp-mock-agent: {error}");
        std::process::exit(1);
    }
}

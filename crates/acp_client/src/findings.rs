//! Switchfold's MCP tools for agents: `report_finding`, `update_plan` and
//! `suggest_comment`. The agent
//! launches Switchfold's binary as a stdio MCP server ([`serve_stdio`]); each tool call
//! is forwarded over a Unix socket to the running app, which listens with
//! [`FindingsListener`].

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A finding as the agent reported it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedFinding {
    /// Path relative to the repository root.
    pub path: String,
    /// Line on the head side.
    pub line: u32,
    /// Markdown explanation of the problem.
    pub body: String,
}

/// One step of the plan an agent shares through `update_plan`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedStep {
    pub step: String,
    /// `pending`, `in_progress` or `completed`.
    pub status: String,
}

/// What a tool call sends to the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Finding(ReportedFinding),
    /// The whole plan, replacing the previous one.
    Plan {
        steps: Vec<ReportedStep>,
    },
    /// The comment an agent proposes after answering a question about selected lines.
    Suggestion {
        body: String,
    },
}

/// The tool agents call to report a finding.
pub const TOOL_NAME: &str = "report_finding";

/// The tool agents call to share their plan.
pub const PLAN_TOOL_NAME: &str = "update_plan";

/// The tool agents call to propose a comment on the lines they were asked about.
pub const SUGGEST_TOOL_NAME: &str = "suggest_comment";

/// The MCP protocol version the server speaks.
const PROTOCOL_VERSION: &str = "2025-06-18";

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

/// Receives findings for one run on a fresh Unix socket in the temporary directory.
pub struct FindingsListener {
    listener: UnixListener,
    path: PathBuf,
}

impl FindingsListener {
    /// Binds a new socket.
    pub fn bind() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "switchfold-findings-{}-{}.sock",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::remove_file(&path).ok();
        let listener = UnixListener::bind(&path)?;
        Ok(Self { listener, path })
    }

    /// Where the MCP server should connect.
    pub fn socket_path(&self) -> &Path {
        &self.path
    }

    /// Calls `on_report` for every report received, on a dedicated thread, until the
    /// process exits. The socket file is removed when the thread ends.
    pub fn forward(self, on_report: impl Fn(Report) + Send + 'static) {
        std::thread::spawn(move || {
            for stream in self.listener.incoming() {
                let Ok(stream) = stream else { break };
                let reader = BufReader::new(stream);
                for line in reader.lines().map_while(Result::ok) {
                    if let Ok(report) = serde_json::from_str::<Report>(&line) {
                        on_report(report);
                    }
                }
            }
            std::fs::remove_file(&self.path).ok();
        });
    }
}

/// Runs the MCP server on stdin and stdout until stdin closes, forwarding each tool call
/// to the app listening on `socket`.
pub fn serve_stdio(socket: &Path) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) = handle(&message, |report| send_report(socket, report)) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn send_report(socket: &Path, report: &Report) -> Result<(), String> {
    let mut stream = UnixStream::connect(socket)
        .map_err(|error| format!("Switchfold isn’t listening: {error}"))?;
    let line = serde_json::to_string(report).map_err(|error| error.to_string())?;
    writeln!(stream, "{line}").map_err(|error| error.to_string())
}

/// Handles one JSON-RPC message; `None` for notifications, which get no response.
fn handle(message: &Value, send: impl Fn(&Report) -> Result<(), String>) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "switchfold", "version": env!("CARGO_PKG_VERSION") },
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": [finding_tool(), plan_tool(), suggestion_tool()] }),
        "tools/call" => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            let parsed = match name {
                TOOL_NAME => serde_json::from_value::<ReportedFinding>(arguments)
                    .map(Report::Finding)
                    .map_err(|problem| problem.to_string()),
                PLAN_TOOL_NAME => arguments
                    .get("steps")
                    .cloned()
                    .ok_or_else(|| "`steps` is required".to_string())
                    .and_then(|steps| {
                        serde_json::from_value::<Vec<ReportedStep>>(steps)
                            .map_err(|problem| problem.to_string())
                    })
                    .map(|steps| Report::Plan { steps }),
                SUGGEST_TOOL_NAME => arguments
                    .get("body")
                    .and_then(Value::as_str)
                    .map(|body| Report::Suggestion {
                        body: body.trim().to_string(),
                    })
                    .ok_or_else(|| "`body` is required".to_string()),
                _ => return Some(error(id, -32602, &format!("Unknown tool {name}"))),
            };
            match parsed {
                Ok(report) => match send(&report) {
                    Ok(()) => json!({
                        "content": [{ "type": "text", "text": acknowledgement(&report) }],
                    }),
                    Err(message) => json!({
                        "content": [{ "type": "text", "text": message }],
                        "isError": true,
                    }),
                },
                Err(problem) => json!({
                    "content": [{ "type": "text", "text": format!("Invalid arguments: {problem}") }],
                    "isError": true,
                }),
            }
        }
        _ => return Some(error(id, -32601, &format!("Method not found: {method}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn acknowledgement(report: &Report) -> String {
    match report {
        Report::Finding(finding) => {
            format!("Recorded finding at {}:{}.", finding.path, finding.line)
        }
        Report::Plan { steps } => format!(
            "Plan updated: {} of {} steps done.",
            steps.iter().filter(|s| s.status == "completed").count(),
            steps.len()
        ),
        Report::Suggestion { .. } => "Shown to the reviewer as a suggested comment.".into(),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn plan_tool() -> Value {
    json!({
        "name": PLAN_TOOL_NAME,
        "description": "Share your review plan with the reviewer, who watches it to follow your progress. Send the whole list every time: first when you've planned, then whenever a step starts or finishes.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "step": { "type": "string", "description": "What the step covers, such as a file or a group of files." },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                        },
                        "required": ["step", "status"]
                    }
                }
            },
            "required": ["steps"]
        }
    })
}

fn suggestion_tool() -> Value {
    json!({
        "name": SUGGEST_TOOL_NAME,
        "description": "Propose the comment the reviewer could post on the lines they asked you about. Call it once, after you've answered their question. Switchfold shows it as a draft the reviewer can edit; nothing is posted to GitHub.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "body": { "type": "string", "description": "The comment in Markdown, written as the reviewer would write it to the author: one to three sentences, specific and actionable." }
            },
            "required": ["body"]
        }
    })
}

fn finding_tool() -> Value {
    json!({
        "name": TOOL_NAME,
        "description": "Report one concrete problem in the pull request under review. Call it once per problem. Switchfold shows it to the reviewer as a draft comment; nothing is posted to GitHub.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to the repository root." },
                "line": { "type": "integer", "minimum": 1, "description": "Line number in the new version of the file." },
                "body": { "type": "string", "description": "What is wrong and how to fix it, in Markdown. Two or three sentences." }
            },
            "required": ["path", "line", "body"]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn lists_and_calls_the_tool() {
        let listed = handle(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(listed["result"]["tools"][0]["name"], TOOL_NAME);
        assert_eq!(listed["result"]["tools"][1]["name"], PLAN_TOOL_NAME);

        let recorded = Arc::new(Mutex::new(Vec::new()));
        let sink = recorded.clone();
        let response = handle(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"report_finding","arguments":{"path":"src/a.rs","line":3,"body":"Off by one."}}}),
            move |report| {
                sink.lock().unwrap().push(report.clone());
                Ok(())
            },
        )
        .unwrap();
        assert!(response["result"]["isError"].is_null());
        assert!(matches!(&recorded.lock().unwrap()[0], Report::Finding(f) if f.line == 3));

        let sink = recorded.clone();
        let response = handle(
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"update_plan","arguments":{"steps":[{"step":"common/mappings_inline.go","status":"completed"},{"step":"goldens","status":"in_progress"}]}}}),
            move |report| {
                sink.lock().unwrap().push(report.clone());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            response["result"]["content"][0]["text"],
            "Plan updated: 1 of 2 steps done."
        );
        assert!(matches!(&recorded.lock().unwrap()[1], Report::Plan { steps } if steps.len() == 2));
        let sink = recorded.clone();
        handle(
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"suggest_comment","arguments":{"body":" Guard the nil case. "}}}),
            move |report| {
                sink.lock().unwrap().push(report.clone());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(listed["result"]["tools"][2]["name"], SUGGEST_TOOL_NAME);
        assert_eq!(
            recorded.lock().unwrap()[2],
            Report::Suggestion {
                body: "Guard the nil case.".into()
            }
        );
        assert!(
            handle(
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                |_| Ok(())
            )
            .is_none()
        );
    }

    #[test]
    fn findings_travel_over_the_socket() {
        let listener = FindingsListener::bind().unwrap();
        let socket = listener.socket_path().to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        listener.forward(move |report| sender.send(report).unwrap());
        let finding = Report::Finding(ReportedFinding {
            path: "a.rs".into(),
            line: 7,
            body: "x".into(),
        });
        send_report(&socket, &finding).unwrap();
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            finding
        );
    }
}

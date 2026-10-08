//! The MCP server Zed's agent talks to. Zed starts `switchfold --zed-mcp`, which serves
//! MCP on stdio ([`serve_stdio`]) and forwards each tool call over a Unix socket to the
//! running app, which answers through a [`BridgeListener`].

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The MCP protocol version the server speaks.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// How long a tool call waits for the app, which may be loading a pull request.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// Where the running app listens, under `home`.
pub fn socket_path(home: &Path) -> PathBuf {
    home.join(".switchfold/zed.sock")
}

/// One tool call, as forwarded to the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BridgeRequest {
    pub tool: String,
    pub arguments: Value,
}

impl BridgeRequest {
    /// A call of `tool` with `arguments`.
    pub fn new(tool: impl Into<String>, arguments: Value) -> Self {
        Self {
            tool: tool.into(),
            arguments,
        }
    }

    /// The string argument `name`, trimmed, when present and not empty.
    pub fn text(&self, name: &str) -> Option<String> {
        self.arguments
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    }

    /// The whole-number argument `name`.
    pub fn number(&self, name: &str) -> Option<u64> {
        let value = self.arguments.get(name)?;
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
    }

    /// The boolean argument `name`, `false` when absent.
    pub fn flag(&self, name: &str) -> bool {
        self.arguments
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
}

/// The app's answer to a tool call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BridgeReply {
    pub text: String,
    pub is_error: bool,
}

impl BridgeReply {
    /// A successful answer.
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    /// A failed call, explained for the agent.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }

    /// `value` as pretty JSON.
    pub fn json(value: &Value) -> Self {
        Self::ok(serde_json::to_string_pretty(value).unwrap_or_default())
    }
}

/// The app's end of the socket.
pub struct BridgeListener {
    listener: UnixListener,
    path: PathBuf,
}

impl BridgeListener {
    /// Listens at `path`, replacing a socket left by an app that quit without cleaning
    /// up. Fails when another running app already answers there.
    pub fn bind(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() {
            if UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another Switchfold is already answering Zed",
                ));
            }
            std::fs::remove_file(path)?;
        }
        Ok(Self {
            listener: UnixListener::bind(path)?,
            path: path.to_path_buf(),
        })
    }

    /// Answers every request with `answer`, each connection on its own thread, until the
    /// process exits.
    pub fn serve(self, answer: impl Fn(BridgeRequest) -> BridgeReply + Send + Sync + 'static) {
        let answer = std::sync::Arc::new(answer);
        std::thread::spawn(move || {
            for stream in self.listener.incoming() {
                let Ok(stream) = stream else { break };
                let answer = answer.clone();
                std::thread::spawn(move || {
                    let Ok(mut writer) = stream.try_clone() else {
                        return;
                    };
                    for line in BufReader::new(stream).lines().map_while(Result::ok) {
                        let reply = match serde_json::from_str::<BridgeRequest>(&line) {
                            Ok(request) => answer(request),
                            Err(error) => BridgeReply::error(format!("Malformed request: {error}")),
                        };
                        let Ok(line) = serde_json::to_string(&reply) else {
                            break;
                        };
                        if writeln!(writer, "{line}").is_err() {
                            break;
                        }
                    }
                });
            }
            std::fs::remove_file(&self.path).ok();
        });
    }
}

/// Sends `request` to the app at `socket` and waits for its answer.
pub fn call(socket: &Path, request: &BridgeRequest) -> BridgeReply {
    let attempt = || -> io::Result<BridgeReply> {
        let stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
        let mut writer = stream.try_clone()?;
        writeln!(writer, "{}", serde_json::to_string(request)?)?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    };
    attempt().unwrap_or_else(|error| {
        BridgeReply::error(format!(
            "Switchfold isn’t answering ({error}). Open Switchfold Review and try again."
        ))
    })
}

/// Runs the MCP server on stdin and stdout until stdin closes, forwarding each tool call
/// to the app listening at `socket`.
pub fn serve_stdio(socket: &Path) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) = handle(&message, |request| call(socket, request)) {
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Handles one JSON-RPC message; `None` for notifications, which get no response.
fn handle(message: &Value, call: impl Fn(&BridgeRequest) -> BridgeReply) -> Option<Value> {
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
            "instructions": "Switchfold Review is the reviewer's GitHub pull request app. Use these tools to read the review threads and agent findings of the pull request open there, and to draft pending review comments. Pending comments stay in Switchfold until the reviewer submits the review; nothing here posts to GitHub.",
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !tools().iter().any(|tool| tool["name"] == name) {
                return Some(error(id, -32602, &format!("Unknown tool {name}")));
            }
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let reply = call(&BridgeRequest::new(name, arguments));
            json!({
                "content": [{ "type": "text", "text": reply.text }],
                "isError": reply.is_error,
            })
        }
        _ => return Some(error(id, -32601, &format!("Method not found: {method}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Tool names, as the app matches them.
pub mod tool {
    pub const CURRENT: &str = "current_pull_request";
    pub const LIST: &str = "list_pull_requests";
    pub const THREADS: &str = "list_review_threads";
    pub const FINDINGS: &str = "list_agent_findings";
    pub const COMMENT: &str = "add_pending_comment";
    pub const REPLY: &str = "reply_to_thread";
    pub const SHOW: &str = "show_in_switchfold";
}

fn tools() -> Vec<Value> {
    let pull_request = json!({
        "type": "string",
        "description": "The pull request as owner/name#number. Defaults to the one open in Switchfold."
    });
    vec![
        json!({
            "name": tool::CURRENT,
            "description": "The pull request open in Switchfold: title, branches, description, changed files with line counts, and the local checkout to read its code in.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true },
        }),
        json!({
            "name": tool::LIST,
            "description": "Pull requests in the reviewer's inboxes: waiting for their review, theirs, and ones they're mentioned in.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true },
        }),
        json!({
            "name": tool::THREADS,
            "description": "Review comment threads on a pull request, each anchored to a file and line, with every comment and whether it is resolved or still a pending draft.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pull_request": pull_request,
                    "include_resolved": { "type": "boolean", "description": "Include resolved threads. Defaults to false." }
                }
            },
            "annotations": { "readOnlyHint": true },
        }),
        json!({
            "name": tool::FINDINGS,
            "description": "Problems agent reviews found in a pull request, with file, line and state (draft, added to the review, or dismissed).",
            "inputSchema": { "type": "object", "properties": { "pull_request": pull_request } },
            "annotations": { "readOnlyHint": true },
        }),
        json!({
            "name": tool::COMMENT,
            "description": "Drafts a review comment on a line of a pull request. It joins the reviewer's pending review in Switchfold and is posted only when they submit it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pull_request": pull_request,
                    "path": { "type": "string", "description": "File path relative to the repository root." },
                    "line": { "type": "integer", "minimum": 1, "description": "Line number in the new version of the file." },
                    "body": { "type": "string", "description": "The comment, in Markdown." }
                },
                "required": ["path", "line", "body"]
            },
        }),
        json!({
            "name": tool::REPLY,
            "description": "Drafts a reply to a review thread. It joins the reviewer's pending review in Switchfold and is posted only when they submit it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pull_request": pull_request,
                    "thread_id": { "type": "string", "description": "The thread's id from list_review_threads." },
                    "body": { "type": "string", "description": "The reply, in Markdown." }
                },
                "required": ["thread_id", "body"]
            },
        }),
        json!({
            "name": tool::SHOW,
            "description": "Brings Switchfold to the front showing a pull request, optionally scrolled to a file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pull_request": pull_request,
                    "path": { "type": "string", "description": "File path relative to the repository root." }
                }
            },
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_tool_calls_and_rejects_unknown_ones() {
        let listed = handle(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            |_| unreachable!(),
        )
        .unwrap();
        assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 7);

        let reply = handle(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_pending_comment","arguments":{"path":"a.rs","line":3,"body":"Hm"}}}),
            |request| {
                assert_eq!(request.tool, tool::COMMENT);
                assert_eq!(request.number("line"), Some(3));
                BridgeReply::ok("Drafted.")
            },
        )
        .unwrap();
        assert_eq!(reply["result"]["content"][0]["text"], "Drafted.");
        assert_eq!(reply["result"]["isError"], false);

        let unknown = handle(
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"submit_review"}}),
            |_| unreachable!(),
        )
        .unwrap();
        assert_eq!(unknown["error"]["code"], -32602);
    }

    #[test]
    fn talks_to_the_app_over_the_socket() {
        let path = std::env::temp_dir().join(format!("zed-bridge-{}.sock", std::process::id()));
        std::fs::remove_file(&path).ok();
        BridgeListener::bind(&path)
            .unwrap()
            .serve(|request| BridgeReply::ok(format!("got {}", request.tool)));
        let reply = call(&path, &BridgeRequest::new(tool::CURRENT, json!({})));
        assert_eq!(reply, BridgeReply::ok("got current_pull_request"));
        assert!(
            BridgeListener::bind(&path).is_err(),
            "a second app can't take over a live socket"
        );
        std::fs::remove_file(&path).ok();
        assert!(call(&path, &BridgeRequest::new(tool::CURRENT, json!({}))).is_error);
    }
}

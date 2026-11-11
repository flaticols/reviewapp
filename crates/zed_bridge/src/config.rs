//! Adds Switchfold to Zed's own configuration files without disturbing them: edits are
//! text insertions found by tokenizing the JSON-with-comments, so comments, trailing
//! commas and formatting survive. The first edit of a file keeps a copy of the original
//! next to it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The label of the Zed task that comments on the cursor's line.
pub const COMMENT_TASK_LABEL: &str = "Comment on this line in Switchfold";

/// The name Switchfold's MCP server has in Zed's `context_servers`.
const SERVER_NAME: &str = "switchfold";

/// Why a Zed configuration file couldn't be changed.
#[derive(Debug)]
pub enum ConfigError {
    Io(PathBuf, io::Error),
    /// The file isn't the shape Zed expects, so it is left alone.
    Unexpected(PathBuf, &'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, error) => write!(f, "Couldn’t update {}: {error}", path.display()),
            Self::Unexpected(path, why) => {
                write!(f, "Left {} unchanged: {why}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// What installing did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installed {
    Added,
    AlreadyThere,
}

/// Whether Zed knows Switchfold's MCP server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpStatus {
    Missing,
    Installed,
}

/// Zed's configuration folder under `home`.
pub fn zed_config_dir(home: &Path) -> PathBuf {
    home.join(".config/zed")
}

/// A space-free link to the app's binary under `home`, for Zed tasks, whose commands go
/// through a shell.
pub fn shim_path(home: &Path) -> PathBuf {
    home.join(".switchfold/bin/switchfold")
}

/// Points the shim at `binary`.
fn install_shim(home: &Path, binary: &Path) -> Result<PathBuf, ConfigError> {
    let shim = shim_path(home);
    let io_error = |error| ConfigError::Io(shim.clone(), error);
    if fs::read_link(&shim).ok().as_deref() == Some(binary) {
        return Ok(shim);
    }
    if let Some(parent) = shim.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    match fs::remove_file(&shim) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(io_error(error)),
        _ => {}
    }
    std::os::unix::fs::symlink(binary, &shim).map_err(io_error)?;
    Ok(shim)
}

/// Whether Zed's settings under `home` list Switchfold's MCP server.
pub fn mcp_status(home: &Path) -> McpStatus {
    let path = zed_config_dir(home).join("settings.json");
    let installed = fs::read_to_string(path)
        .ok()
        .and_then(|text| parse_jsonc(&text))
        .is_some_and(|settings| settings["context_servers"][SERVER_NAME].is_object());
    if installed {
        McpStatus::Installed
    } else {
        McpStatus::Missing
    }
}

/// Whether Zed's global tasks under `home` include the comment task.
pub fn comment_task_installed(home: &Path) -> bool {
    fs::read_to_string(zed_config_dir(home).join("tasks.json"))
        .ok()
        .and_then(|text| parse_jsonc(&text))
        .and_then(|tasks| tasks.as_array().cloned())
        .is_some_and(|tasks| tasks.iter().any(|t| t["label"] == COMMENT_TASK_LABEL))
}

/// Adds Switchfold's MCP server to Zed's `context_servers`, run from `binary` with
/// `--zed-mcp`. Zed's agent then sees the review tools.
pub fn install_mcp_server(home: &Path, binary: &Path) -> Result<Installed, ConfigError> {
    let shim = install_shim(home, binary)?;
    let entry = json!({
        "command": shim.display().to_string(),
        "args": ["--zed-mcp"],
        "env": {},
    });
    edit_file(
        &zed_config_dir(home).join("settings.json"),
        "{\n}\n",
        |text| insert_member(text, &["context_servers"], SERVER_NAME, &entry),
        |settings| settings["context_servers"][SERVER_NAME].is_object(),
    )
}

/// Adds the task that comments on the cursor's line in Switchfold to Zed's global tasks.
pub fn install_comment_task(home: &Path, binary: &Path) -> Result<Installed, ConfigError> {
    let shim = install_shim(home, binary)?;
    let task = json!({
        "label": COMMENT_TASK_LABEL,
        "command": shim.display().to_string(),
        "args": ["comment", "--file", "$ZED_FILE", "--line", "$ZED_ROW"],
        "reveal": "never",
        "hide": "always",
        "allow_concurrent_runs": true,
    });
    let has_task = |tasks: &Value| {
        tasks
            .as_array()
            .is_some_and(|tasks| tasks.iter().any(|t| t["label"] == COMMENT_TASK_LABEL))
    };
    edit_file(
        &zed_config_dir(home).join("tasks.json"),
        "[\n]\n",
        |text| {
            let parsed = parse_jsonc(text).ok_or("it isn’t valid JSON")?;
            if has_task(&parsed) {
                return Ok(None);
            }
            append_item(text, &task).map(Some)
        },
        has_task,
    )
}

/// The keymap entry that runs the comment task, to paste into Zed's keymap.
pub fn comment_task_keybinding() -> String {
    format!(
        "{{\n  \"context\": \"Editor\",\n  \"bindings\": {{\n    \"cmd-alt-shift-c\": [\"task::Spawn\", {{ \"task_name\": \"{COMMENT_TASK_LABEL}\" }}]\n  }}\n}}"
    )
}

/// Reads `path` (or `empty` when it doesn't exist), applies `edit`, checks the result
/// still parses and passes `check`, keeps a one-time backup, and writes it.
fn edit_file(
    path: &Path,
    empty: &str,
    edit: impl FnOnce(&str) -> Result<Option<String>, &'static str>,
    check: impl Fn(&Value) -> bool,
) -> Result<Installed, ConfigError> {
    let io_error = |error| ConfigError::Io(path.to_path_buf(), error);
    let original = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(error)),
    };
    let text = original.as_deref().unwrap_or(empty);
    let Some(edited) = edit(text).map_err(|why| ConfigError::Unexpected(path.into(), why))? else {
        return Ok(Installed::AlreadyThere);
    };
    if !parse_jsonc(&edited).is_some_and(|value| check(&value)) {
        return Err(ConfigError::Unexpected(
            path.into(),
            "the edit wouldn’t have produced valid settings",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    if let Some(original) = &original {
        let backup = path.with_extension("json.before-switchfold");
        if !backup.exists() {
            fs::write(&backup, original).map_err(io_error)?;
        }
    }
    fs::write(path, edited).map_err(io_error)?;
    Ok(Installed::Added)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Open,
    Close,
    Comma,
    Colon,
    String,
    Literal,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
    /// Brace and bracket depth before the token.
    depth: usize,
}

/// Splits JSON with comments into tokens, dropping whitespace and comments. `None` for
/// an unterminated string or block comment.
fn tokenize(text: &str) -> Option<Vec<Token>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut depth = 0usize;
    let mut ix = 0;
    while ix < bytes.len() {
        let start = ix;
        let kind = match bytes[ix] {
            b' ' | b'\t' | b'\n' | b'\r' => {
                ix += 1;
                continue;
            }
            b'/' if bytes.get(ix + 1) == Some(&b'/') => {
                while ix < bytes.len() && bytes[ix] != b'\n' {
                    ix += 1;
                }
                continue;
            }
            b'/' if bytes.get(ix + 1) == Some(&b'*') => {
                ix += 2 + text[ix + 2..].find("*/")? + 2;
                continue;
            }
            b'"' => {
                ix += 1;
                loop {
                    match bytes.get(ix)? {
                        b'\\' => ix += 2,
                        b'"' => break,
                        _ => ix += 1,
                    }
                }
                ix += 1;
                Kind::String
            }
            b'{' | b'[' => {
                ix += 1;
                Kind::Open
            }
            b'}' | b']' => {
                ix += 1;
                Kind::Close
            }
            b',' => {
                ix += 1;
                Kind::Comma
            }
            b':' => {
                ix += 1;
                Kind::Colon
            }
            _ => {
                while ix < bytes.len()
                    && !matches!(
                        bytes[ix],
                        b' ' | b'\t'
                            | b'\n'
                            | b'\r'
                            | b','
                            | b':'
                            | b'{'
                            | b'}'
                            | b'['
                            | b']'
                            | b'/'
                    )
                {
                    ix += 1;
                }
                Kind::Literal
            }
        };
        if kind == Kind::Close {
            depth = depth.checked_sub(1)?;
        }
        tokens.push(Token {
            kind,
            start,
            end: ix,
            depth,
        });
        if kind == Kind::Open {
            depth += 1;
        }
    }
    (depth == 0).then_some(tokens)
}

/// The strict JSON equivalent of `text`, without comments or trailing commas.
fn parse_jsonc(text: &str) -> Option<Value> {
    let tokens = tokenize(text)?;
    let mut strict = String::with_capacity(text.len());
    for (ix, token) in tokens.iter().enumerate() {
        let trailing_comma = token.kind == Kind::Comma
            && tokens
                .get(ix + 1)
                .is_some_and(|next| next.kind == Kind::Close);
        if !trailing_comma {
            strict.push_str(&text[token.start..token.end]);
            strict.push(' ');
        }
    }
    serde_json::from_str(&strict).ok()
}

/// The index of the token that closes the container opened at `open`.
fn closing(tokens: &[Token], open: usize) -> Option<usize> {
    let depth = tokens[open].depth;
    tokens[open + 1..]
        .iter()
        .position(|token| token.kind == Kind::Close && token.depth == depth)
        .map(|offset| open + 1 + offset)
}

/// The index of the value token of `key` directly inside the object opened at `open`.
fn member_value(text: &str, tokens: &[Token], open: usize, key: &str) -> Option<usize> {
    let close = closing(tokens, open)?;
    let inner = tokens[open].depth + 1;
    (open + 1..close).find_map(|ix| {
        let token = &tokens[ix];
        let is_key = token.kind == Kind::String
            && token.depth == inner
            && tokens
                .get(ix + 1)
                .is_some_and(|next| next.kind == Kind::Colon);
        let name: Option<String> = serde_json::from_str(&text[token.start..token.end]).ok();
        (is_key && name.as_deref() == Some(key)).then_some(ix + 2)
    })
}

/// The whitespace a line starts with.
fn indent_at(text: &str, position: usize) -> &str {
    let line_start = text[..position].rfind('\n').map_or(0, |ix| ix + 1);
    let line = &text[line_start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// `value` as pretty JSON, with every line after the first indented by `indent`.
fn pretty(value: &Value, indent: &str) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_default()
        .replace('\n', &format!("\n{indent}"))
}

/// Inserts `item` before the closing token at `close` of the container opened at
/// `open`, adding a separating comma when one is needed.
fn insert_before_close(
    text: &str,
    tokens: &[Token],
    open: usize,
    close: usize,
    item: impl Fn(&str) -> String,
) -> String {
    let outer = indent_at(text, tokens[open].start).to_string();
    let first_inner = (open + 1 < close).then(|| tokens[open + 1].start);
    let inner = match first_inner {
        Some(start) if text[tokens[open].end..start].contains('\n') => {
            indent_at(text, start).to_string()
        }
        _ => format!("{outer}  "),
    };
    let previous = &tokens[close - 1];
    let mut edited = String::with_capacity(text.len() + 256);
    if close - 1 == open {
        edited.push_str(&text[..previous.end]);
        edited.push_str(&format!("\n{inner}{}\n{outer}", item(&inner)));
        edited.push_str(&text[tokens[close].start..]);
    } else {
        // Inserting right after the last value keeps the new item ahead of any comment
        // that ends the line, which would otherwise swallow it.
        edited.push_str(&text[..previous.end]);
        if previous.kind != Kind::Comma {
            edited.push(',');
        }
        edited.push_str(&format!("\n{inner}{}", item(&inner)));
        edited.push_str(&text[previous.end..]);
    }
    edited
}

/// Adds `key: value` to the object reached through `path` from the root, creating the
/// objects along the way. `Ok(None)` when `key` is already there.
fn insert_member(
    text: &str,
    path: &[&str],
    key: &str,
    value: &Value,
) -> Result<Option<String>, &'static str> {
    let tokens = tokenize(text).ok_or("it isn’t valid JSON")?;
    tokens
        .first()
        .filter(|token| token.kind == Kind::Open && &text[token.start..token.end] == "{")
        .ok_or("it isn’t a JSON object")?;
    let mut open = 0;
    for (depth, name) in path.iter().enumerate() {
        match member_value(text, &tokens, open, name) {
            Some(ix)
                if tokens[ix].kind == Kind::Open
                    && &text[tokens[ix].start..tokens[ix].end] == "{" =>
            {
                open = ix;
            }
            Some(_) => return Err("a setting Switchfold needs isn’t an object"),
            None => {
                let mut nested = serde_json::Map::new();
                nested.insert(key.to_string(), value.clone());
                let mut built = Value::Object(nested);
                for name in path[depth + 1..].iter().rev() {
                    let mut wrapper = serde_json::Map::new();
                    wrapper.insert(name.to_string(), built);
                    built = Value::Object(wrapper);
                }
                let close = closing(&tokens, open).ok_or("it isn’t valid JSON")?;
                let member = |indent: &str| {
                    format!(
                        "{}: {}",
                        Value::String(name.to_string()),
                        pretty(&built, indent)
                    )
                };
                return Ok(Some(insert_before_close(
                    text, &tokens, open, close, member,
                )));
            }
        }
    }
    if member_value(text, &tokens, open, key).is_some() {
        return Ok(None);
    }
    let close = closing(&tokens, open).ok_or("it isn’t valid JSON")?;
    let member = |indent: &str| format!("{}: {}", Value::String(key.into()), pretty(value, indent));
    Ok(Some(insert_before_close(
        text, &tokens, open, close, member,
    )))
}

/// Appends `item` to the array at the root of `text`.
fn append_item(text: &str, item: &Value) -> Result<String, &'static str> {
    let tokens = tokenize(text).ok_or("it isn’t valid JSON")?;
    let open = tokens
        .iter()
        .position(|token| token.kind == Kind::Open && token.depth == 0)
        .filter(|&ix| &text[tokens[ix].start..tokens[ix].end] == "[")
        .ok_or("it isn’t a JSON array")?;
    let close = closing(&tokens, open).ok_or("it isn’t valid JSON")?;
    Ok(insert_before_close(text, &tokens, open, close, |indent| {
        pretty(item, indent)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: &str = r#"// Zed settings
{
  "theme": "One Dark", // keep me
  "context_servers": {
    "MDN": {
      "enabled": true,
      "url": "https://mcp.mdn.mozilla.net/",
    },
  },
  /* block */
  "ui_font_size": 16
}
"#;

    fn entry() -> Value {
        json!({ "command": "/u/.switchfold/bin/switchfold", "args": ["--zed-mcp"] })
    }

    #[test]
    fn adds_a_server_next_to_existing_ones_and_keeps_comments() {
        let edited = insert_member(SETTINGS, &["context_servers"], "switchfold", &entry())
            .unwrap()
            .unwrap();
        assert!(edited.contains("// keep me") && edited.contains("/* block */"));
        let parsed = parse_jsonc(&edited).unwrap();
        assert_eq!(
            parsed["context_servers"]["switchfold"]["args"][0],
            "--zed-mcp"
        );
        assert_eq!(parsed["context_servers"]["MDN"]["enabled"], true);
        assert_eq!(parsed["ui_font_size"], 16);
        assert!(edited.contains("    },\n    \"switchfold\": {\n      \""));
        assert_eq!(
            insert_member(&edited, &["context_servers"], "switchfold", &entry()).unwrap(),
            None
        );
    }

    #[test]
    fn creates_the_section_when_missing() {
        let text = "{\n  \"theme\": \"One Dark\" // no trailing comma\n}\n";
        let edited = insert_member(text, &["context_servers"], "switchfold", &entry())
            .unwrap()
            .unwrap();
        let parsed = parse_jsonc(&edited).unwrap();
        assert_eq!(parsed["theme"], "One Dark");
        assert!(parsed["context_servers"]["switchfold"].is_object());
        let empty = insert_member("{}", &["context_servers"], "switchfold", &entry())
            .unwrap()
            .unwrap();
        assert!(parse_jsonc(&empty).unwrap()["context_servers"]["switchfold"].is_object());
    }

    #[test]
    fn appends_a_task_after_commented_examples() {
        let text = "// Tasks\n[\n  {\n    \"label\": \"Example\",\n    //\"args\": [],\n    \"command\": \"echo\"\n  }\n]\n";
        let edited = append_item(text, &json!({ "label": COMMENT_TASK_LABEL })).unwrap();
        let parsed = parse_jsonc(&edited).unwrap();
        assert_eq!(parsed[0]["label"], "Example");
        assert_eq!(parsed[1]["label"], COMMENT_TASK_LABEL);
    }

    #[test]
    fn refuses_files_of_the_wrong_shape() {
        assert!(insert_member("[1]", &["context_servers"], "s", &entry()).is_err());
        assert!(
            insert_member(
                "{\"context_servers\": 3}",
                &["context_servers"],
                "s",
                &entry()
            )
            .is_err()
        );
        assert!(insert_member("{\"a\": \"unterminated}", &["x"], "s", &entry()).is_err());
        assert!(append_item("{}", &entry()).is_err());
    }

    #[test]
    fn installs_into_files_with_a_backup() {
        let home = std::env::temp_dir().join(format!("zed-bridge-config-{}", std::process::id()));
        fs::remove_dir_all(&home).ok();
        let settings = zed_config_dir(&home).join("settings.json");
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(&settings, SETTINGS).unwrap();
        let binary = home.join("Switchfold Review.app/Contents/MacOS/switchfold");
        assert_eq!(mcp_status(&home), McpStatus::Missing);
        assert_eq!(
            install_mcp_server(&home, &binary).unwrap(),
            Installed::Added
        );
        assert_eq!(mcp_status(&home), McpStatus::Installed);
        assert_eq!(
            install_mcp_server(&home, &binary).unwrap(),
            Installed::AlreadyThere
        );
        assert_eq!(
            fs::read_to_string(settings.with_extension("json.before-switchfold")).unwrap(),
            SETTINGS
        );
        assert_eq!(fs::read_link(shim_path(&home)).unwrap(), binary);
        assert_eq!(
            install_comment_task(&home, &binary).unwrap(),
            Installed::Added
        );
        assert_eq!(
            install_comment_task(&home, &binary).unwrap(),
            Installed::AlreadyThere
        );
        fs::remove_dir_all(&home).ok();
    }
}

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::{ApiError, GhSession, gh_session};

/// The one-time code and page of a GitHub device sign-in started through `gh`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceCode {
    pub code: String,
    pub url: String,
}

/// A running `gh auth login --web`. Finish it with [`GhLogin::wait`].
pub struct GhLogin {
    child: Child,
    stdin: Option<ChildStdin>,
}

/// How long `gh` gets to print its one-time code.
const CODE_TIMEOUT: Duration = Duration::from_secs(20);

/// Starts GitHub's device sign-in for `host` through the GitHub CLI and returns the code
/// to show the user. The CLI stores the resulting token in its own keyring entry.
pub fn start_gh_login(host: &str) -> Result<(DeviceCode, GhLogin), ApiError> {
    let mut child = tool_path::command("gh")
        .args([
            "auth",
            "login",
            "--hostname",
            host,
            "--web",
            "--git-protocol",
            "https",
            "--skip-ssh-key",
        ])
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ApiError::new(format!("Couldn’t start the GitHub CLI: {error}")))?;

    let (lines, received) = mpsc::channel::<String>();
    for stream in [
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let lines = lines.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
    }

    let mut code = None;
    let mut url = format!("https://{host}/login/device");
    while code.is_none() {
        let Ok(line) = received.recv_timeout(CODE_TIMEOUT) else {
            child.kill().ok();
            return Err(ApiError::new("The GitHub CLI didn’t print a sign-in code."));
        };
        if let Some(found) = parse_code(&line) {
            code = Some(found);
        }
        if let Some(found) = parse_url(&line) {
            url = found;
        }
    }
    let mut stdin = child.stdin.take();
    // `gh` waits for Enter before opening the browser when it has a terminal; the
    // newline is harmless otherwise.
    if let Some(stdin) = stdin.as_mut() {
        stdin.write_all(b"\n").ok();
    }
    Ok((
        DeviceCode {
            code: code.unwrap_or_default(),
            url,
        },
        GhLogin { child, stdin },
    ))
}

impl GhLogin {
    /// Waits for the user to approve the code, then reads the signed-in account.
    pub fn wait(mut self) -> Result<GhSession, ApiError> {
        drop(self.stdin.take());
        let status = self
            .child
            .wait()
            .map_err(|error| ApiError::new(format!("The GitHub CLI stopped: {error}")))?;
        if !status.success() {
            return Err(ApiError::new("GitHub sign-in didn’t finish."));
        }
        gh_session().ok_or_else(|| ApiError::new("The GitHub CLI isn’t signed in."))
    }
}

fn parse_code(line: &str) -> Option<String> {
    let rest = line.split_once("one-time code:")?.1;
    let code = rest.split_whitespace().next()?;
    (code.len() >= 8 && code.contains('-')).then(|| code.to_string())
}

fn parse_url(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|word| word.starts_with("https://") && word.contains("/login/device"))
        .map(|word| word.trim_end_matches(['.', ',']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_code_and_page_from_gh_output() {
        assert_eq!(
            parse_code("! First copy your one-time code: ABCD-1234").as_deref(),
            Some("ABCD-1234")
        );
        assert_eq!(
            parse_url(
                "Open this URL to continue in your web browser: https://github.com/login/device"
            )
            .as_deref(),
            Some("https://github.com/login/device")
        );
        assert_eq!(parse_code("Press Enter to open github.com"), None);
    }
}

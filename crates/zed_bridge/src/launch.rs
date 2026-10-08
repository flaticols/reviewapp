use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::percent;

/// Why Zed couldn't be started.
#[derive(Debug)]
pub struct LaunchError {
    program: String,
    error: io::Error,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Couldn’t start {}: {}", self.program, self.error)
    }
}

impl std::error::Error for LaunchError {}

fn spawn(program: &str, args: &[String]) -> Result<(), LaunchError> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| LaunchError {
            program: program.to_string(),
            error,
        })
}

/// Opens `root` as a Zed project and, when given, `file` (relative to `root`) at `line`
/// in it. Naming the root keeps the file inside its project, with language servers and
/// project search, rather than in a window of its own.
pub fn open_project(
    zed_command: &str,
    root: &Path,
    file: Option<(&str, Option<u32>)>,
) -> Result<(), LaunchError> {
    let mut args = vec![root.display().to_string()];
    if let Some((file, line)) = file {
        let path = root.join(file).display().to_string();
        args.push(match line {
            Some(line) => format!("{path}:{line}"),
            None => path,
        });
    }
    spawn(zed_command, &args)
}

/// Shows every file that differs between `base_dir` and `head_dir` in one Zed
/// multi-file diff.
pub fn open_diff(zed_command: &str, base_dir: &Path, head_dir: &Path) -> Result<(), LaunchError> {
    spawn(
        zed_command,
        &[
            "--diff".into(),
            base_dir.display().to_string(),
            head_dir.display().to_string(),
        ],
    )
}

/// Opens `url` with the app registered for its scheme.
pub fn open_url(url: &str) -> Result<(), LaunchError> {
    spawn("/usr/bin/open", &[url.to_string()])
}

/// The `zed://` link to Zed's commit view of `sha` in the repository at `repo`.
pub fn commit_url(repo: &Path, sha: &str) -> String {
    format!(
        "zed://git/commit/{}?repo={}",
        percent::encode(sha, false),
        percent::encode(&repo.to_string_lossy(), false)
    )
}

/// The `zed://` link that opens a new thread in Zed's agent panel with `prompt` filled
/// in. Zed never sends it on its own: the user reads it and presses Send.
pub fn agent_prompt_url(prompt: &str) -> String {
    format!("zed://agent?prompt={}", percent::encode(prompt, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_encode_their_arguments() {
        assert_eq!(
            commit_url(Path::new("/Users/me/My Repo"), "abc123"),
            "zed://git/commit/abc123?repo=%2FUsers%2Fme%2FMy%20Repo"
        );
        assert_eq!(
            agent_prompt_url("Fix a.rs:3 & b"),
            "zed://agent?prompt=Fix%20a.rs%3A3%20%26%20b"
        );
    }
}

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::percent;

/// Why Zed didn't open what was asked.
#[derive(Debug)]
pub enum LaunchError {
    /// The command couldn't run at all.
    Start { program: String, error: io::Error },
    /// It ran and reported a failure.
    Failed { program: String, detail: String },
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start { program, error } => write!(f, "Couldn’t start {program}: {error}"),
            Self::Failed { program, detail } => write!(f, "{program} failed: {detail}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Runs `program` in `dir` to completion and appends what happened to
/// `~/.switchfold/logs/zed.log`. Zed's CLI returns once Zed has the request, so this
/// blocks briefly; call it off the UI thread.
fn run(program: &str, args: &[String], dir: &Path) -> Result<(), LaunchError> {
    // Zed's CLI makes its working directory part of the window it opens. An app started
    // from the Dock runs in `/`, which would make the whole disk the project.
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output();
    let outcome = match &output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(LaunchError::Failed {
                program: program.to_string(),
                detail: if stderr.is_empty() {
                    output.status.to_string()
                } else {
                    stderr
                },
            })
        }
        Err(error) => Err(LaunchError::Start {
            program: program.to_string(),
            error: io::Error::new(error.kind(), error.to_string()),
        }),
    };
    log(program, args, &output, &outcome);
    outcome
}

fn log(
    program: &str,
    args: &[String],
    output: &io::Result<std::process::Output>,
    outcome: &Result<(), LaunchError>,
) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = Path::new(&home).join(".switchfold/logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let streams = match output {
        Ok(output) => format!(
            "\n  stdout: {}\n  stderr: {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(_) => String::new(),
    };
    let line = format!(
        "[{}] {program} {args:?} -> {}{streams}\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
        match outcome {
            Ok(()) => "ok".to_string(),
            Err(error) => error.to_string(),
        }
    );
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("zed.log"))
    {
        file.write_all(line.as_bytes()).ok();
    }
}

/// Opens `root` as a Zed project and, when given, `file` (relative to `root`) at `line`
/// in it. Naming the root keeps the file inside its project, with language servers and
/// project search, rather than in a window of its own. Blocks until Zed's CLI returns.
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
    run(zed_command, &args, root)
}

/// Shows every file that differs between `base_dir` and `head_dir` in one Zed
/// multi-file diff. Blocks until Zed's CLI returns.
pub fn open_diff(zed_command: &str, base_dir: &Path, head_dir: &Path) -> Result<(), LaunchError> {
    run(
        zed_command,
        &[
            "--diff".into(),
            base_dir.display().to_string(),
            head_dir.display().to_string(),
        ],
        base_dir.parent().unwrap_or(base_dir),
    )
}

/// Opens `url` with the app registered for its scheme. Blocks until `open` returns.
pub fn open_url(url: &str) -> Result<(), LaunchError> {
    run("/usr/bin/open", &[url.to_string()], Path::new("/"))
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

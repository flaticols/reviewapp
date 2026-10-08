//! Opening files in Zed through its command-line launcher.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

/// Opens `path` in Zed, at `line` when given, with the launcher at `zed_command`.
/// Returns once the launcher has been started.
pub fn open_in_zed(zed_command: &str, path: &Path, line: Option<u32>) -> io::Result<()> {
    let target = match line {
        Some(line) => format!("{}:{line}", path.display()),
        None => path.display().to_string(),
    };
    Command::new(zed_command)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// The version `zed --version` reports, such as `0.205.3`, or `None` when the launcher
/// is missing. Runs a process; call it off the UI thread.
pub fn zed_version(zed_command: &str) -> Option<String> {
    let output = Command::new(zed_command).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    version_in(&String::from_utf8_lossy(&output.stdout))
}

/// The first whitespace-separated word that starts with a digit and contains a dot.
pub fn version_in(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|word| word.trim_start_matches('v'))
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()) && word.contains('.'))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_version_word() {
        assert_eq!(version_in("Zed 0.205.3 abc123"), Some("0.205.3".into()));
        assert_eq!(version_in("2.1.292 (Claude Code)"), Some("2.1.292".into()));
        assert_eq!(version_in("gemini v0.9.0"), Some("0.9.0".into()));
        assert_eq!(version_in("no version"), None);
    }
}

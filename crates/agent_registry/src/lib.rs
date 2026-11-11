//! Finding the agents Switchfold can launch over ACP and the versions they report.

use std::path::{Path, PathBuf};

use std::process::Command;

use settings_model::AgentConfig;

/// What is installed for one configured agent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentAvailability {
    /// The agent's own binary was found.
    pub binary_found: bool,
    /// The version the agent binary reports.
    pub version: Option<String>,
    /// The ACP launch command resolves on `PATH`.
    pub launcher: Option<PathBuf>,
}

impl AgentAvailability {
    /// Whether Switchfold can start the agent.
    pub fn is_launchable(&self) -> bool {
        self.launcher.is_some()
    }
}

/// Inspects the binary and launcher of `agent`. Runs processes; call it off the UI thread.
pub fn detect(agent: &AgentConfig) -> AgentAvailability {
    let binary_found = agent.binary.as_deref().is_some_and(Path::is_file);
    let version = agent
        .binary
        .as_deref()
        .filter(|binary| binary.is_file())
        .and_then(|binary| {
            let output = Command::new(binary).arg("--version").output().ok()?;
            editor_launch::version_in(&String::from_utf8_lossy(&output.stdout))
        });
    AgentAvailability {
        binary_found,
        version,
        launcher: find_on_path(&agent.command),
    }
}

pub use tool_path::{find_on_path, search_dirs};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_commands_resolve_only_when_present() {
        assert!(find_on_path("/bin/sh").is_some());
        assert!(find_on_path("/definitely/not/here").is_none());
        assert!(find_on_path("sh").is_some());
    }
}

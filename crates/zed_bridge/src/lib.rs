//! Switchfold and Zed, both ways.
//!
//! Into Zed: open a pull request's checkout as a project at a line, show the whole pull
//! request as one multi-file diff, open a commit in Zed's commit view, and hand a
//! comment to Zed's agent as a prefilled prompt.
//!
//! Back from Zed: `dev.switchfold.review://` links that a Zed task fires to comment on
//! the line under the cursor, and an MCP server that lets Zed's agent read threads and
//! findings and add pending comments. Neither ever submits a review.

mod config;
mod launch;
mod link;
mod locate;
pub mod mcp;
mod percent;

pub use config::{
    COMMENT_TASK_LABEL, ConfigError, Installed, McpStatus, comment_task_installed,
    comment_task_keybinding, install_comment_task, install_mcp_server, mcp_status, shim_path,
    zed_config_dir,
};
pub use launch::{LaunchError, agent_prompt_url, commit_url, open_diff, open_project, open_url};
pub use link::{SCHEME, SwitchfoldLink};
pub use locate::{Located, locate};

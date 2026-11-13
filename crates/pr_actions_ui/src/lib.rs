//! What you can do with a pull request or one of its files, as context menus shared by
//! the pull request list, the title bar, the file tree and the diff headers: open it on
//! GitHub or in Zed, check it out in place or in a new worktree, approve, close or reopen, request reviews, and copy
//! links, permalinks and a `/review` command for agents.

mod file_menu;
mod local;
mod new_worktree;
mod pull_request_menu;
mod request_review;
mod zed;

pub use file_menu::{FileTarget, file_menu};
pub use local::{check_out, reveal_repository};
pub use new_worktree::{check_out_in_new_worktree, open_new_worktree};
pub use pull_request_menu::{PullRequestTarget, pull_request_menu};
pub use request_review::open_request_review;
pub use zed::{
    ask_zed_agent, detail_when_loaded, diff_in_zed, open_commit_in_zed, open_in_zed,
    open_repository_in_zed,
};

//! Access to GitHub: the pull request source the app reads from, the GitHub CLI session
//! used for sign-in, and personal access token checks. Blocking calls; the UI runs them
//! on a background executor.

mod gh_cli;
mod gh_login;
mod gh_source;
mod source;
mod time;
mod token;

pub use gh_cli::{GhSession, gh_session, gh_user_name};
pub use gh_login::{DeviceCode, GhLogin, start_gh_login};
pub use gh_source::GhSource;
pub use source::{
    ApiError, DraftComment, FixtureSource, PullRequestSource, Reply, ReviewSubmission,
};
pub use token::{REQUIRED_SCOPES, TokenCheck, check_token, is_token_shaped};

//! Switchfold's Agent Client Protocol client: runs one review session against any ACP
//! agent, streams what the agent does, routes permission requests to the user, and
//! collects findings the agent reports through Switchfold's findings MCP server.
//!
//! Runtime-agnostic: [`start_review`] returns a future to run on any executor plus the
//! channel of [`AgentEvent`]s.

pub mod findings;
mod options;
mod prompt;
mod session;

pub use findings::ReportedFinding;
pub use options::{AgentSetting, SessionChoices, SettingChoice, SettingKind, probe_settings};
pub use prompt::{
    PromptInput, QuestionInput, ReviewDepth, ReviewScope, question_prompt, review_prompt,
    unified_patch,
};
pub use session::{
    AgentEvent, Decision, PermissionChoice, PermissionPrompt, PlanStatus, PlanStep, ReviewRequest,
    RunControl, RunOutcome, ToolActivity, ToolKind, ToolStatus, readable_error, start_review,
};

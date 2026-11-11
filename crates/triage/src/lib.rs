//! What Switchfold asks a triage provider and what it gets back, independent of the
//! provider: how severe and actionable an agent finding is, what kind of review
//! thread a conversation is, and how risky a pull request looks.
//!
//! A provider implements [`TriageProvider`]. The rubrics here, each level's meaning,
//! are shared so every provider is asked the same thing.

mod inputs;
mod rubric;

pub use inputs::{Comment, FindingInput, PullRequestInput, ThreadInput};
pub use rubric::{
    ChangeKind, FindingCategory, FindingTriage, PullRequestRisk, Risk, Severity, ThreadKind,
    ThreadLabel,
};

use std::fmt;

/// The yes/no questions every provider is asked; the rubric questions are associated
/// constants on their types, such as [`Severity::QUESTION`].
pub mod questions {
    /// Whether an agent finding is worth acting on.
    pub const ACTIONABLE: &str = crate::rubric::ACTIONABLE_QUESTION;
    /// Whether a review thread was already dealt with.
    pub const ADDRESSED: &str = crate::rubric::ADDRESSED_QUESTION;
}

/// Why a provider couldn't answer, in words for the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriageError(pub String);

impl fmt::Display for TriageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TriageError {}

/// A service that answers triage questions. Calls block; the app makes them off the UI
/// thread, several at a time.
pub trait TriageProvider: Send + Sync {
    /// A stable name, such as `typesafe`, used to keep results apart in the cache.
    fn id(&self) -> &str;

    /// The model or version answering, which also keys the cache, so switching models
    /// asks again.
    fn model(&self) -> &str;

    /// Rates an agent finding.
    fn triage_finding(&self, finding: &FindingInput) -> Result<FindingTriage, TriageError>;

    /// Labels a review thread.
    fn label_thread(&self, thread: &ThreadInput) -> Result<ThreadLabel, TriageError>;

    /// Rates a pull request's kind and risk.
    fn rate_pull_request(
        &self,
        pull_request: &PullRequestInput,
    ) -> Result<PullRequestRisk, TriageError>;
}

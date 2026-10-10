use std::sync::atomic::{AtomicUsize, Ordering};

use triage::{
    ChangeKind, FindingCategory, FindingInput, FindingTriage, PullRequestInput, PullRequestRisk,
    Risk, Severity, ThreadInput, ThreadKind, ThreadLabel, TriageError, TriageProvider,
};

/// A triage provider that answers from simple text rules, for screenshots and tests.
/// It counts calls, and can be made to fail.
#[derive(Default)]
pub struct SampleTriage {
    calls: AtomicUsize,
    fail: bool,
}

impl SampleTriage {
    /// A provider that fails every call, as with a refused key.
    pub fn failing() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            fail: true,
        }
    }

    /// How many questions it was asked.
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn call(&self) -> Result<(), TriageError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(TriageError("TypeSafe.ai refused the API key.".into()))
        } else {
            Ok(())
        }
    }
}

fn mentions(text: &str, words: &[&str]) -> bool {
    let lower = text.to_lowercase();
    words.iter().any(|word| lower.contains(word))
}

impl TriageProvider for SampleTriage {
    fn id(&self) -> &str {
        "sample"
    }

    fn model(&self) -> &str {
        "rules-1"
    }

    fn triage_finding(&self, finding: &FindingInput) -> Result<FindingTriage, TriageError> {
        self.call()?;
        let text = &finding.finding;
        let (severity, category) = if mentions(text, &["panic", "crash", "data loss", "injection"])
        {
            (Severity::Critical, FindingCategory::Bug)
        } else if mentions(text, &["stale", "wrong", "race", "never", "bug"]) {
            (Severity::Major, FindingCategory::Bug)
        } else if mentions(text, &["rename", "naming", "typo"]) {
            (Severity::Nit, FindingCategory::Style)
        } else {
            (Severity::Moderate, FindingCategory::Maintainability)
        };
        let actionable = if severity == Severity::Nit { 0.2 } else { 0.85 };
        Ok(FindingTriage::new(
            severity,
            0.8,
            actionable,
            Some(category),
        ))
    }

    fn label_thread(&self, thread: &ThreadInput) -> Result<ThreadLabel, TriageError> {
        self.call()?;
        let first = thread
            .comments
            .first()
            .map(|c| c.text.as_str())
            .unwrap_or_default();
        let kind = if first.trim_end().ends_with('?') {
            ThreadKind::Question
        } else if mentions(first, &["must", "breaks", "wrong", "before merging"]) {
            ThreadKind::Blocking
        } else if mentions(first, &["nit", "typo"]) {
            ThreadKind::Nit
        } else {
            ThreadKind::Suggestion
        };
        let addressed = if thread.comments.len() > 2 { 0.8 } else { 0.1 };
        Ok(ThreadLabel::new(kind, 0.8, addressed))
    }

    fn rate_pull_request(
        &self,
        pull_request: &PullRequestInput,
    ) -> Result<PullRequestRisk, TriageError> {
        self.call()?;
        let title = &pull_request.title;
        let kind = if mentions(title, &["fix", "bug"]) {
            ChangeKind::Fix
        } else if mentions(title, &["bump", "deps", "dependency"]) {
            ChangeKind::Dependencies
        } else if mentions(title, &["refactor", "rename"]) {
            ChangeKind::Refactor
        } else if mentions(title, &["docs", "readme"]) {
            ChangeKind::Docs
        } else {
            ChangeKind::Feature
        };
        let risk = if mentions(title, &["auth", "migration", "payment", "token"]) {
            Risk::Critical
        } else if mentions(title, &["render", "thread", "scroll"]) {
            Risk::High
        } else if kind == ChangeKind::Docs {
            Risk::Trivial
        } else if kind == ChangeKind::Feature {
            Risk::Medium
        } else {
            Risk::Low
        };
        Ok(PullRequestRisk::new(kind, 0.8, risk, 0.7))
    }
}

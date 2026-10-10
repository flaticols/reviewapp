use serde::{Deserialize, Serialize};

/// Declares an ordered enum whose variants each carry a stable key, a label for the
/// interface, and the rubric text a provider is shown.
macro_rules! rubric {
    (
        $(#[$meta:meta])*
        $name:ident { $($variant:ident => $key:literal, $label:literal, $rubric:literal;)* }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant,)*
        }

        impl $name {
            /// Every level, in order.
            pub const ALL: &'static [$name] = &[$(Self::$variant,)*];

            /// The stable key providers answer with.
            pub fn key(self) -> &'static str {
                match self { $(Self::$variant => $key,)* }
            }

            /// The label the interface shows.
            pub fn label(self) -> &'static str {
                match self { $(Self::$variant => $label,)* }
            }

            /// What this level means, as providers are told.
            pub fn rubric(self) -> &'static str {
                match self { $(Self::$variant => $rubric,)* }
            }

            /// The level for `key`.
            pub fn from_key(key: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|level| level.key() == key)
            }

            /// The level nearest a probability-weighted score from 0 to the number of
            /// levels minus one.
            pub fn from_score(score: f32) -> Self {
                let last = Self::ALL.len() - 1;
                let ix = if score.is_finite() { score.round().clamp(0., last as f32) as usize } else { 0 };
                Self::ALL[ix]
            }
        }
    };
}

rubric! {
    /// How bad the problem an agent finding describes is.
    Severity {
        Nit => "nit", "Nit", "Style or wording only; nothing behaves differently.";
        Minor => "minor", "Minor", "A small improvement with little risk if left as is.";
        Moderate => "moderate", "Moderate", "A real defect in an edge case, or a maintainability problem that will cost later.";
        Major => "major", "Major", "Likely wrong behavior, a bug or bad data in normal use.";
        Critical => "critical", "Critical", "A security hole, data loss, a crash or an outage.";
    }
}

rubric! {
    /// What area an agent finding is about.
    FindingCategory {
        Bug => "bug", "Bug", "Incorrect logic or behavior.";
        Security => "security", "Security", "Vulnerabilities, secrets, injection, unsafe input handling or access control.";
        Performance => "performance", "Performance", "Needless work, slow paths, excess allocation or I/O.";
        ErrorHandling => "error_handling", "Error handling", "Errors ignored, swallowed, or reported unclearly.";
        Tests => "tests", "Tests", "Missing, wrong or flaky tests.";
        Maintainability => "maintainability", "Maintainability", "Duplication, unclear structure, or code that is hard to change.";
        Style => "style", "Style", "Naming, formatting, comments or conventions.";
    }
}

rubric! {
    /// What a review thread asks of the author.
    ThreadKind {
        Blocking => "blocking", "Blocking", "Must be fixed before merging: a bug, a broken contract, or a change the reviewer insists on.";
        Suggestion => "suggestion", "Suggestion", "An improvement the author may take or leave.";
        Nit => "nit", "Nit", "Cosmetic: naming, formatting, typos.";
        Question => "question", "Question", "Asks for an explanation or clarification.";
        Praise => "praise", "Praise", "Approval or a positive remark; nothing to do.";
    }
}

rubric! {
    /// What kind of change a pull request makes.
    ChangeKind {
        Feature => "feature", "Feature", "Adds new behavior or capability.";
        Fix => "fix", "Fix", "Corrects a bug or wrong behavior.";
        Refactor => "refactor", "Refactor", "Restructures code without changing what it does.";
        Dependencies => "dependencies", "Deps", "Updates or adds dependencies, lockfiles or toolchains.";
        Docs => "docs", "Docs", "Documentation, comments or READMEs only.";
        Tests => "tests", "Tests", "Adds or changes tests only.";
        Chore => "chore", "Chore", "Build, CI, configuration or housekeeping.";
    }
}

rubric! {
    /// How likely a pull request is to break something.
    Risk {
        Trivial => "trivial", "Trivial", "Docs, comments, config values or tests only.";
        Low => "low", "Low risk", "A small change with a clear, contained scope.";
        Medium => "medium", "Medium risk", "Touches shared code or behavior users see.";
        High => "high", "High risk", "Changes core logic, data models, migrations, authentication or many files.";
        Critical => "critical", "Critical risk", "Security, payments, data migrations or wide refactors that can break production.";
    }
}

impl Severity {
    /// The question every provider is asked about a finding's severity.
    pub const QUESTION: &'static str =
        "How severe is the problem the `finding` describes in the `code` it points at?";
}

impl FindingCategory {
    /// The question every provider is asked about a finding's area.
    pub const QUESTION: &'static str = "What area of the code is the `finding` about?";
}

/// The question every provider is asked about whether a finding is worth acting on.
pub(crate) const ACTIONABLE_QUESTION: &str =
    "The `finding` points to a concrete problem in the `code` shown that the author should change.";

impl ThreadKind {
    /// The question every provider is asked about a thread.
    pub const QUESTION: &'static str =
        "What does this review thread ask of the pull request's author?";
}

/// The question every provider is asked about whether a thread was dealt with.
pub(crate) const ADDRESSED_QUESTION: &str =
    "The later comments show the point was answered, fixed or agreed to be left as is.";

impl ChangeKind {
    /// The question every provider is asked about a pull request's kind.
    pub const QUESTION: &'static str = "What kind of change does this pull request make?";
}

impl Risk {
    /// The question every provider is asked about a pull request's risk.
    pub const QUESTION: &'static str =
        "How likely is merging this pull request to break something in production?";
}

/// An agent finding, rated.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FindingTriage {
    pub severity: Severity,
    pub severity_confidence: f32,
    /// The probability the finding is worth acting on.
    pub actionable: f32,
    pub category: Option<FindingCategory>,
}

impl FindingTriage {
    /// A rating; `actionable` is a probability.
    pub fn new(
        severity: Severity,
        severity_confidence: f32,
        actionable: f32,
        category: Option<FindingCategory>,
    ) -> Self {
        Self {
            severity,
            severity_confidence,
            actionable,
            category,
        }
    }

    /// Whether the finding is probably not worth the reviewer's time.
    pub fn is_likely_noise(&self) -> bool {
        self.actionable < 0.35
    }
}

/// A review thread, labeled.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ThreadLabel {
    pub kind: ThreadKind,
    pub confidence: f32,
    /// The probability the thread was already dealt with.
    pub addressed: f32,
}

impl ThreadLabel {
    /// A label; `addressed` is a probability.
    pub fn new(kind: ThreadKind, confidence: f32, addressed: f32) -> Self {
        Self {
            kind,
            confidence,
            addressed,
        }
    }

    /// Whether the thread looks dealt with.
    pub fn looks_addressed(&self) -> bool {
        self.addressed >= 0.7
    }
}

/// A pull request, rated.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PullRequestRisk {
    pub kind: ChangeKind,
    pub kind_confidence: f32,
    pub risk: Risk,
    pub risk_confidence: f32,
}

impl PullRequestRisk {
    /// A rating.
    pub fn new(kind: ChangeKind, kind_confidence: f32, risk: Risk, risk_confidence: f32) -> Self {
        Self {
            kind,
            kind_confidence,
            risk,
            risk_confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_round_to_the_nearest_level_and_stay_in_range() {
        assert_eq!(Severity::from_score(0.2), Severity::Nit);
        assert_eq!(Severity::from_score(2.6), Severity::Major);
        assert_eq!(Severity::from_score(9.0), Severity::Critical);
        assert_eq!(Severity::from_score(f32::NAN), Severity::Nit);
        assert_eq!(ThreadKind::from_key("blocking"), Some(ThreadKind::Blocking));
        assert_eq!(ChangeKind::Dependencies.label(), "Deps");
    }
}

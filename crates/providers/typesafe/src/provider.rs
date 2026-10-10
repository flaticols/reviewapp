use std::collections::BTreeMap;

use serde::Serialize;
use triage::{
    ChangeKind, FindingCategory, FindingInput, FindingTriage, PullRequestInput, PullRequestRisk,
    Risk, Severity, ThreadInput, ThreadKind, ThreadLabel, TriageError, TriageProvider,
};

use crate::{Answer, Client, Question};

/// [`TriageProvider`] over TypeSafe.ai: each call is one System One request that asks
/// every question about the item at once.
#[derive(Clone, Debug)]
pub struct TypeSafeProvider {
    client: Client,
}

impl TypeSafeProvider {
    /// A provider asking through `client`.
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    fn ask(
        &self,
        state: &impl Serialize,
        questions: BTreeMap<String, Question>,
    ) -> Result<BTreeMap<String, Answer>, TriageError> {
        let state = serde_json::to_value(state).map_err(|error| TriageError(error.to_string()))?;
        self.client
            .system_one(&state, &questions)
            .map(|(_, answers)| answers)
            .map_err(|error| TriageError(error.to_string()))
    }
}

/// A Score over every level of a rubric.
fn score<T: Copy>(question: &str, levels: &[T], rubric: impl Fn(T) -> &'static str) -> Question {
    let levels: Vec<&str> = levels.iter().map(|level| rubric(*level)).collect();
    Question::score(question, &levels)
}

/// A Choice over every option of a rubric.
fn choice<T: Copy>(
    question: &str,
    options: &[T],
    key: impl Fn(T) -> &'static str,
    rubric: impl Fn(T) -> &'static str,
) -> Question {
    let options: Vec<(&str, &str)> = options
        .iter()
        .map(|option| (key(*option), rubric(*option)))
        .collect();
    Question::choice(question, &options)
}

fn missing(id: &str) -> TriageError {
    TriageError(format!("TypeSafe.ai didn’t answer `{id}`."))
}

impl TriageProvider for TypeSafeProvider {
    fn id(&self) -> &str {
        "typesafe"
    }

    fn model(&self) -> &str {
        self.client.model()
    }

    fn triage_finding(&self, finding: &FindingInput) -> Result<FindingTriage, TriageError> {
        let questions = BTreeMap::from([
            (
                "severity".to_string(),
                score(Severity::QUESTION, Severity::ALL, Severity::rubric),
            ),
            (
                "actionable".to_string(),
                Question::noul(
                    triage::questions::ACTIONABLE,
                    "A real, specific problem worth changing the code for.",
                    "Vague, wrong about the code, already handled, or a matter of taste.",
                ),
            ),
            (
                "category".to_string(),
                choice(
                    FindingCategory::QUESTION,
                    FindingCategory::ALL,
                    FindingCategory::key,
                    FindingCategory::rubric,
                ),
            ),
        ]);
        let answers = self.ask(finding, questions)?;
        let (severity, confidence) = answers
            .get("severity")
            .and_then(Answer::scored)
            .ok_or_else(|| missing("severity"))?;
        let actionable = answers
            .get("actionable")
            .and_then(Answer::yes)
            .ok_or_else(|| missing("actionable"))?;
        let category = answers
            .get("category")
            .and_then(Answer::chosen)
            .and_then(|(key, _)| FindingCategory::from_key(key));
        Ok(FindingTriage::new(
            Severity::from_score(severity),
            confidence,
            actionable,
            category,
        ))
    }

    fn label_thread(&self, thread: &ThreadInput) -> Result<ThreadLabel, TriageError> {
        let questions = BTreeMap::from([
            (
                "kind".to_string(),
                choice(
                    ThreadKind::QUESTION,
                    ThreadKind::ALL,
                    ThreadKind::key,
                    ThreadKind::rubric,
                ),
            ),
            (
                "addressed".to_string(),
                Question::noul(
                    triage::questions::ADDRESSED,
                    "The author replied with a fix or an answer the reviewer accepted.",
                    "The point is still open or unanswered.",
                ),
            ),
        ]);
        let answers = self.ask(thread, questions)?;
        let (kind, confidence) = answers
            .get("kind")
            .and_then(Answer::chosen)
            .and_then(|(key, confidence)| Some((ThreadKind::from_key(key)?, confidence)))
            .ok_or_else(|| missing("kind"))?;
        let addressed = answers
            .get("addressed")
            .and_then(Answer::yes)
            .ok_or_else(|| missing("addressed"))?;
        Ok(ThreadLabel::new(kind, confidence, addressed))
    }

    fn rate_pull_request(
        &self,
        pull_request: &PullRequestInput,
    ) -> Result<PullRequestRisk, TriageError> {
        let questions = BTreeMap::from([
            (
                "kind".to_string(),
                choice(
                    ChangeKind::QUESTION,
                    ChangeKind::ALL,
                    ChangeKind::key,
                    ChangeKind::rubric,
                ),
            ),
            (
                "risk".to_string(),
                score(Risk::QUESTION, Risk::ALL, Risk::rubric),
            ),
        ]);
        let answers = self.ask(pull_request, questions)?;
        let (kind, kind_confidence) = answers
            .get("kind")
            .and_then(Answer::chosen)
            .and_then(|(key, confidence)| Some((ChangeKind::from_key(key)?, confidence)))
            .ok_or_else(|| missing("kind"))?;
        let (risk, risk_confidence) = answers
            .get("risk")
            .and_then(Answer::scored)
            .ok_or_else(|| missing("risk"))?;
        Ok(PullRequestRisk::new(
            kind,
            kind_confidence,
            Risk::from_score(risk),
            risk_confidence,
        ))
    }
}

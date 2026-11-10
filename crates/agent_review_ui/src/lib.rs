//! Reviewing a pull request with an ACP agent: the Review with an agent form, the run
//! that streams the agent's progress and findings into the pull request, and the card
//! that shows the run with its permission prompts in the thread. Also asking an agent
//! about selected diff lines, and the card with its answer and suggested comment.

mod coverage;
mod launcher;
mod permission;
mod question;
mod question_card;
mod run;
mod run_card;
mod start_form;

use std::collections::HashMap;

use gpui_kit::{App, Entity, Global};
use pr_model::PullRequestKey;

pub use launcher::AgentLauncher;
pub use question::{AgentQuestion, AskOptions, ask_about_lines};
pub use question_card::AgentQuestionCard;
pub use run::{Activity, AgentRun, RunSetup, RunState};
pub use run_card::{AgentRunCard, AgentRunDetails};
pub use start_form::{StartOptions, StartReviewEvent, StartReviewForm, start_review};

/// Agent reviews started this session, per pull request, oldest first. Several agents
/// can review one pull request at once; the same agent only once at a time.
#[derive(Default)]
pub struct AgentRuns {
    runs: HashMap<PullRequestKey, Vec<Entity<AgentRun>>>,
}

impl Global for AgentRuns {}

/// Runs for `key` in progress, or failed and not dismissed, oldest first.
pub fn active_runs(key: &PullRequestKey, cx: &App) -> Vec<Entity<AgentRun>> {
    cx.try_global::<AgentRuns>()
        .and_then(|runs| runs.runs.get(key))
        .map(|runs| {
            runs.iter()
                .filter(|run| !matches!(run.read(cx).state(), RunState::Finished(_)))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// How many agents are reviewing `key` right now.
pub fn running_count(key: &PullRequestKey, cx: &App) -> usize {
    active_runs(key, cx)
        .iter()
        .filter(|run| !run.read(cx).state().is_over())
        .count()
}

/// Whether the agent named `name` is reviewing `key` right now.
pub fn is_reviewing(key: &PullRequestKey, name: &str, cx: &App) -> bool {
    active_runs(key, cx).iter().any(|run| {
        let run = run.read(cx);
        !run.state().is_over() && run.agent().login == name
    })
}

/// Every run for `key` this session, newest first.
pub fn runs_for(key: &PullRequestKey, cx: &App) -> Vec<Entity<AgentRun>> {
    cx.try_global::<AgentRuns>()
        .and_then(|runs| runs.runs.get(key))
        .map(|runs| runs.iter().rev().cloned().collect())
        .unwrap_or_default()
}

pub(crate) fn insert_run(key: PullRequestKey, run: Entity<AgentRun>, cx: &mut App) {
    cx.default_global::<AgentRuns>()
        .runs
        .entry(key)
        .or_default()
        .push(run);
    cx.refresh_windows();
}

pub(crate) fn remove_run(key: &PullRequestKey, run: &Entity<AgentRun>, cx: &mut App) {
    if let Some(runs) = cx
        .try_global::<AgentRuns>()
        .and_then(|runs| runs.runs.get(key))
        && runs.contains(run)
    {
        if let Some(runs) = cx.global_mut::<AgentRuns>().runs.get_mut(key) {
            runs.retain(|known| known != run);
        }
        cx.refresh_windows();
    }
}

/// Questions asked about diff lines this session, per pull request, oldest first.
#[derive(Default)]
pub struct AgentQuestions {
    questions: HashMap<PullRequestKey, Vec<Entity<AgentQuestion>>>,
}

impl Global for AgentQuestions {}

/// The questions asked about `key` and not dismissed, oldest first.
pub fn questions_for(key: &PullRequestKey, cx: &App) -> Vec<Entity<AgentQuestion>> {
    cx.try_global::<AgentQuestions>()
        .and_then(|questions| questions.questions.get(key))
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn insert_question(key: PullRequestKey, question: Entity<AgentQuestion>, cx: &mut App) {
    cx.default_global::<AgentQuestions>()
        .questions
        .entry(key)
        .or_default()
        .push(question);
    cx.refresh_windows();
}

pub(crate) fn remove_question(
    key: &PullRequestKey,
    question: &Entity<AgentQuestion>,
    cx: &mut App,
) {
    if let Some(questions) = cx.default_global::<AgentQuestions>().questions.get_mut(key) {
        questions.retain(|known| known != question);
    }
    cx.refresh_windows();
}

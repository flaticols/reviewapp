//! Triage answers as the review screen shows them: tags, and the order that puts
//! blocking threads and severe findings first.

use gpui_kit::App;
use pr_model::PullRequestKey;
use review_model::AgentFinding;
use triage::{FindingTriage, Severity, ThreadKind, ThreadLabel};
use triage_ui::TriageStoreExt as _;
use widgets_ui::{StatusTag, Tone};

pub(crate) fn finding(key: &PullRequestKey, id: &str, cx: &App) -> Option<FindingTriage> {
    cx.triage()?.read(cx).finding(key, id)
}

pub(crate) fn thread(key: &PullRequestKey, id: &str, cx: &App) -> Option<ThreadLabel> {
    cx.triage()?.read(cx).thread(key, id)
}

fn severity_tone(severity: Severity) -> Tone {
    match severity {
        Severity::Critical | Severity::Major => Tone::Danger,
        Severity::Moderate => Tone::Warning,
        Severity::Minor | Severity::Nit => Tone::Neutral,
    }
}

/// `Major · Bug`, toned by severity.
pub(crate) fn finding_tag(triage: &FindingTriage) -> StatusTag {
    let label = match triage.category {
        Some(category) => format!("{} · {}", triage.severity.label(), category.label()),
        None => triage.severity.label().to_string(),
    };
    StatusTag::new(severity_tone(triage.severity), label)
}

/// The thread's kind, toned by how much it asks of the author.
pub(crate) fn thread_tag(label: &ThreadLabel) -> StatusTag {
    let tone = match label.kind {
        ThreadKind::Blocking => Tone::Danger,
        ThreadKind::Suggestion => Tone::Accent,
        ThreadKind::Nit => Tone::Neutral,
        ThreadKind::Question => Tone::Info,
        ThreadKind::Praise => Tone::Success,
    };
    StatusTag::new(tone, label.kind.label())
}

/// Lower comes first: blocking threads and severe findings, then the rest, then noise.
pub(crate) fn finding_rank(triage: Option<FindingTriage>) -> u8 {
    match triage {
        None => 5,
        Some(triage) if triage.is_likely_noise() => 9,
        Some(triage) => 4 - triage.severity as u8,
    }
}

pub(crate) fn thread_rank(label: Option<ThreadLabel>) -> u8 {
    match label.map(|label| label.kind) {
        Some(ThreadKind::Blocking) => 0,
        Some(ThreadKind::Praise) => 8,
        _ => 5,
    }
}

/// `findings` ordered for the Unresolved filter.
pub(crate) fn sort_findings(key: &PullRequestKey, findings: &mut [&AgentFinding], cx: &App) {
    findings.sort_by_key(|finding| finding_rank(self::finding(key, &finding.id, cx)));
}

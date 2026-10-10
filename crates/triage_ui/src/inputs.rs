//! What the store sends a provider, built from the app's models.

use diff_model::{FileDiff, LineKind};
use pr_model::PullRequest;
use review_model::{AgentFinding, PullRequestDetail, ReviewThread};
use triage::{Comment, FindingInput, PullRequestInput, ThreadInput};

/// How many diff lines either side of the line a finding or thread is on.
const CONTEXT_LINES: usize = 6;

/// The diff lines around head-side `line` of `file`, prefixed as a unified diff.
pub(crate) fn code_around(file: Option<&FileDiff>, line: u32) -> Vec<String> {
    let Some(file) = file else {
        return Vec::new();
    };
    for hunk in &file.hunks {
        let Some(at) = hunk.lines.iter().position(|l| l.new_line == Some(line)) else {
            continue;
        };
        let start = at.saturating_sub(CONTEXT_LINES);
        let end = (at + CONTEXT_LINES + 1).min(hunk.lines.len());
        return hunk.lines[start..end]
            .iter()
            .map(|l| {
                let mark = match l.kind {
                    LineKind::Added => '+',
                    LineKind::Removed => '-',
                    LineKind::Context => ' ',
                };
                format!("{mark}{}", l.text)
            })
            .collect();
    }
    Vec::new()
}

pub(crate) fn finding(detail: &PullRequestDetail, finding: &AgentFinding) -> FindingInput {
    FindingInput::new(
        finding.path.clone(),
        finding.line,
        finding.body.clone(),
        code_around(detail.file(&finding.path), finding.line),
    )
}

pub(crate) fn thread(detail: &PullRequestDetail, thread: &ReviewThread) -> ThreadInput {
    ThreadInput::new(
        thread.path.clone(),
        thread.line,
        thread
            .comments
            .iter()
            .map(|comment| Comment::new(comment.author.login.clone(), comment.body.clone()))
            .collect(),
        code_around(detail.file(&thread.path), thread.line),
    )
}

pub(crate) fn pull_request(pull_request: &PullRequest) -> PullRequestInput {
    let mut input = PullRequestInput::new(pull_request.title.clone());
    input.description = pull_request.body_excerpt.clone();
    input.head_branch = pull_request.head_ref.clone();
    input.base_branch = pull_request.base_ref.clone();
    input.labels = pull_request.labels.clone();
    input.changed_files = pull_request.changed_files;
    input.additions = pull_request.additions;
    input.deletions = pull_request.deletions;
    input.paths = pull_request.paths.clone();
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use diff_model::{DiffLine, FileChange, Hunk};

    #[test]
    fn code_context_centers_on_the_line_and_marks_changes() {
        let lines: Vec<DiffLine> = (1..=20)
            .map(|n| {
                let kind = if n == 10 {
                    LineKind::Added
                } else {
                    LineKind::Context
                };
                DiffLine::new(kind, Some(n), Some(n), format!("line {n}"))
            })
            .collect();
        let file = FileDiff::new(
            "a.rs",
            FileChange::Modified,
            vec![Hunk::new(1, 20, 1, 20, "", lines)],
        );
        let code = code_around(Some(&file), 10);
        assert_eq!(code.len(), 13);
        assert_eq!(code[6], "+line 10");
        assert_eq!(code[0], " line 4");
        assert!(code_around(Some(&file), 99).is_empty());
    }
}

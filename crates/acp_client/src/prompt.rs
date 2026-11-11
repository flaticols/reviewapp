use std::fmt::Write as _;

use diff_model::{DiffLine, FileDiff, LineKind};

/// Which part of the pull request the agent reviews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewScope {
    WholePullRequest,
    UnviewedFiles,
    SinceLastReview,
}

/// How hard the agent looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReviewDepth {
    /// Explores the codebase around the change, follows callers and tests, and runs
    /// checks, working through a plan.
    #[default]
    Thorough,
    /// Reviews the diff in the prompt, opening files only when the diff isn't enough.
    Quick,
}

/// What the review prompt is built from.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PromptInput<'a> {
    pub repository: &'a str,
    pub number: u64,
    pub title: &'a str,
    pub description: &'a str,
    pub base_ref: &'a str,
    pub head_ref: &'a str,
    pub files: Vec<&'a FileDiff>,
    pub instructions: &'a str,
    /// Existing review comments, as `path:line author: text`, when the user shares them.
    pub discussion: Vec<String>,
    pub can_report: bool,
    pub depth: ReviewDepth,
    /// The head commit the working directory is checked out at, when it is a checkout.
    pub head_sha: Option<&'a str>,
}

impl<'a> PromptInput<'a> {
    /// Input for reviewing `files` of pull request `number`.
    pub fn new(repository: &'a str, number: u64, title: &'a str, files: Vec<&'a FileDiff>) -> Self {
        Self {
            repository,
            number,
            title,
            description: "",
            base_ref: "main",
            head_ref: "",
            files,
            instructions: "",
            discussion: Vec::new(),
            can_report: true,
            depth: ReviewDepth::Thorough,
            head_sha: None,
        }
    }
}

/// Renders a file diff as a unified patch, the way `git diff` prints it.
pub fn unified_patch(file: &FileDiff) -> String {
    let mut patch = String::new();
    let old = file.old_path.as_deref().unwrap_or(&file.path);
    writeln!(patch, "--- a/{old}").ok();
    writeln!(patch, "+++ b/{}", file.path).ok();
    for hunk in &file.hunks {
        writeln!(patch, "{}", hunk.header()).ok();
        for line in &hunk.lines {
            let sign = match line.kind {
                LineKind::Context => ' ',
                LineKind::Added => '+',
                LineKind::Removed => '-',
            };
            writeln!(patch, "{sign}{}", line.text).ok();
        }
    }
    patch
}

/// The prompt that asks an agent to review a pull request and report findings.
pub fn review_prompt(input: &PromptInput<'_>) -> String {
    let mut prompt = String::new();
    writeln!(
        prompt,
        "Review pull request #{} in {}: \u{201c}{}\u{201d} ({} → {}).",
        input.number, input.repository, input.title, input.head_ref, input.base_ref
    )
    .ok();
    let thorough = input.depth == ReviewDepth::Thorough;
    match input.head_sha {
        Some(sha) => writeln!(
            prompt,
            "The working directory is a detached checkout of the pull request's head commit {sha}. `git diff origin/{base}...HEAD` shows the whole change; add `-- <path>` for one file. Don't modify, commit or push anything.",
            base = input.base_ref
        ),
        None => writeln!(
            prompt,
            "The working directory holds `pull-request.diff` with the change; the rest of the repository isn't available. Don't modify anything."
        ),
    }
    .ok();
    if !input.description.trim().is_empty() {
        writeln!(prompt, "\nDescription:\n{}", input.description.trim()).ok();
    }
    if !input.instructions.trim().is_empty() {
        writeln!(
            prompt,
            "\nThe reviewer asks you to: {}",
            input.instructions.trim()
        )
        .ok();
    }
    if !input.discussion.is_empty() {
        writeln!(prompt, "\nExisting review discussion:").ok();
        for comment in &input.discussion {
            writeln!(prompt, "- {comment}").ok();
        }
    }
    let report = if input.can_report {
        "call the `report_finding` tool with the file path, the line in the new version, and a short Markdown explanation with a suggested fix. Report each finding as soon as you're confident in it, not all at the end. Only reported findings reach the reviewer: a problem described only in your reply is lost"
    } else {
        "list it as `path:line — explanation`"
    };
    if thorough && input.head_sha.is_some() {
        writeln!(
            prompt,
            "\nSwitchfold's tools `update_plan` and `report_finding` come from the `switchfold` MCP server; some agents name them `mcp__switchfold__update_plan` and `mcp__switchfold__report_finding` and load them with tool search before the first call."
        )
        .ok();
        writeln!(
            prompt,
            "\nDo a thorough review, the way a senior engineer who owns this code would. This takes a while; don't rush it.\n\
1. Run `git diff --stat origin/{base}...HEAD`, then share a plan with the `update_plan` tool, one step per changed file or group of files that change together. Call it again whenever a step starts or finishes: the reviewer follows your progress there.\n\
2. For each item, read the diff of those files, then the code around the change, not only the hunks: the whole function or type, the code it calls, and the code that calls it. Search the repository for other uses of anything whose behaviour changed.\n\
3. Look for correctness bugs, unhandled edge cases and errors, broken contracts with callers, concurrency and resource problems, security issues, performance regressions, and tests that are missing or don't test what they claim.\n\
4. Generated files, fixtures and golden outputs: check that they change the way the code change says they should, then skim the rest of them.\n\
5. When it would settle a question, run fast checks such as the affected package's tests, a type check or a linter. You'll be asked for permission before each command.\n\
6. For every concrete problem, {report}. Skip style nits unless asked.\n\
Work through every item of the plan before you finish.\n\
Finish with a short summary: what you checked, what you didn't and why, and how risky the change looks.",
            base = input.base_ref
        )
        .ok();
    } else {
        writeln!(
            prompt,
            "\nFor every concrete problem, {report}. Skip style nits unless asked. Don't restate the diff.\nFinish with one short paragraph summarizing the review."
        )
        .ok();
    }
    writeln!(prompt, "\nChanged files ({}):", input.files.len()).ok();
    for file in &input.files {
        writeln!(
            prompt,
            "- {} (+{} −{})",
            file.path,
            file.additions(),
            file.deletions()
        )
        .ok();
    }
    // A checkout lets a thorough review read the change itself; the diff is inlined only
    // when there's nothing else to read or a quick review is asked for.
    if !thorough || input.head_sha.is_none() {
        writeln!(prompt, "\nDiff:\n```diff").ok();
        for file in &input.files {
            if !file.is_binary {
                prompt.push_str(&unified_patch(file));
            }
        }
        writeln!(prompt, "```").ok();
    }
    prompt
}

/// What a question about selected diff lines is built from.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct QuestionInput<'a> {
    pub repository: &'a str,
    pub number: u64,
    pub title: &'a str,
    pub description: &'a str,
    pub base_ref: &'a str,
    pub head_ref: &'a str,
    /// The file the lines are in.
    pub file: &'a FileDiff,
    /// The selected lines, top to bottom.
    pub lines: Vec<&'a DiffLine>,
    pub question: &'a str,
    /// Existing review comments on the file, as `path:line author: text`, when the user
    /// shares them.
    pub discussion: Vec<String>,
    /// Whether the session has the `suggest_comment` tool.
    pub can_suggest: bool,
    /// The head commit the working directory is checked out at, when it is a checkout.
    pub head_sha: Option<&'a str>,
}

impl<'a> QuestionInput<'a> {
    /// Input for asking `question` about `lines` of `file` in pull request `number`.
    pub fn new(
        repository: &'a str,
        number: u64,
        title: &'a str,
        file: &'a FileDiff,
        lines: Vec<&'a DiffLine>,
        question: &'a str,
    ) -> Self {
        Self {
            repository,
            number,
            title,
            description: "",
            base_ref: "main",
            head_ref: "",
            file,
            lines,
            question,
            discussion: Vec::new(),
            can_suggest: true,
            head_sha: None,
        }
    }
}

/// `line 14` or `lines 14–28`, by the numbers in the new version where the selection
/// has any, else by the old ones, said to be removed.
fn span_label(lines: &[&DiffLine]) -> String {
    let span = |numbers: Vec<u32>| match (numbers.first(), numbers.last()) {
        (Some(first), Some(last)) if first == last => Some(format!("line {first}")),
        (Some(first), Some(last)) => Some(format!("lines {first}\u{2013}{last}")),
        _ => None,
    };
    let head = lines
        .iter()
        .filter(|line| line.kind != LineKind::Removed)
        .filter_map(|line| line.new_line)
        .collect();
    let base = lines.iter().filter_map(|line| line.old_line).collect();
    span(head)
        .or_else(|| span(base).map(|label| format!("removed {label}")))
        .unwrap_or_else(|| "the selected lines".into())
}

/// The prompt that asks an agent a reviewer's question about selected diff lines, and
/// for a short comment the reviewer could post on them.
pub fn question_prompt(input: &QuestionInput<'_>) -> String {
    let mut prompt = String::new();
    let path = &input.file.path;
    writeln!(
        prompt,
        "A reviewer of pull request #{} in {}: \u{201c}{}\u{201d} ({} → {}) asks about {} of `{path}`.",
        input.number,
        input.repository,
        input.title,
        input.head_ref,
        input.base_ref,
        span_label(&input.lines),
    )
    .ok();
    match input.head_sha {
        Some(sha) => writeln!(
            prompt,
            "The working directory is a detached checkout of the pull request's head commit {sha}, so the whole repository is there to read. `git diff origin/{base}...HEAD` shows the whole change. Don't modify, commit or push anything.",
            base = input.base_ref
        ),
        None => writeln!(
            prompt,
            "The working directory holds `pull-request.diff` with the change; the rest of the repository isn't available. Don't modify anything."
        ),
    }
    .ok();
    if !input.description.trim().is_empty() {
        writeln!(
            prompt,
            "\nPull request description:\n{}",
            input.description.trim()
        )
        .ok();
    }
    writeln!(
        prompt,
        "\nThe reviewer's question:\n{}",
        input.question.trim()
    )
    .ok();
    writeln!(
        prompt,
        "\nThe selected lines, each with its old and new line number and diff sign:\n```diff"
    )
    .ok();
    let number = |number: Option<u32>| number.map(|n| n.to_string()).unwrap_or_default();
    for line in &input.lines {
        let sign = match line.kind {
            LineKind::Context => ' ',
            LineKind::Added => '+',
            LineKind::Removed => '-',
        };
        writeln!(
            prompt,
            "{:>5} {:>5} {sign}{}",
            number(line.old_line),
            number(line.new_line),
            line.text
        )
        .ok();
    }
    writeln!(prompt, "```").ok();
    if !input.file.is_binary {
        writeln!(
            prompt,
            "\nThe whole change to `{path}`:\n```diff\n{}```",
            unified_patch(input.file)
        )
        .ok();
    }
    if !input.discussion.is_empty() {
        writeln!(prompt, "\nExisting review discussion on this file:").ok();
        for comment in &input.discussion {
            writeln!(prompt, "- {comment}").ok();
        }
    }
    let ground = if input.head_sha.is_some() {
        "Before you answer, read what the question depends on: the whole function or type around the lines, the code they call, the code that calls them, and their tests. Search the repository rather than guess."
    } else {
        "Answer from the diff; say so where the answer depends on code you can't see."
    };
    let suggest = if input.can_suggest {
        "After the reply, call the `suggest_comment` tool once with the comment the reviewer could post on these lines. Switchfold's tools come from the `switchfold` MCP server; some agents name this one `mcp__switchfold__suggest_comment` and load it with tool search before the call."
    } else {
        "End the reply with a line `Suggested comment:` followed by the comment the reviewer could post on these lines."
    };
    writeln!(
        prompt,
        "\n{ground}\n\
Reply in Markdown. Start with the direct answer in a sentence or two, then explain it in full: what the code does, why, and what follows from it, citing code as `path:line`. Don't restate the selection.\n\
{suggest} Write the comment as the reviewer would, to the author: one to three sentences, specific and actionable, no praise and no preamble. When the lines need no change, make it a short question or acknowledgement worth posting."
    )
    .ok();
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use diff_model::{FileChange, Hunk};

    fn file() -> FileDiff {
        FileDiff::new(
            "src/a.rs",
            FileChange::Modified,
            vec![Hunk::new(
                1,
                1,
                1,
                1,
                "fn a()",
                vec![
                    DiffLine::new(LineKind::Removed, Some(1), None, "old"),
                    DiffLine::new(LineKind::Added, None, Some(1), "new"),
                ],
            )],
        )
    }

    #[test]
    fn patches_render_like_git() {
        assert_eq!(
            unified_patch(&file()),
            "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,1 +1,1 @@ fn a()\n-old\n+new\n"
        );
    }

    #[test]
    fn prompt_mentions_the_tool_and_the_instructions() {
        let file = file();
        let mut input = PromptInput::new("flaticols/switchfold", 482, "Render thread", vec![&file]);
        input.instructions = "Focus on concurrency.";
        let prompt = review_prompt(&input);
        assert!(prompt.contains("report_finding"));
        assert!(prompt.contains("Focus on concurrency."));
        assert!(prompt.contains("- src/a.rs (+1 −1)"));
        assert!(
            prompt.contains("+new"),
            "without a checkout the diff is inlined"
        );
    }

    #[test]
    fn thorough_reviews_of_a_checkout_explore_instead_of_reading_an_inlined_diff() {
        let file = file();
        let mut input = PromptInput::new("flaticols/switchfold", 482, "Render thread", vec![&file]);
        input.head_sha = Some("abc1234");
        input.base_ref = "main";
        let prompt = review_prompt(&input);
        assert!(prompt.contains("git diff origin/main...HEAD"));
        assert!(prompt.contains("share a plan with the `update_plan` tool"));
        assert!(prompt.contains("git diff --stat origin/main...HEAD"));
        assert!(!prompt.contains("+new"));
        input.depth = ReviewDepth::Quick;
        let quick = review_prompt(&input);
        assert!(quick.contains("+new"));
        assert!(!quick.contains("update_plan"));
    }

    #[test]
    fn a_question_carries_the_selection_the_file_and_the_ask_for_a_comment() {
        let file = file();
        let lines: Vec<&DiffLine> = file.hunks[0].lines.iter().collect();
        let mut input = QuestionInput::new(
            "flaticols/switchfold",
            482,
            "Render thread",
            &file,
            lines,
            "Why was this renamed?",
        );
        input.head_sha = Some("abc1234");
        let prompt = question_prompt(&input);
        assert!(prompt.contains("asks about line 1 of `src/a.rs`"));
        assert!(prompt.contains("Why was this renamed?"));
        assert!(prompt.contains("    1       -old\n          1 +new"));
        assert!(prompt.contains("@@ -1,1 +1,1 @@ fn a()"));
        assert!(prompt.contains("`suggest_comment`"));
        assert!(prompt.contains("the code that calls them"));

        input.lines.truncate(1);
        input.can_suggest = false;
        input.head_sha = None;
        let prompt = question_prompt(&input);
        assert!(prompt.contains("asks about removed line 1 of"));
        assert!(prompt.contains("Suggested comment:"));
        assert!(!prompt.contains("suggest_comment"));
    }
}

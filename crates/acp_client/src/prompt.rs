use std::fmt::Write as _;

use diff_model::{FileDiff, LineKind};

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

#[cfg(test)]
mod tests {
    use super::*;
    use diff_model::{DiffLine, FileChange, Hunk};

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
        input.head_sha = Some("016e1f0");
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
}

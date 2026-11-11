//! Which changed files an agent's tool call looked at, read from its title, command and
//! locations. Agents mostly work through shell commands, so file names come relative
//! to a `cd` earlier in the command, as absolute paths into the worktree, or as a
//! folder handed to `git diff`.

use std::path::Path;

/// Changed files in `scope` (paths relative to `root`) that `text` names: a command
/// line or a tool title. A file counts when named exactly; a folder counts for the files
/// under it when `git diff` or `git show` is given it; a `git diff` without paths counts
/// every file.
pub(crate) fn files_named(text: &str, root: &Path, scope: &[String]) -> Vec<String> {
    let root = root.to_string_lossy();
    let mut found = Vec::new();
    // A `cd` lasts for the rest of the command line, across `;` and `&&`.
    let mut dir = String::new();
    for command in text.split([';', '\n']).flat_map(|part| part.split("&&")) {
        let words = words(command);
        let diffs = is_diff(&words);
        let mut named_path = false;
        let mut words = words.iter().peekable();
        while let Some(word) = words.next() {
            if word == "cd"
                && let Some(target) = words.peek()
            {
                dir = relative(target, &root, "");
                words.next();
                continue;
            }
            let path = relative(word, &root, &dir);
            if path.is_empty() || path.starts_with('-') {
                continue;
            }
            for file in scope {
                let under = diffs && file.starts_with(&format!("{path}/"));
                if *file == path || under {
                    found.push(file.clone());
                    named_path = true;
                }
            }
        }
        if diffs && !named_path && !command.contains(" -- ") {
            found.extend(scope.iter().cloned());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The words of a shell command, with quotes, pipes and redirections stripped.
fn words(command: &str) -> Vec<String> {
    command
        .split(|c: char| c.is_whitespace() || matches!(c, '|' | '(' | ')' | '<' | '>'))
        .map(|word| word.trim_matches(|c| matches!(c, '"' | '\'' | '`' | ',')))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether the command prints a diff: `git diff`, `git --no-pager diff`, `git show`.
fn is_diff(words: &[String]) -> bool {
    words
        .windows(2)
        .any(|pair| pair[0] == "git" && matches!(pair[1].as_str(), "diff" | "show" | "--no-pager"))
        && words.iter().any(|word| word == "diff" || word == "show")
}

/// `word` as a path relative to the repository root: absolute paths into `root` lose
/// the prefix, relative ones are joined to `dir`.
fn relative(word: &str, root: &str, dir: &str) -> String {
    let word = word.trim_end_matches('/');
    let path = match word.strip_prefix(root) {
        Some(rest) => rest.trim_start_matches('/').to_string(),
        None if word.starts_with('/') => return String::new(),
        None if dir.is_empty() => word.to_string(),
        None => format!("{dir}/{word}"),
    };
    normalize(&path)
}

/// Resolves `.` and `..` segments.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Vec<String> {
        [
            "internal/api/resources/ndc/24-1/common/mappings_inline.go",
            "internal/api/resources/ndc/24-1/offerprice/response.go",
            "bdd/__goldens__/ndc2130/OrderQuote/a.xml",
            "bdd/__goldens__/ndc241/ServiceList/b.xml",
        ]
        .map(String::from)
        .to_vec()
    }

    const ROOT: &str = "/Users/me/.switchfold/review-worktrees/repo-pr-1";

    #[test]
    fn files_after_cd_count() {
        let found = files_named(
            "cd internal/api/resources/ndc/24-1/common; sed -n 560,760p mappings_inline.go; grep -rn \"Preferred\" --include=*.go .",
            Path::new(ROOT),
            &scope(),
        );
        assert_eq!(
            found,
            ["internal/api/resources/ndc/24-1/common/mappings_inline.go"]
        );
    }

    #[test]
    fn absolute_paths_into_the_worktree_count() {
        let found = files_named(
            &format!("cat {ROOT}/internal/api/resources/ndc/24-1/offerprice/response.go"),
            Path::new(ROOT),
            &scope(),
        );
        assert_eq!(
            found,
            ["internal/api/resources/ndc/24-1/offerprice/response.go"]
        );
    }

    #[test]
    fn diffs_of_a_folder_count_every_file_under_it() {
        let found = files_named(
            &format!("cd {ROOT}; git diff origin/main...HEAD --stat -- bdd | tail -1"),
            Path::new(ROOT),
            &scope(),
        );
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|file| file.starts_with("bdd/")));
    }

    #[test]
    fn a_whole_diff_counts_everything_and_a_grep_of_a_folder_nothing() {
        assert_eq!(
            files_named(
                "git --no-pager diff origin/main...HEAD",
                Path::new(ROOT),
                &scope()
            )
            .len(),
            4
        );
        assert!(
            files_named(
                "grep -rn \"NewDataListCollectors(\" internal",
                Path::new(ROOT),
                &scope()
            )
            .is_empty()
        );
    }
}

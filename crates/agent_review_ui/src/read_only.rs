//! Telling a shell command that only reads from one that might change something, so
//! the reviewer can let agents read without a prompt for each `sed -n` or `git diff`.
//!
//! An allowlist, conservative by construction: anything it doesn't understand asks.

/// Whether `command` only reads: every command in it is a known reader with arguments
/// that can't write, run another program or reach the network, and nothing redirects
/// output into a file.
pub(crate) fn is_read_only(command: &str) -> bool {
    if ["`", "$(", "<(", ">("]
        .iter()
        .any(|it| command.contains(it))
    {
        return false;
    }
    let Some(segments) = segments(command) else {
        return false;
    };
    segments.iter().any(|words| !words.is_empty())
        && segments
            .iter()
            .all(|words| words.is_empty() || allowed(words))
}

/// The simple commands of `command`, split at `|`, `||`, `&&`, `;` and newlines, each as
/// its words with quotes removed. `None` when it holds anything else the shell would
/// act on: a file redirect, input redirect, background `&`, a subshell.
fn segments(command: &str) -> Option<Vec<Vec<String>>> {
    let mut segments = vec![Vec::new()];
    let mut word: Option<String> = None;
    let mut chars = command.chars().peekable();
    fn end_word(word: &mut Option<String>, segments: &mut [Vec<String>]) {
        if let Some(word) = word.take() {
            segments.last_mut().expect("never empty").push(word);
        }
    }
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                let word = word.get_or_insert_with(String::new);
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                let word = word.get_or_insert_with(String::new);
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            c @ ('"' | '\\' | '$') => word.push(c),
                            c => {
                                word.push('\\');
                                word.push(c);
                            }
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' => match chars.next()? {
                '\n' => {}
                c => word.get_or_insert_with(String::new).push(c),
            },
            '#' if word.is_none() => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                end_word(&mut word, &mut segments);
                segments.push(Vec::new());
            }
            ' ' | '\t' => end_word(&mut word, &mut segments),
            '\n' | ';' => {
                end_word(&mut word, &mut segments);
                segments.push(Vec::new());
            }
            '|' => {
                end_word(&mut word, &mut segments);
                if matches!(chars.peek(), Some('|' | '&')) {
                    chars.next();
                }
                segments.push(Vec::new());
            }
            '&' => {
                end_word(&mut word, &mut segments);
                match chars.peek() {
                    Some('&') => {
                        chars.next();
                        segments.push(Vec::new());
                    }
                    Some('>') => {
                        chars.next();
                        if !redirects_to_null(&mut chars) {
                            return None;
                        }
                    }
                    _ => return None,
                }
            }
            '>' => {
                // `2>` and `>`: only a digit-only word before it is a file descriptor.
                if word
                    .as_deref()
                    .is_some_and(|w| !w.chars().all(|c| c.is_ascii_digit()))
                {
                    return None;
                }
                word = None;
                if !redirects_to_null(&mut chars) {
                    return None;
                }
            }
            '<' | '(' | ')' => return None,
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    end_word(&mut word, &mut segments);
    Some(segments)
}

/// Reads the target of a `>` just consumed: `&1`/`&2`, or `/dev/null`. Any other target
/// writes a file.
fn redirects_to_null(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    if chars.peek() == Some(&'>') {
        chars.next();
    }
    if chars.peek() == Some(&'&') {
        chars.next();
        return matches!(chars.next(), Some('1' | '2'));
    }
    while chars.peek() == Some(&' ') {
        chars.next();
    }
    let mut target = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || matches!(c, ';' | '|' | '&') {
            break;
        }
        target.push(c);
        chars.next();
    }
    target == "/dev/null"
}

/// Whether the simple command `words` is a reader the allowlist knows, with arguments
/// that keep it one.
fn allowed(words: &[String]) -> bool {
    let (program, args) = (&words[0], &words[1..]);
    // A path could be a program the pull request brought; `NAME=` changes behaviour.
    if program.contains(['/', '$', '=']) {
        return false;
    }
    match program.as_str() {
        "cat" | "head" | "tail" | "wc" | "ls" | "pwd" | "echo" | "printf" | "true" | "false"
        | "which" | "cut" | "tr" | "nl" | "diff" | "file" | "stat" | "basename" | "dirname"
        | "realpath" | "comm" | "column" | "cd" | "grep" | "egrep" | "fgrep" | "jq" | "du"
        | "od" | "shasum" | "md5" | "date" => true,
        "rg" => !args.iter().any(|arg| arg.starts_with("--pre")),
        "sort" => !args.iter().any(|arg| {
            arg.starts_with("--output")
                || arg.starts_with("--compress-program")
                || (arg.starts_with('-') && !arg.starts_with("--") && arg.contains('o'))
        }),
        "uniq" => args.iter().filter(|arg| !arg.starts_with('-')).count() <= 1,
        "tree" => !args.iter().any(|arg| arg == "-o"),
        "find" => !args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "-exec" | "-execdir" | "-ok" | "-okdir" | "-delete" | "-fls"
            ) || arg.starts_with("-fprint")
        }),
        "sed" => sed_reads(args),
        "awk" => awk_reads(args),
        "git" => git_reads(args),
        _ => false,
    }
}

/// `sed` that prints: no in-place edit, no script file, and scripts made only of
/// addresses and print, delete, line-number and quit commands. `s`, `w`, `r` and `e`
/// can write or run, so they ask.
fn sed_reads(args: &[String]) -> bool {
    let mut scripts = Vec::new();
    let mut positional = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if let Some(script) = arg.strip_prefix("--expression=") {
            scripts.push(script.to_string());
        } else if arg == "--expression" {
            scripts.push(args.next().cloned().unwrap_or_default());
        } else if arg.starts_with("--in-place") || arg.starts_with("--file") {
            return false;
        } else if arg.starts_with("--") {
        } else if let Some(flags) = arg.strip_prefix('-').filter(|flags| !flags.is_empty()) {
            if flags.contains(['i', 'f']) {
                return false;
            }
            if flags.contains('e') {
                scripts.push(args.next().cloned().unwrap_or_default());
            }
        } else {
            positional.push(arg.clone());
        }
    }
    if scripts.is_empty() {
        let Some(script) = positional.first() else {
            return false;
        };
        scripts.push(script.clone());
    }
    scripts.iter().all(|script| plain_sed_script(script))
}

/// A sed script of `/regex/` and line addresses with `p`, `d`, `n`, `=`, `q`, `l`, and
/// `s` or `y` substitutions whose flags neither write (`w`) nor run (`e`) anything.
fn plain_sed_script(script: &str) -> bool {
    /// Skips past the next unescaped `delimiter`; false when there is none.
    fn skip_to(chars: &mut std::str::Chars<'_>, delimiter: char) -> bool {
        loop {
            match chars.next() {
                None => return false,
                Some('\\') => {
                    chars.next();
                }
                Some(c) if c == delimiter => return true,
                Some(_) => {}
            }
        }
    }
    let mut chars = script.chars();
    while let Some(c) = chars.next() {
        match c {
            '/' => {
                if !skip_to(&mut chars, '/') {
                    return false;
                }
            }
            's' | 'y' => {
                let Some(delimiter) = chars.next().filter(|d| !d.is_alphanumeric() && *d != '\\')
                else {
                    return false;
                };
                if !skip_to(&mut chars, delimiter) || !skip_to(&mut chars, delimiter) {
                    return false;
                }
                for flag in chars.clone() {
                    match flag {
                        'g' | 'I' | 'i' | 'p' | 'm' | 'M' => {}
                        f if f.is_ascii_digit() => {}
                        ';' | '}' | ' ' | '\n' => break,
                        _ => return false,
                    }
                    chars.next();
                }
            }
            c if c.is_ascii_digit() => {}
            ',' | '$' | ';' | '!' | 'p' | 'd' | 'n' | '=' | 'q' | 'l' | '+' | '~' | '{' | '}'
            | ' ' | '\n' => {}
            _ => return false,
        }
    }
    true
}

/// `awk` with an inline program that can't run commands, write files or read input
/// beyond its arguments.
fn awk_reads(args: &[String]) -> bool {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "-f" || arg.starts_with("--file") || arg.starts_with("-f") {
            return false;
        }
        if arg == "-v" || arg == "-F" {
            args.next();
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        // `>=` compares; a lone `>` could redirect `print` into a file.
        let program = arg.replace(">=", "");
        return !["system", "getline", "|", ">", "@", "ENVIRON"]
            .iter()
            .any(|it| program.contains(it));
    }
    false
}

/// `git` subcommands that read history and the work tree, without an external diff,
/// pager or output file.
fn git_reads(args: &[String]) -> bool {
    let mut args = args.iter();
    let subcommand = loop {
        match args.next().map(String::as_str) {
            None => return false,
            Some("--no-pager" | "-P" | "--no-optional-locks") => {}
            Some("-C") => {
                args.next();
            }
            // `-c diff.external=` turns an external diff off; any other value could run one.
            Some("-c") => {
                if args.next().map(String::as_str) != Some("diff.external=") {
                    return false;
                }
            }
            Some(other) if other.starts_with('-') => return false,
            Some(subcommand) => break subcommand,
        }
    };
    let reads = matches!(
        subcommand,
        "diff"
            | "show"
            | "log"
            | "status"
            | "blame"
            | "rev-parse"
            | "ls-files"
            | "ls-tree"
            | "cat-file"
            | "merge-base"
            | "describe"
            | "shortlog"
            | "rev-list"
            | "name-rev"
            | "grep"
    );
    reads
        && !args.any(|arg| {
            arg.starts_with("--output")
                || arg == "--ext-diff"
                || arg.starts_with("--open-files-in-pager")
                || (subcommand == "grep" && arg.starts_with("-O"))
        })
}

#[cfg(test)]
mod tests {
    use super::is_read_only;

    #[test]
    fn commands_that_only_read_pass() {
        for command in [
            "sed -n 640,700p src/lib.rs; sed -n 706,770p src/lib.rs",
            r#"sed -n 285,440p src/task.rs | grep -n "Ownership\|Metadata""#,
            r#"head -12 tests/it.rs; grep -rn "go:build" --include="*_test.go" internal"#,
            "git --no-pager diff --no-ext-diff origin/main...HEAD -- src tests",
            "git -c diff.external= diff --no-ext-diff origin/main...HEAD -- src/a.rs",
            "git diff origin/main...HEAD --numstat -- bdd | awk '$1!=0{a+=$1} {d+=$2} END{print \"added:\",a+0}'",
            "git --no-pager show --no-ext-diff --stat 46d0a35; echo ---; git log -1 --format=%B 46d0a35",
            "grep -rnoE 'Contains\\([^,]+' internal --include='*.go' | sort | uniq -c | sort -rn | head",
            "sed -n '/^func Unmarshal/,/^}/p' src/decode.go",
            "git diff -U0 | grep '^[-+]' | sed -E 's/^([-+])[[:space:]]*/\\1/' | sort | uniq -c",
            "grep -rl x . | sed 's#/[^/]*$##' | sort -u",
            "awk 'NR>=2853 && NR<=3100' a.rs | grep -n Title",
            "ls $TMPDIR 2>/dev/null",
            "cat Cargo.toml | head -60 2>&1",
            "find . -name '*.rs' -newer Cargo.toml | wc -l",
            "rg -n 'fn main' --type rust && git status --short",
        ] {
            assert!(is_read_only(command), "{command}");
        }
    }

    #[test]
    fn commands_that_write_run_or_reach_out_ask() {
        for command in [
            "GR=$(go env GOROOT); sed -n 355,400p $GR/src/a.go",
            "go vet ./... 2>&1 | tail -5",
            "git show HEAD:a.json | python3 -c 'print(1)'",
            "git --no-pager show 46d0a35 > /tmp/a.diff",
            "sed -n 16,70p a.rs; which golangci-lint && golangci-lint run ./...",
            "cat skaffold.yaml | head; for f in a b; do echo $f; done",
            "git tag --contains 46d0a35 | head; gh pr view 1",
            "mkdir -p /tmp/x && tar -xzf a.tgz -C /tmp/x",
            "rm a.rs && git status --short",
            "sed -i 's/a/b/' a.rs",
            "sed -n 's/a/b/w out' a.rs",
            "sed 's/a/id/e' a.rs",
            "sed 's/a/b/;w out' a.rs",
            "awk 'NR>5 {print > \"out\"}' a",
            "sed -n '1p' a.rs > out",
            "find . -name '*.rs' -exec rm {} +",
            "find . -delete",
            "awk 'BEGIN{system(\"rm -rf ~\")}'",
            "awk '{print > \"out\"}' a",
            "sort -o out a",
            "sort -rno out a",
            "git -c core.pager=evil log",
            "git diff --output=out",
            "git diff --ext-diff",
            "git branch -D main",
            "GIT_EXTERNAL_DIFF=evil git diff",
            "./grep x a",
            "cat a | sh",
            "echo `id`",
            "cat <(id)",
            "cat a &",
            "cat < a",
            "(cd a && ls)",
            "cat a | xargs rm",
            "rg --pre evil x",
            "echo 'unterminated",
            "",
        ] {
            assert!(!is_read_only(command), "{command}");
        }
    }
}

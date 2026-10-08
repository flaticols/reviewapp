/// The account the GitHub CLI is signed in with.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct GhSession {
    pub login: String,
    pub host: String,
}

/// Reads the active `gh` account from `gh auth status`. Returns `None` when `gh` is not
/// installed or not signed in. Runs a process; call it off the UI thread.
pub fn gh_session() -> Option<GhSession> {
    let output = tool_path::command("gh")
        .args(["auth", "status", "--active"])
        .output()
        .ok()?;
    // `gh auth status` writes to stderr on older versions and stdout on newer ones.
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    parse_status(&text)
}

/// The profile name of the signed-in `gh` account, when it has one.
pub fn gh_user_name() -> Option<String> {
    let output = tool_path::command("gh")
        .args(["api", "user", "--jq", ".name // empty"])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !name.is_empty()).then_some(name)
}

fn parse_status(text: &str) -> Option<GhSession> {
    text.lines().find_map(|line| {
        let rest = line.split_once("Logged in to ")?.1;
        let (host, rest) = rest.split_once(" account ")?;
        let login = rest.split_whitespace().next()?;
        Some(GhSession {
            login: login.to_string(),
            host: host.to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_active_account() {
        let text = "github.com\n  ✓ Logged in to github.com account flaticols (keyring)\n  - Active account: true\n";
        assert_eq!(
            parse_status(text),
            Some(GhSession {
                login: "flaticols".into(),
                host: "github.com".into()
            })
        );
        assert_eq!(
            parse_status("You are not logged into any GitHub hosts."),
            None
        );
    }
}

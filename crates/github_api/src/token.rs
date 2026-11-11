use serde_json::Value;

/// What checking a personal access token found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenCheck {
    /// The token works and carries the scopes Switchfold needs. Fine-grained tokens
    /// report no scopes; their repository access is set on GitHub.
    Valid { login: String, scopes: Vec<String> },
    /// The token is not shaped like a GitHub token.
    Malformed,
    /// GitHub refused the token: it is wrong, expired or revoked.
    Rejected,
    /// The token works but lacks `repo` or `read:org`.
    MissingScopes { login: String, missing: Vec<String> },
    /// GitHub couldn't be reached to check it.
    Unreachable(String),
}

/// Scopes required to load pull requests.
pub const REQUIRED_SCOPES: [&str; 2] = ["repo", "read:org"];

/// Whether `token` is shaped like a classic or fine-grained GitHub token.
pub fn is_token_shaped(token: &str) -> bool {
    let token = token.trim();
    let body = token
        .strip_prefix("ghp_")
        .or_else(|| token.strip_prefix("github_pat_"));
    body.is_some_and(|body| {
        body.len() >= 16 && body.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Checks `token` against `host` by asking GitHub who it belongs to, through `gh api`
/// with the token in the environment. Blocks; call it off the UI thread.
pub fn check_token(host: &str, token: &str) -> TokenCheck {
    let token = token.trim();
    if !is_token_shaped(token) {
        return TokenCheck::Malformed;
    }
    let variable = if host == "github.com" {
        "GH_TOKEN"
    } else {
        "GH_ENTERPRISE_TOKEN"
    };
    let output = match tool_path::command("gh")
        .args(["api", "-i", "user", "--hostname", host])
        .env(variable, token)
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return TokenCheck::Unreachable(format!("Couldn’t run the GitHub CLI: {error}"));
        }
    };
    parse_response(&String::from_utf8_lossy(&output.stdout))
        .unwrap_or_else(|| TokenCheck::Unreachable(format!("{host} didn’t answer.")))
}

/// Reads `gh api -i user` output: status line, headers, blank line, JSON body.
fn parse_response(text: &str) -> Option<TokenCheck> {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))?;
    let mut lines = head.lines();
    let status: u16 = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    if status == 401 || status == 403 {
        return Some(TokenCheck::Rejected);
    }
    if status != 200 {
        return None;
    }
    let scopes: Option<Vec<String>> = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("x-oauth-scopes"))
        .map(|(_, value)| {
            value
                .split(',')
                .map(|scope| scope.trim().to_string())
                .filter(|scope| !scope.is_empty())
                .collect()
        });
    let login = serde_json::from_str::<Value>(body.trim())
        .ok()?
        .get("login")?
        .as_str()?
        .to_string();
    let Some(scopes) = scopes else {
        return Some(TokenCheck::Valid {
            login,
            scopes: Vec::new(),
        });
    };
    let missing: Vec<String> = REQUIRED_SCOPES
        .iter()
        .filter(|required| !scopes.iter().any(|scope| scope == *required))
        .map(|scope| scope.to_string())
        .collect();
    Some(if missing.is_empty() {
        TokenCheck::Valid { login, scopes }
    } else {
        TokenCheck::MissingScopes { login, missing }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_strings_that_are_not_github_tokens() {
        assert!(!is_token_shaped("hunter2"));
        assert!(!is_token_shaped("ghp_short"));
        assert!(is_token_shaped("ghp_0123456789abcdefghijABCDEF4f2a"));
        assert_eq!(check_token("github.com", "hunter2"), TokenCheck::Malformed);
    }

    #[test]
    fn reads_login_and_scopes_from_the_response() {
        let ok = "HTTP/2.0 200 OK\r\nX-Oauth-Scopes: read:org, repo, workflow\r\n\r\n{\"login\":\"octo\"}";
        assert_eq!(
            parse_response(ok),
            Some(TokenCheck::Valid {
                login: "octo".into(),
                scopes: vec!["read:org".into(), "repo".into(), "workflow".into()]
            })
        );
        let narrow = "HTTP/2.0 200 OK\nX-Oauth-Scopes: gist\n\n{\"login\":\"octo\"}";
        assert_eq!(
            parse_response(narrow),
            Some(TokenCheck::MissingScopes {
                login: "octo".into(),
                missing: vec!["repo".into(), "read:org".into()]
            })
        );
        let fine_grained = "HTTP/2.0 200 OK\n\n{\"login\":\"octo\"}";
        assert!(
            matches!(parse_response(fine_grained), Some(TokenCheck::Valid { scopes, .. }) if scopes.is_empty())
        );
        assert_eq!(
            parse_response("HTTP/2.0 401 Unauthorized\n\n{}"),
            Some(TokenCheck::Rejected)
        );
    }

    #[test]
    fn github_rejects_a_made_up_token() {
        if tool_path::find_on_path("gh").is_none() {
            return;
        }
        let check = check_token("github.com", "ghp_0000000000000000000000000000000000ab");
        assert!(
            matches!(check, TokenCheck::Rejected | TokenCheck::Unreachable(_)),
            "{check:?}"
        );
    }
}

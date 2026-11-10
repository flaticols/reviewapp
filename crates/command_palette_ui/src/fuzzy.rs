//! Ranks file paths against a typed query the way GoLand's Go to File does: the query's
//! letters in order anywhere in the path, file-name hits first.

/// The indices of `paths` that contain `query`'s letters in order, ignoring case, best
/// first. Ties keep the input order; an empty query keeps every path.
pub fn rank(query: &str, paths: &[impl AsRef<str>]) -> Vec<usize> {
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let mut scored: Vec<(usize, u32)> = paths
        .iter()
        .enumerate()
        .filter_map(|(ix, path)| Some((ix, score(&query, path.as_ref())?)))
        .collect();
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(ix, _)| ix).collect()
}

/// The better of matching from the path's start and from its file name's start.
fn score(query: &[char], path: &str) -> Option<u32> {
    let chars: Vec<char> = path.chars().collect();
    let name_start = chars.iter().rposition(|c| *c == '/').map_or(0, |ix| ix + 1);
    [0, name_start]
        .into_iter()
        .filter_map(|from| score_from(query, &chars, from, name_start))
        .max()
}

/// Matches each query letter to the next equal letter at or after `from`.
fn score_from(query: &[char], chars: &[char], from: usize, name_start: usize) -> Option<u32> {
    let mut total = 0;
    let mut next = from;
    let mut previous: Option<usize> = None;
    for wanted in query {
        let at = next
            + chars[next..]
                .iter()
                .position(|c| c.to_lowercase().eq(std::iter::once(*wanted)))?;
        total += 1;
        if previous.is_some_and(|p| p + 1 == at) {
            total += 5;
        }
        if starts_word(chars, at) {
            total += 8;
        }
        if at >= name_start {
            total += 10;
        }
        previous = Some(at);
        next = at + 1;
    }
    Some(total)
}

fn starts_word(chars: &[char], at: usize) -> bool {
    let Some(before) = at.checked_sub(1).map(|ix| chars[ix]) else {
        return true;
    };
    matches!(before, '/' | '_' | '-' | '.') || (before.is_lowercase() && chars[at].is_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATHS: [&str; 4] = [
        "internal/services/ocn/service_test.go",
        "internal/services/ocn/service.go",
        "cmd/core/config/config.go",
        "docs/wiki/tenant-configuration.md",
    ];

    #[test]
    fn subsequence_across_folders_matches() {
        assert!(rank("svcgo", &PATHS).contains(&1));
    }

    #[test]
    fn out_of_order_does_not_match() {
        assert!(!rank("gosvc", &PATHS).contains(&1));
    }

    #[test]
    fn case_is_ignored() {
        assert_eq!(rank("CONFIG.GO", &PATHS)[0], 2);
    }

    #[test]
    fn file_name_hit_beats_folder_hit() {
        assert_eq!(rank("config", &PATHS)[0], 2);
    }

    #[test]
    fn contiguous_beats_scattered() {
        assert_eq!(rank("service.go", &PATHS)[0], 1);
    }

    #[test]
    fn empty_query_keeps_order() {
        assert_eq!(rank("", &PATHS), vec![0, 1, 2, 3]);
    }
}

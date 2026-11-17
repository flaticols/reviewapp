//! What the merge queue does next in one repository, worked out from GitHub's answers
//! alone so it can be tested without a window or a network.

use std::collections::HashMap;

use pr_model::{CheckStatus, PullRequestKey, PullRequestState};
use review_model::{MergeMethod, MergeState, MergeStatus};

/// Why a queued pull request is passed over for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    Draft,
    Conflicts,
    CiFailed,
    CantUpdate,
    NotFound,
}

impl Skip {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Conflicts => "has conflicts",
            Self::CiFailed => "CI failed",
            Self::CantUpdate => "can’t update branch",
            Self::NotFound => "not found",
        }
    }
}

/// What the active pull request of a repository is going through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Rebasing,
    Merging,
    WaitingForCi,
    WaitingForApproval,
    AutoMergeOn,
    WaitingForGitHub,
}

impl Activity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rebasing => "Rebasing",
            Self::Merging => "Merging",
            Self::WaitingForCi => "Waiting for CI",
            Self::WaitingForApproval => "Waiting for approval",
            Self::AutoMergeOn => "Auto-merge on",
            Self::WaitingForGitHub => "Waiting for GitHub",
        }
    }
}

/// A queued pull request as the sidebar shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Queued,
    Skipped(Skip),
    Active(Activity),
}

impl RowState {
    pub fn label(self) -> String {
        match self {
            Self::Queued => "Queued".into(),
            Self::Skipped(skip) => format!("Skipped: {}", skip.label()),
            Self::Active(activity) => activity.label().into(),
        }
    }
}

/// A change the queue asks GitHub, or the queue itself, to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Remove {
        key: PullRequestKey,
        merged: bool,
    },
    DisableAutoMerge {
        key: PullRequestKey,
        node_id: String,
    },
    Rebase {
        key: PullRequestKey,
        node_id: String,
        head_sha: String,
    },
    Merge {
        key: PullRequestKey,
        method: MergeMethod,
        head_sha: String,
    },
    EnableAutoMerge {
        key: PullRequestKey,
        node_id: String,
        method: MergeMethod,
    },
}

impl Action {
    /// The pull request the action is about.
    pub fn key(&self) -> &PullRequestKey {
        match self {
            Self::Remove { key, .. }
            | Self::DisableAutoMerge { key, .. }
            | Self::Rebase { key, .. }
            | Self::Merge { key, .. }
            | Self::EnableAutoMerge { key, .. } => key,
        }
    }
}

/// One repository's next actions, the pull request now in flight and every remaining
/// row, in queue order.
#[derive(Debug)]
pub(crate) struct RepositoryPlan {
    pub actions: Vec<Action>,
    pub in_flight: Option<PullRequestKey>,
    pub rows: Vec<(PullRequestKey, RowState)>,
}

/// Plans one repository's `queue`, in order. `in_flight` keeps its turn while it can
/// still merge; `rebased_from` holds the head each pull request had before the queue
/// rebased it; `viewer` is whose auto-merge the queue may turn off.
pub(crate) fn plan_repository(
    queue: &[PullRequestKey],
    statuses: &HashMap<PullRequestKey, MergeStatus>,
    in_flight: Option<&PullRequestKey>,
    rebased_from: &HashMap<PullRequestKey, String>,
    viewer: &str,
) -> RepositoryPlan {
    let mut actions = Vec::new();
    let mut live = Vec::new();
    for key in queue {
        let status = statuses.get(key);
        match status.map(|status| status.state) {
            Some(PullRequestState::Merged) => actions.push(Action::Remove {
                key: key.clone(),
                merged: true,
            }),
            Some(PullRequestState::Closed) => actions.push(Action::Remove {
                key: key.clone(),
                merged: false,
            }),
            _ => live.push((key, status, skip(status))),
        }
    }
    let can_go = |candidate: &PullRequestKey| {
        live.iter()
            .any(|(key, _, skip)| *key == candidate && skip.is_none())
    };
    let active = in_flight
        .filter(|key| can_go(key))
        .or_else(|| live.iter().find(|(_, _, skip)| skip.is_none()).map(|(key, ..)| *key))
        .cloned();

    for (key, status, _) in &live {
        if let Some(status) = status
            && Some(*key) != active.as_ref()
            && status
                .merge
                .auto_merge
                .as_ref()
                .is_some_and(|auto| auto.enabled_by == viewer)
        {
            actions.push(Action::DisableAutoMerge {
                key: (*key).clone(),
                node_id: status.merge.node_id.clone(),
            });
        }
    }

    let mut rows = Vec::new();
    for (key, status, skip) in live {
        let row = match (skip, status) {
            (Some(skip), _) => RowState::Skipped(skip),
            (None, Some(status)) if Some(key) == active.as_ref() => {
                let (activity, action) = step(status, rebased_from.get(key));
                actions.extend(action);
                RowState::Active(activity)
            }
            _ => RowState::Queued,
        };
        rows.push((key.clone(), row));
    }
    RepositoryPlan {
        actions,
        in_flight: active,
        rows,
    }
}

fn skip(status: Option<&MergeStatus>) -> Option<Skip> {
    let Some(status) = status else {
        return Some(Skip::NotFound);
    };
    let merge = &status.merge;
    if status.state == PullRequestState::Draft || merge.state == MergeState::Draft {
        Some(Skip::Draft)
    } else if merge.state == MergeState::Dirty {
        Some(Skip::Conflicts)
    } else if status.required_checks == CheckStatus::Failed {
        Some(Skip::CiFailed)
    } else if merge.state == MergeState::Behind && !merge.can_update_branch {
        Some(Skip::CantUpdate)
    } else {
        None
    }
}

/// The one step to take on the active pull request, and what it's going through.
fn step(status: &MergeStatus, rebased_from: Option<&String>) -> (Activity, Option<Action>) {
    let merge = &status.merge;
    let key = status.key.clone();
    let waiting = if status.needs_review {
        Activity::WaitingForApproval
    } else {
        Activity::WaitingForCi
    };
    match (merge.state, merge.default_method) {
        (MergeState::Behind, _) if rebased_from == Some(&merge.head_sha) => {
            (Activity::WaitingForGitHub, None)
        }
        (MergeState::Behind, _) => (
            Activity::Rebasing,
            Some(Action::Rebase {
                key,
                node_id: merge.node_id.clone(),
                head_sha: merge.head_sha.clone(),
            }),
        ),
        _ if merge.auto_merge.is_some() => {
            let checks_in = !status.needs_review && status.required_checks == CheckStatus::Passed;
            (if checks_in { Activity::AutoMergeOn } else { waiting }, None)
        }
        (state, Some(method)) if state.can_merge() => (
            Activity::Merging,
            Some(Action::Merge {
                key,
                method,
                head_sha: merge.head_sha.clone(),
            }),
        ),
        (MergeState::Blocked, Some(method)) if merge.can_enable_auto_merge => (
            waiting,
            Some(Action::EnableAutoMerge {
                key,
                node_id: merge.node_id.clone(),
                method,
            }),
        ),
        (MergeState::Blocked, _) => (waiting, None),
        _ => (Activity::WaitingForGitHub, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pr_model::RepoSlug;
    use review_model::{AutoMerge, MergeInfo};

    use MergeState::{Behind, Blocked, Clean, Dirty, Unknown, Unstable};
    use PullRequestState::{Closed, Draft, Merged, Open};

    fn key(n: u64) -> PullRequestKey {
        PullRequestKey::new(RepoSlug::new("acme", "app"), n)
    }

    fn status(n: u64, state: PullRequestState, merge_state: MergeState) -> MergeStatus {
        let mut merge = MergeInfo::new(
            format!("N{n}"),
            format!("h{n}"),
            merge_state,
            vec![MergeMethod::Squash],
        );
        merge.can_update_branch = true;
        merge.can_enable_auto_merge = true;
        MergeStatus::new(key(n), state, merge)
    }

    fn open(n: u64, merge_state: MergeState) -> MergeStatus {
        status(n, Open, merge_state)
    }

    fn with_checks(mut status: MergeStatus, checks: CheckStatus) -> MergeStatus {
        status.required_checks = checks;
        status
    }

    fn with_auto_merge(mut status: MergeStatus, by: &str) -> MergeStatus {
        status.merge.auto_merge = Some(AutoMerge::new(MergeMethod::Squash, by));
        status
    }

    fn plan_with(
        queue: &[u64],
        statuses: Vec<MergeStatus>,
        in_flight: Option<u64>,
        rebased_from: &[(u64, &str)],
    ) -> RepositoryPlan {
        let queue: Vec<PullRequestKey> = queue.iter().map(|n| key(*n)).collect();
        let statuses = statuses.into_iter().map(|s| (s.key.clone(), s)).collect();
        let rebased_from = rebased_from
            .iter()
            .map(|(n, sha)| (key(*n), sha.to_string()))
            .collect();
        plan_repository(
            &queue,
            &statuses,
            in_flight.map(key).as_ref(),
            &rebased_from,
            "me",
        )
    }

    fn plan(queue: &[u64], statuses: Vec<MergeStatus>) -> RepositoryPlan {
        plan_with(queue, statuses, None, &[])
    }

    fn rebase(n: u64) -> Action {
        Action::Rebase {
            key: key(n),
            node_id: format!("N{n}"),
            head_sha: format!("h{n}"),
        }
    }

    fn merge(n: u64) -> Action {
        Action::Merge {
            key: key(n),
            method: MergeMethod::Squash,
            head_sha: format!("h{n}"),
        }
    }

    fn disable(n: u64) -> Action {
        Action::DisableAutoMerge {
            key: key(n),
            node_id: format!("N{n}"),
        }
    }

    fn rows(plan: &RepositoryPlan) -> Vec<RowState> {
        plan.rows.iter().map(|(_, row)| *row).collect()
    }

    #[test]
    fn merged_head_is_removed_and_next_is_worked() {
        let plan = plan(&[1, 2], vec![status(1, Merged, Unknown), open(2, Behind)]);
        assert_eq!(
            plan.actions,
            [Action::Remove { key: key(1), merged: true }, rebase(2)]
        );
        assert_eq!(rows(&plan), [RowState::Active(Activity::Rebasing)]);
    }

    #[test]
    fn closed_head_is_removed_unmerged() {
        let plan = plan(&[1], vec![status(1, Closed, Unknown)]);
        assert_eq!(plan.actions, [Action::Remove { key: key(1), merged: false }]);
    }

    #[test]
    fn behind_head_is_rebased() {
        let plan = plan(&[1], vec![open(1, Behind)]);
        assert_eq!(plan.actions, [rebase(1)]);
    }

    #[test]
    fn behind_after_own_rebase_waits() {
        let plan = plan_with(&[1], vec![open(1, Behind)], Some(1), &[(1, "h1")]);
        assert_eq!(plan.actions, []);
        assert_eq!(rows(&plan), [RowState::Active(Activity::WaitingForGitHub)]);
    }

    #[test]
    fn behind_after_a_new_push_is_rebased_again() {
        let plan = plan_with(&[1], vec![open(1, Behind)], Some(1), &[(1, "h0")]);
        assert_eq!(plan.actions, [rebase(1)]);
    }

    #[test]
    fn clean_head_merges_with_default_method() {
        let plan = plan(&[1], vec![open(1, Clean)]);
        assert_eq!(plan.actions, [merge(1)]);
        assert_eq!(rows(&plan), [RowState::Active(Activity::Merging)]);
    }

    #[test]
    fn unstable_head_merges() {
        let plan = plan(&[1], vec![open(1, Unstable)]);
        assert_eq!(plan.actions, [merge(1)]);
    }

    #[test]
    fn blocked_head_turns_on_auto_merge() {
        let plan = plan(&[1], vec![with_checks(open(1, Blocked), CheckStatus::Running)]);
        assert_eq!(
            plan.actions,
            [Action::EnableAutoMerge {
                key: key(1),
                node_id: "N1".into(),
                method: MergeMethod::Squash
            }]
        );
        assert_eq!(rows(&plan), [RowState::Active(Activity::WaitingForCi)]);
    }

    #[test]
    fn auto_merge_on_waits_for_approval_when_review_is_needed() {
        let mut head = with_auto_merge(open(1, Blocked), "me");
        head.needs_review = true;
        let plan = plan(&[1], vec![head]);
        assert_eq!(plan.actions, []);
        assert_eq!(rows(&plan), [RowState::Active(Activity::WaitingForApproval)]);
    }

    #[test]
    fn auto_merge_on_with_checks_in_shows_auto_merge_on() {
        let plan = plan(&[1], vec![with_auto_merge(open(1, Blocked), "me")]);
        assert_eq!(rows(&plan), [RowState::Active(Activity::AutoMergeOn)]);
    }

    #[test]
    fn failed_ci_skips_to_next() {
        let plan = plan(
            &[1, 2],
            vec![with_checks(open(1, Blocked), CheckStatus::Failed), open(2, Behind)],
        );
        assert_eq!(plan.actions, [rebase(2)]);
        assert_eq!(
            rows(&plan),
            [
                RowState::Skipped(Skip::CiFailed),
                RowState::Active(Activity::Rebasing)
            ]
        );
        assert_eq!(plan.in_flight, Some(key(2)));
    }

    #[test]
    fn conflicts_draft_and_cant_update_are_skipped() {
        let mut stuck = open(3, Behind);
        stuck.merge.can_update_branch = false;
        let plan = plan(
            &[1, 2, 3, 4],
            vec![open(1, Dirty), status(2, Draft, Blocked), stuck, open(4, Clean)],
        );
        assert_eq!(plan.actions, [merge(4)]);
        assert_eq!(
            rows(&plan),
            [
                RowState::Skipped(Skip::Conflicts),
                RowState::Skipped(Skip::Draft),
                RowState::Skipped(Skip::CantUpdate),
                RowState::Active(Activity::Merging)
            ]
        );
    }

    #[test]
    fn missing_status_is_not_found_and_queue_continues() {
        let plan = plan(&[1, 2], vec![open(2, Clean)]);
        assert_eq!(plan.actions, [merge(2)]);
        assert_eq!(rows(&plan)[0], RowState::Skipped(Skip::NotFound));
    }

    #[test]
    fn in_flight_keeps_its_turn_over_a_recovered_earlier_pr() {
        let plan = plan_with(
            &[1, 2],
            vec![
                with_checks(open(1, Blocked), CheckStatus::Running),
                with_auto_merge(open(2, Blocked), "me"),
            ],
            Some(2),
            &[],
        );
        assert_eq!(plan.in_flight, Some(key(2)));
        assert_eq!(rows(&plan)[0], RowState::Queued);
        assert_eq!(plan.actions, []);
    }

    #[test]
    fn skipped_in_flight_loses_viewer_auto_merge() {
        let failed = with_checks(with_auto_merge(open(1, Blocked), "me"), CheckStatus::Failed);
        let plan = plan_with(&[1], vec![failed], Some(1), &[]);
        assert_eq!(plan.actions, [disable(1)]);
    }

    #[test]
    fn non_active_viewer_auto_merge_is_turned_off() {
        let plan = plan(
            &[1, 2],
            vec![open(1, Behind), with_auto_merge(open(2, Blocked), "me")],
        );
        assert_eq!(plan.actions, [disable(2), rebase(1)]);
    }

    #[test]
    fn authors_auto_merge_is_left_alone() {
        let plan = plan(
            &[1, 2],
            vec![open(1, Behind), with_auto_merge(open(2, Blocked), "author")],
        );
        assert_eq!(plan.actions, [rebase(1)]);
    }

    #[test]
    fn unknown_state_waits() {
        let plan = plan(&[1], vec![open(1, Unknown)]);
        assert_eq!(plan.actions, []);
        assert_eq!(rows(&plan), [RowState::Active(Activity::WaitingForGitHub)]);
    }

    #[test]
    fn all_skipped_has_nothing_in_flight() {
        let plan = plan(&[1], vec![open(1, Dirty)]);
        assert_eq!(plan.in_flight, None);
    }

    #[test]
    fn row_labels_read_as_the_sidebar_shows_them() {
        assert_eq!(RowState::Skipped(Skip::CiFailed).label(), "Skipped: CI failed");
        assert_eq!(RowState::Active(Activity::WaitingForCi).label(), "Waiting for CI");
        assert_eq!(RowState::Queued.label(), "Queued");
    }
}

//! The merge queue runner against a scripted GitHub: what it asks GitHub to do, what it
//! takes out of the queue and what it tells the user.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use app_state_ui::{Activity, MergeQueue, MergeQueueEvent, RowState, SettingsStore, Skip};
use github_api::FixtureSource;
use gpui_kit::{AppContext as _, Entity, TestAppContext};
use pr_model::{CheckStatus, PullRequestKey, PullRequestState, RepoSlug, Timestamp};
use review_model::{AutoMerge, MergeInfo, MergeMethod, MergeState, MergeStatus};

/// The sample settings' signed-in viewer.
const VIEWER: &str = "flaticols";

fn key(n: u64) -> PullRequestKey {
    PullRequestKey::new(RepoSlug::new("acme", "app"), n)
}

fn status(n: u64, merge_state: MergeState) -> MergeStatus {
    let mut merge = MergeInfo::new(
        format!("N{n}"),
        format!("h{n}"),
        merge_state,
        vec![MergeMethod::Squash],
    );
    merge.can_update_branch = true;
    merge.can_enable_auto_merge = true;
    MergeStatus::new(key(n), PullRequestState::Open, merge)
}

fn running(mut status: MergeStatus) -> MergeStatus {
    status.required_checks = CheckStatus::Running;
    status
}

fn with_viewer_auto_merge(mut status: MergeStatus) -> MergeStatus {
    status.merge.auto_merge = Some(AutoMerge::new(MergeMethod::Squash, VIEWER));
    status
}

struct Harness {
    source: Arc<FixtureSource>,
    settings: Entity<SettingsStore>,
    queue: Entity<MergeQueue>,
    events: Rc<RefCell<Vec<MergeQueueEvent>>>,
}

impl Harness {
    fn new(queued: &[u64], statuses: Vec<MergeStatus>, cx: &mut TestAppContext) -> Self {
        let source = Arc::new(FixtureSource::new(Vec::new(), Vec::new()));
        for status in statuses {
            source.set_merge_status(status);
        }
        let mut file =
            fixtures::sample(Timestamp::now(), std::path::Path::new("/Users/test")).settings;
        file.preferences.merge_queue = queued.iter().map(|n| key(*n).id()).collect();
        let settings =
            cx.update(|cx| cx.new(|cx| SettingsStore::new(file, None, None, Vec::new(), cx)));
        let dyn_source: Arc<dyn github_api::PullRequestSource> = source.clone();
        let queue = cx.update(|cx| cx.new(|cx| MergeQueue::new(dyn_source, settings.clone(), cx)));
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        cx.update(|cx| {
            cx.subscribe(&queue, move |_, event: &MergeQueueEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
            .detach()
        });
        Self {
            source,
            settings,
            queue,
            events,
        }
    }

    fn tick(&self, cx: &mut TestAppContext) {
        self.queue.update(cx, |queue, cx| queue.tick(cx));
        cx.run_until_parked();
    }

    fn queued(&self, cx: &mut TestAppContext) -> Vec<String> {
        self.settings
            .read_with(cx, |settings, _| settings.preferences().merge_queue.clone())
    }

    fn row(&self, n: u64, cx: &mut TestAppContext) -> RowState {
        self.queue.read_with(cx, |queue, _| queue.row(&key(n)))
    }

    fn count(&self, action: &str) -> usize {
        self.source
            .actions()
            .iter()
            .filter(|done| done.as_str() == action)
            .count()
    }
}

#[gpui_kit::test]
fn behind_head_is_rebased(cx: &mut TestAppContext) {
    let harness = Harness::new(&[1], vec![status(1, MergeState::Behind)], cx);
    harness.tick(cx);
    assert_eq!(harness.source.actions(), ["update-branch N1 rebase"]);
    assert_eq!(harness.row(1, cx), RowState::Active(Activity::Rebasing));
}

#[gpui_kit::test]
fn merged_head_leaves_the_queue_and_the_next_is_merged(cx: &mut TestAppContext) {
    let mut merged = status(1, MergeState::Unknown);
    merged.state = PullRequestState::Merged;
    let harness = Harness::new(&[1, 2], vec![merged, status(2, MergeState::Clean)], cx);
    harness.tick(cx);
    assert_eq!(harness.queued(cx), [key(2).id()]);
    assert_eq!(*harness.events.borrow(), [MergeQueueEvent::Merged(key(1))]);
    assert_eq!(harness.count(&format!("merge {:?} squash", key(2))), 1);
}

#[gpui_kit::test]
fn poll_ticks_every_30_seconds(cx: &mut TestAppContext) {
    let harness = Harness::new(&[1], vec![status(1, MergeState::Behind)], cx);
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    assert_eq!(harness.count("update-branch N1 rebase"), 1);
}

#[gpui_kit::test]
fn fetch_failure_keeps_rows(cx: &mut TestAppContext) {
    let harness = Harness::new(&[1], vec![status(1, MergeState::Dirty)], cx);
    harness.tick(cx);
    harness.source.fail_merge_statuses(true);
    harness
        .source
        .set_merge_status(status(1, MergeState::Clean));
    harness.tick(cx);
    assert_eq!(harness.row(1, cx), RowState::Skipped(Skip::Conflicts));
    assert!(harness.source.actions().is_empty());
    assert_eq!(harness.events.borrow().len(), 1);
}

#[gpui_kit::test]
fn skip_notifies_once(cx: &mut TestAppContext) {
    let harness = Harness::new(&[1], vec![status(1, MergeState::Dirty)], cx);
    harness.tick(cx);
    harness.tick(cx);
    assert_eq!(
        *harness.events.borrow(),
        [MergeQueueEvent::Skipped(key(1), Skip::Conflicts)]
    );
}

#[gpui_kit::test]
fn moving_up_takes_over_in_flight(cx: &mut TestAppContext) {
    let harness = Harness::new(
        &[1, 2],
        vec![
            running(status(1, MergeState::Blocked)),
            running(status(2, MergeState::Blocked)),
        ],
        cx,
    );
    harness.tick(cx);
    assert_eq!(harness.count("auto-merge N1 squash"), 1);
    harness
        .source
        .set_merge_status(with_viewer_auto_merge(running(status(
            1,
            MergeState::Blocked,
        ))));
    harness
        .queue
        .update(cx, |queue, cx| queue.move_by(&key(2), true, cx));
    cx.run_until_parked();
    assert_eq!(harness.count("auto-merge N1 off"), 1);
    assert_eq!(harness.count("auto-merge N2 squash"), 1);
    assert_eq!(harness.row(2, cx), RowState::Active(Activity::WaitingForCi));
}

#[gpui_kit::test]
fn overlapping_ticks_rebase_once(cx: &mut TestAppContext) {
    let harness = Harness::new(&[1], vec![status(1, MergeState::Behind)], cx);
    harness.queue.update(cx, |queue, cx| {
        queue.tick(cx);
        queue.tick(cx);
    });
    cx.run_until_parked();
    assert_eq!(harness.count("update-branch N1 rebase"), 1);
}

#[gpui_kit::test]
fn removing_turns_off_viewer_auto_merge(cx: &mut TestAppContext) {
    let harness = Harness::new(
        &[1],
        vec![with_viewer_auto_merge(running(status(
            1,
            MergeState::Blocked,
        )))],
        cx,
    );
    harness.tick(cx);
    harness
        .queue
        .update(cx, |queue, cx| queue.remove(&key(1), cx));
    cx.run_until_parked();
    assert_eq!(harness.count("auto-merge N1 off"), 1);
    assert!(harness.queued(cx).is_empty());
}

#[gpui_kit::test]
fn adding_queues_and_works_it(cx: &mut TestAppContext) {
    let harness = Harness::new(&[], vec![status(1, MergeState::Behind)], cx);
    harness.queue.update(cx, |queue, cx| queue.add(key(1), cx));
    cx.run_until_parked();
    assert_eq!(harness.queued(cx), [key(1).id()]);
    assert_eq!(harness.count("update-branch N1 rebase"), 1);
}

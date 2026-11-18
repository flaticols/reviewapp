//! The pull request list keeps itself current: it syncs every 5 minutes, and when the
//! window comes back to the front a minute or more after the last sync.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use github_api::FixtureSource;
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext, VisualTestContext, px, size};
use pr_model::{PullRequestKey, RepoSlug, Timestamp};
use workspace_ui::{Workspace, launch};

/// The list row of the pull request GitHub stops returning.
const ROW: &str = "flaticols/switchfold#482";

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Arc<FixtureSource>) {
    let home = PathBuf::from("/Users/test");
    let sample = fixtures::sample(Timestamp::now(), &home);
    let source = Arc::new(FixtureSource::new(sample.pull_requests, sample.details));
    let mut data = launch::sample_data(&home);
    data.source = source.clone();
    cx.update(|cx| {
        gpui_kit::init(cx);
        pr_list_ui::init(cx);
        review_thread_ui::init(cx);
        workspace_ui::init(cx);
        launch::install_stores(data, Some(home), cx);
        theme_ui::apply(&settings_model::Preferences::default(), None, cx);
    });
    let handle = cx.open_window(size(px(1440.), px(900.)), |window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    (handle.into(), source)
}

/// GitHub stops returning #482, as when it's closed.
fn close_on_github(source: &FixtureSource, cx: &mut TestAppContext) {
    let closed = PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482);
    let mut pull_requests =
        fixtures::sample(Timestamp::now(), &PathBuf::from("/Users/test")).pull_requests;
    pull_requests.retain(|pr| pr.key != closed);
    source.set_pull_requests(pull_requests);
    cx.run_until_parked();
}

fn row_shown(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(ROW).is_some_and(|row| row.visible())
    })
    .unwrap()
}

fn leave_and_come_back(window: AnyWindowHandle, away: Duration, cx: &mut TestAppContext) {
    VisualTestContext::from_window(window, cx).deactivate_window();
    cx.executor().advance_clock(away);
    cx.update_window(window, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_list_syncs_every_five_minutes(cx: &mut TestAppContext) {
    let (window, source) = open(cx);
    assert!(row_shown(window, cx), "the sample lists #482");
    close_on_github(&source, cx);
    cx.executor().advance_clock(Duration::from_secs(5 * 60));
    cx.run_until_parked();
    assert!(
        !row_shown(window, cx),
        "five minutes on, the list no longer shows what GitHub dropped"
    );
}

#[gpui_kit::test]
fn coming_back_after_a_minute_syncs_the_list(cx: &mut TestAppContext) {
    let (window, source) = open(cx);
    close_on_github(&source, cx);
    leave_and_come_back(window, Duration::from_secs(61), cx);
    assert!(
        !row_shown(window, cx),
        "coming back after a minute syncs the list"
    );
}

#[gpui_kit::test]
fn coming_back_within_a_minute_keeps_the_list(cx: &mut TestAppContext) {
    let (window, source) = open(cx);
    close_on_github(&source, cx);
    leave_and_come_back(window, Duration::from_secs(30), cx);
    assert!(
        row_shown(window, cx),
        "a quick switch away and back doesn't sync again"
    );
}

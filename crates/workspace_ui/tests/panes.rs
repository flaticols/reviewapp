//! Split panes in the review screen: one pane's own state, then splitting, focus and
//! the layout each pull request keeps.

use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, px, size};
use pr_model::{PullRequestKey, RepoSlug};
use review_thread_ui::{
    PaneState, ReviewLayout, ReviewPanes, ReviewThreadView, ScrollAnchor, SplitAxis,
    TimelineFilter,
};
use workspace_ui::launch;

/// Not the first file, so landing on it can't happen by accident.
const RENDER: &str = "crates/review/src/render.rs";
const HUNK: &str = "crates/review/src/hunk.rs";

fn featured() -> PullRequestKey {
    PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482)
}

fn install(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        review_thread_ui::init(cx);
        workspace_ui::init(cx);
        let home = std::path::PathBuf::from("/Users/test");
        launch::install_stores(launch::sample_data(&home), Some(home), cx);
        let mut look = settings_model::Preferences::default();
        look.appearance = settings_model::Appearance::Dark;
        theme_ui::apply(&look, None, cx);
    });
}

fn open_pane(
    state: PaneState,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<ReviewThreadView>) {
    install(cx);
    let mut pane = None;
    let handle = cx.open_window(size(px(1440.), px(900.)), |window, cx| {
        let view = cx.new(|cx| {
            ReviewThreadView::new_pane(featured(), "Review requests".into(), state, true, window, cx)
        });
        pane = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    let window: AnyWindowHandle = handle.into();
    render(window, cx);
    (window, pane.expect("pane was built"))
}

fn render(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
}

fn changes_at(file: &str) -> PaneState {
    PaneState {
        filter: TimelineFilter::Changes,
        outline: true,
        scroll: ScrollAnchor {
            file: Some(file.into()),
            row: 0,
            offset: 0.,
        },
    }
}

#[gpui_kit::test]
fn a_pane_reports_where_it_is_scrolled(cx: &mut TestAppContext) {
    let (window, pane) = open_pane(PaneState::default(), cx);
    pane.update(cx, |pane, cx| pane.reveal_file(RENDER, cx));
    render(window, cx);
    let scroll = pane.read_with(cx, |pane, _| pane.pane_state().scroll);
    assert_eq!(scroll.file.as_deref(), Some(RENDER));
}

#[gpui_kit::test]
fn a_pane_opens_where_its_state_says(cx: &mut TestAppContext) {
    let (_, pane) = open_pane(changes_at(RENDER), cx);
    let scroll = pane.read_with(cx, |pane, _| pane.pane_state().scroll);
    assert_eq!(scroll.file.as_deref(), Some(RENDER));
}

#[gpui_kit::test]
fn a_missing_anchor_file_scrolls_to_the_top(cx: &mut TestAppContext) {
    let mut state = changes_at("gone.rs");
    state.filter = TimelineFilter::Everything;
    let (_, pane) = open_pane(state, cx);
    let scroll = pane.read_with(cx, |pane, _| pane.pane_state().scroll);
    assert_eq!(scroll, ScrollAnchor::default());
}

fn open_panes(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ReviewPanes>) {
    install(cx);
    new_panes(cx)
}

/// Another review screen for the featured pull request, in the app already set up.
fn new_panes(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ReviewPanes>) {
    let mut panes = None;
    let handle = cx.open_window(size(px(1440.), px(900.)), |window, cx| {
        let view = cx.new(|cx| ReviewPanes::new(featured(), "Review requests", window, cx));
        panes = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    let window: AnyWindowHandle = handle.into();
    render(window, cx);
    (window, panes.expect("panes were built"))
}

fn press(window: AnyWindowHandle, keys: &[&str], cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        for key in keys {
            window.press(key, cx);
        }
    })
    .unwrap();
    cx.run_until_parked();
    render(window, cx);
}

fn count(panes: &Entity<ReviewPanes>, cx: &mut TestAppContext) -> usize {
    panes.read_with(cx, |panes, _| panes.pane_count())
}

/// Each pane's file tree column, measured in the last frame.
fn outline_heights(
    window: AnyWindowHandle,
    panes: &Entity<ReviewPanes>,
    cx: &mut TestAppContext,
) -> Vec<f32> {
    let ids: Vec<u64> = panes.read_with(cx, |panes, _| {
        panes.panes().iter().map(|pane| pane.entity_id().as_u64()).collect()
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        ids.iter()
            .map(|id| {
                window
                    .within(("review-pane", *id))
                    .find("outline")
                    .bounds()
                    .size
                    .height
                    .as_f32()
            })
            .collect()
    })
    .unwrap()
}

/// The window is 900 high: a side-by-side pane fills it, stacked ones share it.
#[gpui_kit::test]
fn every_pane_fills_its_share_of_the_window(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["ctrl-b", "%", "ctrl-b", "\""], cx);
    let heights = outline_heights(window, &panes, cx);
    assert!(
        heights.iter().all(|height| *height > 400.),
        "pane heights: {heights:?}"
    );
}

#[gpui_kit::test]
fn prefix_percent_splits_side_by_side(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["ctrl-b", "%"], cx);
    assert_eq!(count(&panes, cx), 2);
}

#[gpui_kit::test]
fn a_split_focuses_the_new_pane(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["ctrl-b", "\""], cx);
    assert_eq!(panes.read_with(cx, |panes, _| panes.focused_index()), 1);
}

#[gpui_kit::test]
fn prefix_x_closes_the_focused_pane(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["ctrl-b", "%", "ctrl-b", "x"], cx);
    assert_eq!(count(&panes, cx), 1);
}

#[gpui_kit::test]
fn prefix_x_on_the_only_pane_does_nothing(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["ctrl-b", "x"], cx);
    assert_eq!(count(&panes, cx), 1);
}

#[gpui_kit::test]
fn percent_without_the_prefix_does_not_split(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["%"], cx);
    assert_eq!(count(&panes, cx), 1);
}

#[gpui_kit::test]
fn prefix_arrow_moves_focus_across_a_split(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["ctrl-b", "%", "ctrl-b", "left"], cx);
    assert_eq!(panes.read_with(cx, |panes, _| panes.focused_index()), 0);
}

#[gpui_kit::test]
fn panes_scroll_independently(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["ctrl-b", "%"], cx);
    let [left, right] = panes.read_with(cx, |panes, _| {
        let all = panes.panes();
        [all[0].clone(), all[1].clone()]
    });
    left.update(cx, |pane, cx| pane.reveal_file(RENDER, cx));
    right.update(cx, |pane, cx| pane.reveal_file(HUNK, cx));
    render(window, cx);
    let files = [&left, &right].map(|pane| pane.read_with(cx, |pane, _| pane.pane_state().scroll.file));
    assert_eq!(files, [Some(RENDER.into()), Some(HUNK.into())]);
}

#[gpui_kit::test]
fn reopening_the_pull_request_restores_its_panes(cx: &mut TestAppContext) {
    let (window, _) = open_panes(cx);
    press(window, &["ctrl-b", "%", "ctrl-b", "\""], cx);
    let (_, reopened) = new_panes(cx);
    assert_eq!(count(&reopened, cx), 3);
}

#[gpui_kit::test]
fn a_late_saved_layout_does_not_replace_a_split_made_meanwhile(cx: &mut TestAppContext) {
    let (window, _) = open_panes(cx);
    press(window, &["ctrl-b", "%", "ctrl-b", "\""], cx);
    let mut reopened = None;
    cx.open_window(size(px(1440.), px(900.)), |window, cx| {
        let view = cx.new(|cx| ReviewPanes::new(featured(), "Review requests", window, cx));
        view.update(cx, |panes, cx| panes.split(SplitAxis::SideBySide, window, cx));
        reopened = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(count(&reopened.expect("panes were built"), cx), 2);
}

#[gpui_kit::test]
fn closing_the_diffing_pane_hands_the_duty_on(cx: &mut TestAppContext) {
    let (window, panes) = open_panes(cx);
    press(window, &["ctrl-b", "%", "ctrl-b", "left", "ctrl-b", "x"], cx);
    let diffs = panes.read_with(cx, |panes, cx| panes.panes()[0].read(cx).diffs_locally());
    assert!(diffs);
}

#[gpui_kit::test]
fn hiding_one_panes_tree_hides_it_for_that_pane(cx: &mut TestAppContext) {
    let (_, pane) = open_pane(PaneState::default(), cx);
    pane.update(cx, |pane, cx| pane.toggle_outline(cx));
    let visible = cx.update(|cx| pane.read(cx).outline_visible(cx));
    assert!(!visible);
}

#[gpui_kit::test]
fn hiding_one_panes_tree_leaves_the_setting_for_new_panes_alone(cx: &mut TestAppContext) {
    let (_, pane) = open_pane(PaneState::default(), cx);
    pane.update(cx, |pane, cx| pane.toggle_outline(cx));
    assert!(cx.update(|cx| ReviewLayout::get(cx).outline));
}


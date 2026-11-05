//! Split panes in the review screen: one pane's own state, then splitting, focus and
//! the layout each pull request keeps.

use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, px, size};
use pr_model::{PullRequestKey, RepoSlug};
use review_thread_ui::{
    PaneState, ReviewLayout, ReviewThreadView, ScrollAnchor, TimelineFilter,
};
use workspace_ui::launch;

/// Not the first file, so landing on it can't happen by accident.
const RENDER: &str = "crates/review/src/render.rs";

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

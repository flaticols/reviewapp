//! Frame cost of scrolling a large pull request. Ignored by default; run with
//! `cargo test -p workspace_ui --release --test scroll_perf -- --ignored --nocapture`.

use std::sync::Arc;
use std::time::Instant;

use diff_model::{DiffLine, FileChange, FileDiff, Hunk, LineKind};
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AppContext as _, InputEvent as _, ScrollDelta, ScrollWheelEvent, TestAppContext, point, px,
    size,
};
use pr_model::{PullRequestKey, RepoSlug, Timestamp};
use review_model::TimelineItem;
use workspace_ui::{Workspace, launch};

const FILES: usize = 90;
const HUNKS: usize = 4;
const LINES: usize = 40;

fn large_file(ix: usize) -> FileDiff {
    let hunks = (0..HUNKS)
        .map(|h| {
            let start = (h * 100 + 1) as u32;
            let lines = (0..LINES)
                .map(|l| {
                    let n = start + l as u32;
                    let text =
                        format!("    let value_{l} = compute(&input, {n}).unwrap_or_default();");
                    match l % 10 {
                        3 => DiffLine::new(LineKind::Removed, Some(n), None, text),
                        4 => DiffLine::new(LineKind::Added, None, Some(n), text),
                        _ => DiffLine::new(LineKind::Context, Some(n), Some(n), text),
                    }
                })
                .collect();
            Hunk::new(
                start,
                LINES as u32,
                start,
                LINES as u32,
                "fn compute()",
                lines,
            )
        })
        .collect();
    FileDiff::new(
        format!("src/module_{ix}/file_{ix}.rs"),
        FileChange::Modified,
        hunks,
    )
}

#[gpui_kit::test]
#[ignore = "benchmark"]
fn scrolling_a_large_pull_request(cx: &mut TestAppContext) {
    let home = std::path::PathBuf::from("/Users/test");
    let key = PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482);
    let mut sample = fixtures::sample(Timestamp::now(), &home);
    let detail = sample
        .details
        .iter_mut()
        .find(|detail| detail.summary.key == key)
        .unwrap();
    detail.files = (0..FILES).map(large_file).collect();
    detail.timeline = std::iter::once(TimelineItem::Description)
        .chain(detail.files.iter().map(|file| TimelineItem::File {
            path: file.path.clone(),
        }))
        .collect();
    let data = launch::StartingData {
        source: Arc::new(github_api::FixtureSource::new(
            sample.pull_requests,
            sample.details,
        )),
        saved_filters: sample.saved_filters,
        settings: sample.settings,
        settings_path: None,
        clone_statuses: sample.clone_statuses,
        cache: None,
    };
    cx.update(|cx| {
        gpui_kit::init(cx);
        pr_list_ui::init(cx);
        review_thread_ui::init(cx);
        workspace_ui::init(cx);
        launch::install_stores(data, Some(home), cx);
        let mut look = settings_model::Preferences::default();
        look.appearance = settings_model::Appearance::Dark;
        theme_ui::apply(&look, None, cx);
    });
    let mut workspace = None;
    let window = cx.open_window(size(px(1440.), px(900.)), |window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    let workspace = workspace.unwrap();
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_pull_request(key, window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();

    let started = Instant::now();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    eprintln!("first frame: {:.1?}", started.elapsed());

    let frames = 60;
    let started = Instant::now();
    for _ in 0..frames {
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(
                ScrollWheelEvent {
                    position: point(px(700.), px(500.)),
                    delta: ScrollDelta::Pixels(point(px(0.), px(-120.))),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
    eprintln!(
        "{} lines: {:.2?} per scroll frame",
        FILES * HUNKS * LINES,
        started.elapsed() / frames
    );
}

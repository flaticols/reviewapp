//! UI integration tests: the real workspace with the sample data, driven by native
//! pointer and keyboard events.

use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, px, size};
use pr_model::{PullRequestKey, RepoSlug};
use review_model::{ReviewVerdict, ReviewerState, TimelineItem};
use workspace_ui::{Workspace, launch};

fn featured() -> PullRequestKey {
    PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482)
}

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Workspace>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        pr_list_ui::init(cx);
        review_thread_ui::init(cx);
        workspace_ui::init(cx);
        let home = std::path::PathBuf::from("/Users/test");
        launch::install_stores(launch::sample_data(&home), Some(home), cx);
        let mut look = settings_model::Preferences::default();
        look.appearance = settings_model::Appearance::Dark;
        theme_ui::apply(&look, None, cx);
    });
    let mut workspace = None;
    let handle = cx.open_window(size(px(1440.), px(900.)), |window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    cx.run_until_parked();
    (handle.into(), workspace.expect("workspace was built"))
}

fn open_featured(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Workspace>) {
    let (window, workspace) = open(cx);
    cx.update_window(window, |_, window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.open_pull_request(featured(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    (window, workspace)
}

/// Waits for overlay open animations, which run on wall-clock time, to bring `id` to
/// rest, so pointer events land where the control will stay.
fn settle(window: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
    let mut last = None;
    for _ in 0..40 {
        cx.run_until_parked();
        let bounds = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.try_find(id).map(|target| target.bounds())
            })
            .unwrap();
        if bounds.is_some() && bounds == last {
            return;
        }
        last = bounds;
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("{id} never came to rest");
}

#[gpui_kit::test]
fn keyboard_opens_a_pull_request_and_escape_returns(cx: &mut TestAppContext) {
    let (window, workspace) = open(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let opened = cx.update(|cx| workspace.read(cx).reviewing(cx));
    assert_eq!(opened, Some(featured()), "the first review request opens");

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| workspace.read(cx).reviewing(cx)), None);
}

#[gpui_kit::test]
fn ctrl_b_percent_splits_the_open_review(cx: &mut TestAppContext) {
    let (window, workspace) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-b", cx);
        window.press("%", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let panes = cx.update(|cx| {
        workspace
            .read(cx)
            .review_panes()
            .map(|panes| panes.read(cx).pane_count())
    });
    assert_eq!(panes, Some(2));
}

#[gpui_kit::test]
fn cmd_b_hides_the_shared_tree_and_remembers_it(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(on_screen(window, "outline"), "the tree shows at first");
        window.press("cmd-b", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!on_screen(window, "outline"), "⌘B hides the tree");
    })
    .unwrap();
    let collapsed = cx.update(|cx| {
        cx.stores()
            .settings()
            .read(cx)
            .preferences()
            .outline_collapsed
    });
    assert!(collapsed, "hiding the tree is remembered");
}

#[gpui_kit::test]
fn viewed_checkbox_marks_the_file(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    let path = "crates/review/src/thread.rs";
    let viewed = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            use app_state_ui::StoresExt as _;
            cx.stores()
                .pull_requests()
                .read(cx)
                .loaded_detail(&featured())
                .map(|detail| detail.is_viewed(path))
        })
    };
    assert_eq!(viewed(cx), Some(false));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window
            .within(gpui_kit::SharedString::from(format!("file-{path}")))
            .click("viewed", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(viewed(cx), Some(true));
}

#[gpui_kit::test]
fn approving_through_the_dialog_updates_reviewers_and_timeline(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("submit-review", cx);
    })
    .unwrap();
    settle(window, "verdict-Approve", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("verdict-Approve", cx);
        assert_eq!(window.find("verdict-Approve").checked(), Some(true));
        assert_eq!(window.find("verdict-Comment").checked(), Some(false));
        window.click("submit", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        use app_state_ui::StoresExt as _;
        let store = cx.stores().pull_requests().read(cx);
        let detail = store
            .loaded_detail(&featured())
            .expect("detail stays loaded");
        let viewer = detail.reviewers.iter().find(|r| r.is_viewer).unwrap();
        assert_eq!(viewer.state, ReviewerState::Approved);
        assert!(matches!(
            detail.timeline.last(),
            Some(TimelineItem::Reviewed {
                verdict: ReviewVerdict::Approve,
                ..
            })
        ));
        assert!(
            detail
                .threads
                .iter()
                .flat_map(|t| &t.comments)
                .all(|c| !c.is_pending)
        );
    });
}

#[gpui_kit::test]
fn escape_in_the_composer_keeps_the_review_and_its_draft(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (window, workspace) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("description-reply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(window.has_focused_input(cx), "Reply focuses the composer");
        window.input("Looks right", cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| workspace.read(cx).reviewing(cx)),
        Some(featured()),
        "the review stays open"
    );
    cx.update_window(window, |_, window, cx| {
        assert!(!window.has_focused_input(cx), "Escape leaves the composer");
        window.render_frame(cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| workspace.read(cx).reviewing(cx)), None);
}

/// Each pane's top file, first halves before second halves.
fn pane_files(workspace: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<Option<String>> {
    cx.update(|cx| {
        let panes = workspace.read(cx).review_panes().expect("review open");
        let panes = panes.read(cx).panes();
        panes
            .iter()
            .map(|pane| pane.read(cx).pane_state().scroll.file)
            .collect()
    })
}

#[gpui_kit::test]
fn alt_shift_o_jumps_the_focused_pane_to_a_fuzzy_match(cx: &mut TestAppContext) {
    let (window, workspace) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-b", cx);
        window.press("%", cx);
        window.render_frame(cx);
        window.press("alt-shift-o", cx);
    })
    .unwrap();
    settle(window, "go-to-file", cx);
    let before = pane_files(&workspace, cx);
    // The query reaches the list after this update ends, so Enter goes in the next one.
    cx.update_window(window, |_, window, cx| window.input("rndr", cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("go-to-file").is_none(),
            "picking a file closes go-to-file"
        );
    })
    .unwrap();
    assert_eq!(
        pane_files(&workspace, cx),
        vec![
            before[0].clone(),
            Some("crates/review/src/render.rs".into())
        ],
        "only the focused pane jumps"
    );
}

#[gpui_kit::test]
fn alt_shift_o_types_in_the_composer(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("description-reply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.press("alt-shift-o", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("go-to-file").is_none(),
            "go-to-file stays shut while typing"
        );
    })
    .unwrap();
}

/// Opens find in the focused pane and searches for `query`.
fn find_text(window: AnyWindowHandle, query: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-f", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let query = query.to_string();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.input(&query, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn press_and_render(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    let key = key.to_string();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(&key, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// The find bar's count, as painted.
fn find_count(window: AnyWindowHandle, cx: &mut TestAppContext) -> String {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window
            .find("find-count")
            .label()
            .unwrap_or_default()
            .to_string()
    })
    .unwrap()
}

#[gpui_kit::test]
fn cmd_f_counts_and_steps_through_matches(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    find_text(window, "fn ", cx);
    press_and_render(window, "enter", cx);
    let count = find_count(window, cx);
    assert!(
        count.starts_with("2 of "),
        "Enter steps to the second match: {count}"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let current = window.find("find-match-current");
        assert!(
            current.visible(),
            "the current match is on screen: {:?}",
            current.bounds()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn shift_enter_wraps_to_the_last_match(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    find_text(window, "fn ", cx);
    press_and_render(window, "shift-enter", cx);
    let count = find_count(window, cx);
    let total = count.rsplit(' ').next().unwrap_or_default().to_string();
    assert_eq!(
        count,
        format!("{total} of {total}"),
        "⇧Enter from the first wraps to the last"
    );
}

#[gpui_kit::test]
fn escape_closes_find_and_keeps_the_review(cx: &mut TestAppContext) {
    let (window, workspace) = open_featured(cx);
    find_text(window, "fn ", cx);
    press_and_render(window, "escape", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!on_screen(window, "find-bar"), "Escape closes find");
    })
    .unwrap();
    assert_eq!(
        cx.update(|cx| workspace.read(cx).reviewing(cx)),
        Some(featured()),
        "the review stays open"
    );
}

#[gpui_kit::test]
fn switching_tabs_reruns_find(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    find_text(window, "hunk hash", cx);
    let everything = find_count(window, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(("timeline-filter", 2usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let changes = find_count(window, cx);
    assert_ne!(everything, changes, "Changes drops the description's match");
}

#[gpui_kit::test]
fn find_matches_comment_text(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    find_text(window, "pick up its thread", cx);
    assert_eq!(find_count(window, cx), "1 of 1");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.find("find-match-current").visible(),
            "the comment is outlined"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_check_opens_its_log_and_escape_closes_only_the_sheet(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (window, workspace) = open_featured(cx);
    let row = format!("inspector-check-{}", fixtures::SNAPSHOT_CHECK_ID);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(gpui_kit::SharedString::from(row), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(window.has_active_sheet(cx), "the check log opens");
        window.render_frame(cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(!window.has_active_sheet(cx), "Escape closes the sheet");
    })
    .unwrap();
    assert_eq!(
        cx.update(|cx| workspace.read(cx).reviewing(cx)),
        Some(featured()),
        "the review stays open under the sheet"
    );
}

#[gpui_kit::test]
fn hiding_the_inspector_is_remembered_and_leaves_check_icons(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    use gpui_kit::component::WindowExt as _;
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            on_screen(window, "inspector"),
            "the inspector shows at first"
        );
        window.click("toggle-inspector", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let collapsed = cx.update(|cx| {
        cx.stores()
            .settings()
            .read(cx)
            .preferences()
            .inspector_collapsed
    });
    assert!(collapsed, "hiding the inspector is remembered");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            !on_screen(window, "inspector"),
            "the inspector leaves the screen"
        );
        let icon = window.find(check_rail_icon());
        assert!(icon.visible(), "its check icons stay on screen");
        assert!(
            window.viewport_size().width - icon.bounds().right() < px(24.),
            "the icons sit at the window's right edge: {:?}",
            icon.bounds()
        );
        window.click(check_rail_icon(), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(
            window.has_active_sheet(cx),
            "a folded check's icon opens its log"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_title_bar_holds_the_only_inspector_toggle_at_its_right_end(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    for inspector_shown in [true, false, true] {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                on_screen(window, "inspector"),
                inspector_shown,
                "the title bar's toggle shows and hides the inspector"
            );
            assert_eq!(
                on_screen(window, check_rail_icon()),
                !inspector_shown,
                "the check rail stands in for the hidden inspector"
            );
            let toggle = window.find("toggle-inspector");
            assert!(toggle.visible(), "the toggle stays in the title bar");
            for earlier in ["toggle-focus", "notifications"] {
                assert!(
                    toggle.bounds().left() >= window.find(earlier).bounds().right(),
                    "the toggle sits right of {earlier}: {:?}",
                    toggle.bounds()
                );
            }
            assert!(
                window.viewport_size().width - toggle.bounds().right() < px(24.),
                "the toggle ends the title bar: {:?}",
                toggle.bounds()
            );
            for duplicate in ["collapse-inspector", "expand-inspector"] {
                assert!(
                    window.try_find(duplicate).is_none(),
                    "{duplicate} duplicates the title bar's toggle (inspector shown: {inspector_shown})"
                );
            }
            window.click("toggle-inspector", cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

/// Whether `id` was painted inside the window in the last frame.
fn on_screen(window: &gpui_kit::Window, id: impl Into<gpui_kit::ElementId>) -> bool {
    window.try_find(id).is_some_and(|element| element.visible())
}

fn check_rail_icon() -> gpui_kit::SharedString {
    format!("inspector-check-icon-{}", fixtures::SNAPSHOT_CHECK_ID).into()
}

#[gpui_kit::test]
fn clicking_a_line_number_starts_a_pending_thread(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (window, _) = open_featured(cx);
    let path = "crates/review/src/thread.rs";
    let threads = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            use app_state_ui::StoresExt as _;
            cx.stores()
                .pull_requests()
                .read(cx)
                .loaded_detail(&featured())
                .map(|detail| {
                    detail
                        .threads
                        .iter()
                        .filter(|thread| thread.path == path && thread.line == 137)
                        .count()
                })
        })
    };
    assert_eq!(threads(cx), Some(0));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("gutter-137-137", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.has_focused_input(cx),
            "the line comment field takes focus"
        );
        window.input("Rename this", cx);
        window.press("enter", cx);
        window.input("and its callers", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        threads(cx),
        Some(0),
        "Enter adds a line instead of submitting"
    );
    cx.update_window(window, |_, window, cx| window.press("cmd-enter", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(threads(cx), Some(1));
    let body = cx.update(|cx| {
        use app_state_ui::StoresExt as _;
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .and_then(|detail| {
                detail
                    .threads
                    .iter()
                    .find(|thread| thread.line == 137)
                    .map(|thread| thread.comments[0].body.clone())
            })
    });
    assert_eq!(body.as_deref(), Some("Rename this\nand its callers"));
}

/// Picks item `ix` of the context menu open in `window`, once it has come to rest.
fn pick_menu_item(window: AnyWindowHandle, ix: usize, cx: &mut TestAppContext) {
    settle(window, "popup-menu", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(ix, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn selected_diff_lines_copy_and_quote_into_a_line_comment(cx: &mut TestAppContext) {
    use gpui_kit::{point, px};
    let (window, _) = open_featured(cx);
    // The code starts past each gutter; drag from the first line's indent into the
    // second line's `let`, then open the second line's menu.
    let select_and_open_menu = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let first = window.find("gutter-136-136").bounds();
            let second = window.find("gutter-137-137").bounds();
            let from = point(first.right() + px(12.), first.center().y);
            let to = point(second.right() + px(110.), second.center().y);
            window.drag(from, to, cx);
            window.right_click("gutter-137-137", cx);
        })
        .unwrap();
    };
    select_and_open_menu(cx);
    // Copy, Copy line, separator, Comment on line, Suggest change, Quote in comment.
    pick_menu_item(window, 0, cx);
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default();
    assert!(
        copied.contains("pub fn apply_push") && copied.contains("\n        let"),
        "Copy takes the selection across both lines: {copied:?}"
    );
    assert_eq!(copied.lines().count(), 2, "and nothing else: {copied:?}");

    // Picking Copy clicked the menu, which ends the selection as any click does.
    select_and_open_menu(cx);
    pick_menu_item(window, 5, cx);
    cx.update_window(window, |_, window, cx| {
        use gpui_kit::component::WindowExt as _;
        window.render_frame(cx);
        assert!(
            window.has_focused_input(cx),
            "the quote opens a comment on the line, focused"
        );
        window.input("Why both?", cx);
        window.press("cmd-enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let body = cx.update(|cx| {
        use app_state_ui::StoresExt as _;
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .and_then(|detail| {
                detail
                    .threads
                    .iter()
                    .find(|thread| thread.line == 137)
                    .map(|thread| thread.comments[0].body.clone())
            })
    });
    assert_eq!(
        body,
        Some(format!("```rs\n{}\n```\n\nWhy both?", copied.trim_end())),
        "the selection lands as a Rust code block above the comment"
    );
}

/// Answers every agent session with `events` instead of starting an agent, and returns
/// the prompts the sessions were given.
fn script_agent(
    events: impl Fn() -> Vec<acp_client::AgentEvent> + 'static,
    cx: &mut TestAppContext,
) -> std::rc::Rc<std::cell::RefCell<Vec<String>>> {
    let prompts = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = prompts.clone();
    cx.update(|cx| {
        cx.set_global(agent_review_ui::AgentLauncher::new(move |request, _| {
            seen.borrow_mut().push(request.prompt.clone());
            let (sender, receiver) = async_channel::unbounded();
            for event in events() {
                sender.try_send(event).unwrap();
            }
            (receiver, None)
        }))
    });
    prompts
}

#[gpui_kit::test]
fn dragged_lines_ask_an_agent_and_its_suggestion_becomes_a_comment(cx: &mut TestAppContext) {
    use acp_client::{AgentEvent, RunOutcome};
    use gpui_kit::component::WindowExt as _;
    let answer = "**Both are needed.** The loop keeps blocks the reviewer already saw.";
    let suggestion = "Could `entry().or_insert` drop a block whose hunk changed?";
    let prompts = script_agent(
        move || {
            vec![
                AgentEvent::Connected {
                    name: None,
                    version: None,
                },
                AgentEvent::Message("**Both are needed.** ".into()),
                AgentEvent::Message("The loop keeps blocks the reviewer already saw.".into()),
                AgentEvent::Suggestion(suggestion.into()),
                AgentEvent::Finished(RunOutcome::Completed),
            ]
        },
        cx,
    );
    let (window, _) = open_featured(cx);
    let path = "crates/review/src/thread.rs";

    // From the context line above the change, over the removal, into the third added line.
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to("gutter-137-137", "gutter-0-140", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        for line in ["137-137", "138-0", "0-138", "0-139", "0-140"] {
            assert!(
                on_screen(window, format!("selected-{line}")),
                "line {line} is painted as selected"
            );
        }
        for line in ["136-136", "0-141"] {
            assert!(
                !on_screen(window, format!("selected-{line}")),
                "line {line} is outside the drag"
            );
        }
        assert_eq!(
            window.find("ask-agent-heading").label(),
            Some("Ask Claude Code about lines 137\u{2013}140"),
            "the question field names the agent and the lines"
        );
        let heading = window.find("ask-agent-heading").bounds();
        let last = window.find("gutter-0-140").bounds();
        let next = window.find("gutter-0-141").bounds();
        assert!(
            heading.top() >= last.bottom() && heading.bottom() <= next.top(),
            "the field sits under the last selected line: {heading:?}"
        );
        assert!(
            window.has_focused_input(cx),
            "the question field takes focus"
        );
        window.input("Why both passes?", cx);
        window.press("cmd-enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let prompt = prompts
        .borrow()
        .first()
        .cloned()
        .expect("the agent was asked");
    assert!(
        prompt.contains(&format!("asks about lines 137\u{2013}140 of `{path}`")),
        "{prompt}"
    );
    assert!(prompt.contains("Why both passes?"), "{prompt}");
    assert!(
        prompt.contains("  138       -        self.blocks = blocks.collect();"),
        "the selection carries its numbers and signs: {prompt}"
    );
    assert!(
        prompt.contains("@@ -201,6 +204,9 @@"),
        "the file's whole change comes along: {prompt}"
    );
    assert!(prompt.contains("`suggest_comment`"), "{prompt}");

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            !on_screen(window, "selected-0-139"),
            "asking ends the selection"
        );
        assert!(
            window.try_find("ask-agent-heading").is_none(),
            "the question field gives way to the answer"
        );
        let shown = window
            .try_find("agent-answer")
            .expect("the answer shows under the selected lines");
        assert!(shown.visible(), "the answer is painted");
        assert_eq!(shown.label(), Some(answer), "both chunks of the reply");
        let suggested = window
            .try_find("suggested-comment")
            .expect("the suggested comment shows under the answer");
        assert!(suggested.visible(), "the suggested comment is painted");
        assert_eq!(suggested.label(), Some(suggestion));
        assert!(
            suggested.bounds().top() >= shown.bounds().bottom(),
            "the suggestion follows the answer"
        );
        window.click("add-suggested-comment", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let pending = cx.update(|cx| {
        use app_state_ui::StoresExt as _;
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .and_then(|detail| {
                detail
                    .threads
                    .iter()
                    .find(|thread| thread.path == path && thread.line == 140)
                    .map(|thread| {
                        (
                            thread.comments[0].body.clone(),
                            thread.comments[0].is_pending,
                        )
                    })
            })
    });
    assert_eq!(
        pending,
        Some((suggestion.to_string(), true)),
        "the suggestion waits in the pending review on the last selected line"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            on_screen(window, "suggested-comment-added"),
            "the card says the comment is in the review"
        );
        assert!(
            window.try_find("add-suggested-comment").is_none(),
            "and offers to add it only once"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_click_on_a_line_number_selects_nothing_and_escape_drops_a_selection(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("gutter-137-137", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            !on_screen(window, "selected-137-137"),
            "a click starts a comment, not a selection"
        );
        assert!(window.try_find("ask-agent-heading").is_none());
        assert!(window.has_focused_input(cx), "the line comment has focus");
        window.drag_to("gutter-136-136", "gutter-137-137", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            on_screen(window, "selected-136-136"),
            "the dragged lines are painted as selected"
        );
        assert!(on_screen(window, "ask-agent-heading"));
        // The first Escape leaves the field, the second drops the selection.
        window.press("escape", cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            !on_screen(window, "selected-136-136"),
            "Escape drops the selection"
        );
        assert!(window.try_find("ask-agent-heading").is_none());
        assert!(
            on_screen(window, "gutter-136-136"),
            "and the review stays open"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_split_layout_selects_lines_of_the_side_that_was_dragged(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(("diff-mode", 1usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to("split-new-138", "split-new-140", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        for line in ["new-138", "new-139", "new-140"] {
            assert!(
                on_screen(window, format!("selected-{line}")),
                "head line {line} is painted as selected"
            );
        }
        assert!(
            !on_screen(window, "selected-old-138"),
            "the removal beside the first line isn't part of the drag"
        );
        assert_eq!(
            window.find("ask-agent-heading").label(),
            Some("Ask Claude Code about lines 138\u{2013}140")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_drag_released_outside_the_diff_still_ends(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let from = window.find("gutter-136-136").bounds().center();
        let outside = window.find("toggle-inspector").bounds().center();
        // Let go over the title bar, outside the timeline.
        window.drag(from, outside, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // With the drag over, crossing a line with the button held selects nothing.
        let later = window.find("gutter-0-139").bounds().center();
        window.drag(gpui_kit::point(later.x + px(300.), later.y), later, cx);
        window.render_frame(cx);
        assert!(
            !on_screen(window, "selected-0-139"),
            "a drag that ended outside the diff isn't still extending"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn ask_agent_in_the_line_menu_asks_about_that_line(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.right_click("gutter-137-137", cx);
    })
    .unwrap();
    // Copy, Copy line, separator, Comment on line, Suggest change, Quote in comment,
    // Ask agent….
    pick_menu_item(window, 6, cx);
    cx.update_window(window, |_, window, cx| {
        use gpui_kit::component::WindowExt as _;
        window.render_frame(cx);
        assert!(
            on_screen(window, "selected-137-137"),
            "the line the menu was opened on is selected"
        );
        assert!(!on_screen(window, "selected-136-136"));
        assert_eq!(
            window.find("ask-agent-heading").label(),
            Some("Ask Claude Code about line 137")
        );
        assert!(window.has_focused_input(cx), "the question field has focus");
    })
    .unwrap();
}

/// Drags over two lines and asks `question` about them.
fn ask_about_two_lines(window: AnyWindowHandle, question: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to("gutter-136-136", "gutter-137-137", cx);
        window.render_frame(cx);
        window.input(question, cx);
        window.press("cmd-enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn a_question_stops_its_suggestion_opens_as_a_draft_and_the_card_dismisses(
    cx: &mut TestAppContext,
) {
    use acp_client::AgentEvent;
    use gpui_kit::component::WindowExt as _;
    // No end of turn: the agent is still at work after suggesting.
    script_agent(
        || {
            vec![
                AgentEvent::Connected {
                    name: None,
                    version: None,
                },
                AgentEvent::Message("Still looking.".into()),
                AgentEvent::Suggestion("Name the pass.".into()),
            ]
        },
        cx,
    );
    let (window, _) = open_featured(cx);
    ask_about_two_lines(window, "Why?", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            on_screen(window, "stop-question"),
            "a question in progress offers Stop"
        );
        assert!(window.try_find("dismiss-question").is_none());
        window.click("stop-question", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("stop-question").is_none(),
            "a stopped question no longer offers Stop"
        );
        assert!(on_screen(window, "dismiss-question"));
        assert!(
            on_screen(window, "agent-answer"),
            "what it answered before the stop stays"
        );
        window.click("edit-suggested-comment", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.has_focused_input(cx),
            "Edit opens the suggestion as a line comment draft"
        );
        window.input(" Please.", cx);
        window.press("cmd-enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let body = cx.update(|cx| {
        use app_state_ui::StoresExt as _;
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .and_then(|detail| {
                detail
                    .threads
                    .iter()
                    .find(|thread| thread.line == 137 && thread.comments[0].is_pending)
                    .map(|thread| thread.comments[0].body.clone())
            })
    });
    assert_eq!(
        body.as_deref(),
        Some("Name the pass. Please."),
        "the edited suggestion is the comment"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("dismiss-question", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("agent-answer").is_none(),
            "Dismiss removes the card"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_question_that_fails_says_why(cx: &mut TestAppContext) {
    use acp_client::AgentEvent;
    script_agent(
        || vec![AgentEvent::Failed("The agent isn’t signed in.".into())],
        cx,
    );
    let (window, _) = open_featured(cx);
    ask_about_two_lines(window, "Why?", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let failed = window
            .try_find("agent-question-failed")
            .expect("the failure shows on the card");
        assert!(failed.visible());
        assert_eq!(failed.label(), Some("The agent isn’t signed in."));
        assert!(on_screen(window, "dismiss-question"));
        assert!(window.try_find("suggested-comment").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn quote_reply_on_the_description_quotes_it_in_the_composer(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    let (window, _) = open_featured(cx);
    let description = cx.update(|cx| {
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .map(|detail| detail.body.to_string())
            .unwrap_or_default()
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.right_click("description-body", cx);
    })
    .unwrap();
    // Copy, Copy text, separator, Quote reply.
    pick_menu_item(window, 3, cx);
    cx.update_window(window, |_, window, cx| {
        use gpui_kit::component::WindowExt as _;
        window.render_frame(cx);
        assert!(window.has_focused_input(cx), "the composer takes focus");
        window.input("Agreed", cx);
        window.press("cmd-enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let posted = cx.update(|cx| {
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .and_then(|detail| {
                detail.timeline.iter().rev().find_map(|item| match item {
                    TimelineItem::Commented { comment } => Some(comment.body.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default()
    });
    let (quote, reply) = posted
        .rsplit_once("\n\n")
        .expect("the reply follows the quote");
    assert_eq!(reply, "Agreed");
    assert!(
        quote.lines().all(|line| line.starts_with('>')),
        "every line is quoted: {quote:?}"
    );
    let unquoted: Vec<&str> = quote
        .lines()
        .map(|line| line.trim_start_matches('>').trim_start())
        .collect();
    let original: Vec<&str> = description
        .trim_end()
        .lines()
        .map(str::trim_start)
        .collect();
    assert_eq!(
        unquoted, original,
        "with nothing selected the whole description is quoted"
    );
}

#[gpui_kit::test]
fn the_composer_cuts_and_pastes_through_the_app_menu(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (window, _) = open_featured(cx);
    let open_menu = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.right_click("composer-edit-menu", cx);
        })
        .unwrap();
    };
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("description-reply", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.input("Hello there", cx))
        .unwrap();
    // Cut, Copy, Paste, separator, Select all.
    open_menu(cx);
    pick_menu_item(window, 4, cx);
    open_menu(cx);
    pick_menu_item(window, 0, cx);
    let cut = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(
        cut.as_deref(),
        Some("Hello there"),
        "Cut takes the selected text"
    );

    open_menu(cx);
    pick_menu_item(window, 2, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_focused_input(cx), "the composer keeps focus");
        window.press("cmd-enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let posted = cx.update(|cx| {
        use app_state_ui::StoresExt as _;
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .and_then(|detail| {
                detail.timeline.iter().rev().find_map(|item| match item {
                    TimelineItem::Commented { comment } => Some(comment.body.clone()),
                    _ => None,
                })
            })
    });
    assert_eq!(posted.as_deref(), Some("Hello there"), "Paste put it back");
}

#[gpui_kit::test]
fn approve_asks_in_a_popover_listing_approvals(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("approve", cx);
    })
    .unwrap();
    settle(window, "approve-confirm", cx);
    let viewer_state = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            use app_state_ui::StoresExt as _;
            cx.stores()
                .pull_requests()
                .read(cx)
                .loaded_detail(&featured())
                .and_then(|detail| detail.reviewers.iter().find(|r| r.is_viewer).cloned())
                .map(|viewer| viewer.state)
        })
    };
    assert_eq!(
        viewer_state(cx),
        Some(ReviewerState::Pending),
        "opening the popover approves nothing"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("approve-confirm", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(viewer_state(cx), Some(ReviewerState::Approved));
}

#[gpui_kit::test]
fn menu_actions_close_reopen_approve_and_request_reviews(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    let (_, _) = open(cx);
    let key = PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 477);
    let state = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            cx.stores()
                .pull_requests()
                .read(cx)
                .pull_request(&key)
                .map(|pr| (pr.state, pr.review))
        })
    };
    let close = cx.update(|cx| {
        cx.stores()
            .pull_requests()
            .clone()
            .update(cx, |store, cx| store.set_open(&key, false, cx))
    });
    cx.run_until_parked();
    assert!(futures::executor::block_on(close).is_ok());
    assert_eq!(
        state(cx).map(|s| s.0),
        Some(pr_model::PullRequestState::Closed)
    );

    let reopen = cx.update(|cx| {
        cx.stores()
            .pull_requests()
            .clone()
            .update(cx, |store, cx| store.set_open(&key, true, cx))
    });
    cx.run_until_parked();
    assert!(futures::executor::block_on(reopen).is_ok());
    assert_eq!(
        state(cx).map(|s| s.0),
        Some(pr_model::PullRequestState::Open)
    );

    let approve = cx.update(|cx| {
        let viewer = cx.stores().settings().read(cx).viewer_actor();
        cx.stores()
            .pull_requests()
            .clone()
            .update(cx, |store, cx| store.approve(&key, &viewer, cx))
    });
    cx.run_until_parked();
    assert!(futures::executor::block_on(approve).is_ok());
    assert_eq!(
        state(cx).map(|s| s.1),
        Some(pr_model::ReviewDecision::Approved)
    );

    let people = cx.update(|cx| {
        cx.stores()
            .pull_requests()
            .clone()
            .update(cx, |store, cx| store.assignable_users(key.repo(), cx))
    });
    cx.run_until_parked();
    let people = futures::executor::block_on(people).expect("reviewers load");
    assert!(people.iter().any(|actor| actor.login == "tanya"));
    let request = cx.update(|cx| {
        let tanya = people.iter().find(|a| a.login == "tanya").cloned().unwrap();
        cx.stores().pull_requests().clone().update(cx, |store, cx| {
            store.request_reviewers(&key, vec![tanya], cx)
        })
    });
    cx.run_until_parked();
    assert!(futures::executor::block_on(request).is_ok());
}

#[gpui_kit::test]
fn picking_a_commit_shows_only_its_changes(cx: &mut TestAppContext) {
    let (window, workspace) = open_featured(cx);
    let review = cx
        .update(|cx| workspace.read(cx).review_view(cx))
        .expect("review open");
    let (all, sha) = cx.update(|cx| {
        let review = review.read(cx);
        let sha = review
            .detail()
            .and_then(|d| d.commits.first())
            .map(|c| c.sha.clone());
        (review.shown_files().len(), sha)
    });
    cx.update_window(window, |_, _, cx| {
        review.update(cx, |review, cx| {
            review.show_commit(sha.clone().unwrap(), cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    for _ in 0..20 {
        if cx.update(|cx| review.read(cx).shown_files().len()) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
        cx.run_until_parked();
    }
    let picked = cx.update(|cx| review.read(cx).shown_files().len());
    assert_eq!(picked, all - 1, "the sample commit drops one file");
    cx.update_window(window, |_, _, cx| {
        review.update(cx, |review, cx| review.show_all_commits(cx))
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| review.read(cx).shown_files().len()), all);
}

#[gpui_kit::test]
fn focus_mode_hides_both_sidebars_and_brings_them_back(cx: &mut TestAppContext) {
    let (window, _) = open_featured(cx);
    let shown = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            (
                window.try_find("outline").is_some(),
                window.try_find("inspector").is_some(),
            )
        })
        .unwrap()
    };
    let click = |id: &'static str, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
        cx.run_until_parked();
    };
    assert_eq!(shown(cx), (true, true));
    click("toggle-inspector", cx);
    assert_eq!(shown(cx), (true, false));
    click("toggle-focus", cx);
    assert_eq!(shown(cx), (false, false));
    click("toggle-focus", cx);
    assert_eq!(shown(cx), (true, false), "focus mode restores what it hid");
    click("toggle-inspector", cx);
    assert_eq!(shown(cx), (true, true));
}

#[gpui_kit::test]
fn a_link_from_zed_opens_a_comment_on_its_line(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    let (window, workspace) = open(cx);
    cx.update(|cx| {
        let store = cx.stores().pull_requests().clone();
        store.update(cx, |store, cx| store.load_detail(&featured(), cx));
    });
    cx.run_until_parked();
    let (path, line) = cx.update(|cx| {
        let store = cx.stores().pull_requests().read(cx);
        let detail = store.loaded_detail(&featured()).unwrap();
        let file = detail.files.iter().find(|f| !f.hunks.is_empty()).unwrap();
        let line = file
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .find_map(|l| l.new_line)
            .unwrap();
        (file.path.clone(), line)
    });
    let url = zed_bridge::SwitchfoldLink::Open {
        key: featured(),
        file: Some(path),
        line: Some(line),
    }
    .to_url();
    cx.update_window(window, |_, window, cx| {
        workspace.update(cx, |workspace, cx| workspace.open_link(&url, window, cx))
    })
    .unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    let composer = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find("line-comment-cancel").is_some()
        })
        .unwrap();
    assert!(composer, "the link opens a comment composer on that line");
}

#[gpui_kit::test]
fn zed_agent_drafts_pending_comments_but_cannot_post(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    use zed_bridge::mcp::{BridgeRequest, tool};
    let (window, workspace) = open_featured(cx);
    let ask = |request: BridgeRequest, cx: &mut TestAppContext| {
        let task = cx
            .update_window(window, |_, window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace.answer_zed(request, window, cx)
                })
            })
            .unwrap();
        cx.run_until_parked();
        cx.foreground_executor().block_test(task)
    };
    let threads = ask(BridgeRequest::new(tool::THREADS, serde_json::json!({})), cx);
    assert!(!threads.is_error && threads.text.contains("thread_id"));

    let (path, line) = cx.update(|cx| {
        let store = cx.stores().pull_requests().read(cx);
        let detail = store.loaded_detail(&featured()).unwrap();
        let file = detail.files.iter().find(|f| !f.hunks.is_empty()).unwrap();
        let line = file
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .find_map(|l| l.new_line)
            .unwrap();
        (file.path.clone(), line)
    });
    let drafted = ask(
        BridgeRequest::new(
            tool::COMMENT,
            serde_json::json!({ "path": path, "line": line, "body": "From Zed" }),
        ),
        cx,
    );
    assert!(!drafted.is_error, "{}", drafted.text);
    let outside = ask(
        BridgeRequest::new(
            tool::COMMENT,
            serde_json::json!({ "path": path, "line": 999_999, "body": "Nope" }),
        ),
        cx,
    );
    assert!(outside.is_error);
    let pending = cx.update(|cx| {
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .unwrap()
            .threads
            .iter()
            .flat_map(|t| &t.comments)
            .any(|c| c.is_pending && c.body == "From Zed")
    });
    assert!(pending, "the comment waits in the pending review");
    let other = ask(
        BridgeRequest::new("submit_review", serde_json::json!({})),
        cx,
    );
    assert!(other.is_error);
}

#[gpui_kit::test]
fn list_filters_show_as_chips_that_clear_them(cx: &mut TestAppContext) {
    let (window, workspace) = open(cx);
    let list = cx.update(|cx| workspace.read(cx).pull_request_list().clone());
    list.update(cx, |list, cx| {
        list.update_filters(|filters| filters.author = Some("tanya".into()), cx)
    });
    cx.run_until_parked();
    let chip = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find("chip-Author").is_some()
        })
        .unwrap();
    assert!(chip, "an active filter is visible above the list");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("chip-Author", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        list.read_with(cx, |list, _| list.filters().author.clone()),
        None
    );
    let gone = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find("chip-Author").is_none()
        })
        .unwrap();
    assert!(gone);
}

#[gpui_kit::test]
fn pinned_repositories_get_a_section_that_folds(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    let (window, _) = open(cx);
    let find = |id: &'static str, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    };
    assert!(
        !find("fold-Pinned", cx),
        "no Pinned section until something is pinned"
    );
    cx.update(|cx| {
        let settings = cx.stores().settings().clone();
        settings.update(cx, |settings, cx| {
            settings.update_preferences(cx, |p| {
                p.set_pinned("flaticols/switchfold", true);
                p.set_repository_color(
                    "flaticols/switchfold",
                    Some(settings_model::RepositoryColor::Blue),
                );
            })
        });
    });
    cx.run_until_parked();
    assert!(find("fold-Pinned", cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("fold-Pinned", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let folded = cx.update(|cx| {
        cx.stores()
            .settings()
            .read(cx)
            .preferences()
            .is_folded("Pinned")
    });
    assert!(folded, "the chevron folds the section and remembers it");
    assert!(find("fold-Repositories", cx), "Repositories folds too");
}

#[gpui_kit::test]
fn a_color_circle_sets_the_repository_color_and_closes_the_menu(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    let (window, _) = open(cx);
    cx.update(|cx| {
        let settings = cx.stores().settings().clone();
        settings.update(cx, |settings, cx| {
            settings.update_preferences(cx, |p| p.set_pinned("flaticols/switchfold", true))
        });
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Sidebar rows are named section-row; section 1 is Pinned.
        window.right_click("1-0", cx);
    })
    .unwrap();
    settle(window, "repo-color-Red", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("repo-color-Red", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let color = cx.update(|cx| {
        cx.stores()
            .settings()
            .read(cx)
            .preferences()
            .repository_color("flaticols/switchfold")
    });
    assert_eq!(color, Some(settings_model::RepositoryColor::Red));
    let still_open = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find("repo-color-Red").is_some()
        })
        .unwrap();
    assert!(!still_open, "picking a color closes the menu");
}

#[gpui_kit::test]
fn merging_from_the_title_bar_uses_the_chosen_method(cx: &mut TestAppContext) {
    use app_state_ui::StoresExt as _;
    let (window, _) = open_featured(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("merge", cx);
    })
    .unwrap();
    settle(window, "merge-confirm", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("merge-confirm", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let state = cx.update(|cx| {
        cx.stores()
            .pull_requests()
            .read(cx)
            .pull_request(&featured())
            .map(|pr| pr.state)
    });
    assert_eq!(
        state,
        Some(pr_model::PullRequestState::Merged),
        "merging with the repository's default, squash, marks it merged"
    );
}

#[gpui_kit::test]
fn agents_run_read_only_commands_unasked_once_the_setting_allows_it(cx: &mut TestAppContext) {
    use acp_client::{AgentEvent, Decision, PermissionChoice, PermissionPrompt};
    use app_state_ui::StoresExt as _;
    type Answer = futures::channel::oneshot::Receiver<Option<String>>;
    let answers: std::rc::Rc<std::cell::RefCell<Vec<Answer>>> = Default::default();
    let kept = answers.clone();
    script_agent(
        move || {
            let prompt = |command: &str| {
                let (prompt, answer) = PermissionPrompt::scripted(
                    format!("Run {command}"),
                    Some(command.to_string()),
                    vec![
                        PermissionChoice::new("allow", "Allow", Decision::AllowOnce),
                        PermissionChoice::new("deny", "Deny", Decision::Deny),
                    ],
                );
                kept.borrow_mut().push(answer);
                AgentEvent::Permission(prompt)
            };
            vec![
                AgentEvent::Connected {
                    name: None,
                    version: None,
                },
                prompt("sed -n 120,160p crates/review/src/thread.rs | grep -n blocks"),
                AgentEvent::Message("Read the loop.".into()),
                prompt("rm crates/review/src/thread.rs"),
            ]
        },
        cx,
    );
    let (window, _) = open(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-,", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Settings navigation rows are section-group-row; Agents & editor is the third.
        window.click("0-0-2", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("allow-read-only-commands").checked(),
            Some(false),
            "agents ask before every command by default"
        );
        window.click("allow-read-only-commands", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("allow-read-only-commands").checked(),
            Some(true),
            "the switch turns on"
        );
        window.click("back-to-pull-requests", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let allowed = cx.update(|cx| {
        cx.stores()
            .settings()
            .read(cx)
            .preferences()
            .allow_read_only_commands
    });
    assert!(allowed, "the setting is saved");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("flaticols/switchfold#482", cx);
    })
    .unwrap();
    cx.run_until_parked();

    ask_about_two_lines(window, "Why both?", cx);
    let mut answers = answers.borrow_mut();
    assert_eq!(answers.len(), 2);
    assert_eq!(
        answers[0].try_recv(),
        Ok(Some(Some("allow".to_string()))),
        "a command that only reads runs without asking"
    );
    assert_eq!(
        answers[1].try_recv(),
        Ok(None),
        "rm still waits for the reviewer"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            on_screen(window, "allow-once"),
            "rm still asks in the question card"
        );
    })
    .unwrap();
}

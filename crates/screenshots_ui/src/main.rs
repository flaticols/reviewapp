//! Renders Switchfold's screens with the sample workspace to PNG files, offscreen, in
//! light and dark mode, for comparison with the Sketch design.
//!
//! `cargo run -p screenshots_ui -- [--live] [--pr owner/name#N] [out-dir] [scene…]`
//!
//! `--live` renders the signed-in `gh` account's real pull requests instead of the sample;
//! `--pr` picks the pull request the review scenes open.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{AppContext as _, Entity, HeadlessAppContext, px, size};
use pr_model::{PullRequestKey, RepoSlug};
use review_model::ReviewVerdict;
use settings_model::Appearance;
use settings_ui::SettingsPageKind;
use workspace_ui::{Workspace, launch};

/// Every screen the tool can render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scene {
    List,
    Thread,
    /// The Approve confirmation popover.
    ApprovePopover,
    /// The merge panel, open from the title bar.
    MergePopover,
    /// A pull request row's context menu.
    RowMenu,
    /// The list's Filter menu, open from the title bar.
    FilterMenu,
    /// A pinned repository's context menu, with the color grid.
    RepoMenu,
    /// A file header's context menu.
    FileMenu,
    /// One commit picked in the commit menu.
    OneCommit,
    /// The Request review dialog.
    RequestReview,
    /// The pull request popover from the title bar.
    TitlePopover,
    /// The thread with a picked code font and size.
    ThreadCodeFont,
    /// The thread in the Espresso dark and Islands Light palettes.
    ThreadEspresso,
    /// A description using most of GitHub Flavored Markdown.
    Markdown,
    /// The Unresolved filter: findings in triage order, with their tags.
    Unresolved,
    /// The thread scrolled to its first file.
    ThreadFirstFile,
    /// The review split side by side, then the right pane split top and bottom.
    SplitPanes,
    Submit,
    StartAgent,
    CheckLog,
    SignIn,
    Accounts,
    Repositories,
    Agents,
    /// Settings › Triage.
    Triage,
    Appearance,
    Keyboard,
    Palette,
    /// Go to file with a fuzzy query typed.
    GoToFile,
    AgentRun,
    /// The Reviews tab during the mock agent's run.
    ReviewsTab,
    /// A real review by the default agent, run to the end. Only with `--live`, and only
    /// when named, since it uses the agent's quota.
    LiveReview,
}

impl Scene {
    const ALL: [Scene; 32] = [
        Self::List,
        Self::OneCommit,
        Self::RowMenu,
        Self::FilterMenu,
        Self::RepoMenu,
        Self::FileMenu,
        Self::RequestReview,
        Self::Thread,
        Self::ApprovePopover,
        Self::MergePopover,
        Self::TitlePopover,
        Self::ThreadCodeFont,
        Self::ThreadEspresso,
        Self::Markdown,
        Self::Unresolved,
        Self::ThreadFirstFile,
        Self::SplitPanes,
        Self::Submit,
        Self::StartAgent,
        Self::CheckLog,
        Self::SignIn,
        Self::Accounts,
        Self::Repositories,
        Self::Agents,
        Self::Triage,
        Self::Appearance,
        Self::Keyboard,
        Self::Palette,
        Self::GoToFile,
        Self::AgentRun,
        Self::ReviewsTab,
        Self::LiveReview,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::List => "01-list",
            Self::Thread => "02-thread",
            Self::ApprovePopover => "03b-approve-popover",
            Self::MergePopover => "03e-merge-popover",
            Self::RowMenu => "01c-row-menu",
            Self::FilterMenu => "01d-filter-menu",
            Self::RepoMenu => "01e-repo-menu",
            Self::FileMenu => "02h-file-menu",
            Self::OneCommit => "02i-one-commit",
            Self::RequestReview => "03c-request-review",
            Self::TitlePopover => "02e-title-popover",
            Self::ThreadCodeFont => "02c-thread-code-font",
            Self::ThreadEspresso => "02g-thread-palettes",
            Self::Markdown => "02k-markdown",
            Self::Unresolved => "02l-unresolved",
            Self::ThreadFirstFile => "02d-thread-first-file",
            Self::SplitPanes => "02m-split-panes",
            Self::Submit => "03-submit",
            Self::StartAgent => "05a-start-agent",
            Self::CheckLog => "04-check-log",
            Self::SignIn => "06-sign-in",
            Self::Accounts => "07-accounts",
            Self::Repositories => "07b-repositories",
            Self::Agents => "07c-agents",
            Self::Triage => "07f-triage",
            Self::Appearance => "07d-appearance",
            Self::Keyboard => "07e-keyboard",
            Self::Palette => "08-palette",
            Self::GoToFile => "08b-go-to-file",
            Self::AgentRun => "05-agent-running",
            Self::ReviewsTab => "05b-reviews-tab",
            Self::LiveReview => "09-live-review",
        }
    }
}

fn featured() -> PullRequestKey {
    PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482)
}

fn render(
    scene: Scene,
    appearance: Appearance,
    live: bool,
    pull_request: Option<&PullRequestKey>,
    out: &Path,
) -> anyhow::Result<PathBuf> {
    let text_system = gpui_kit::platform::current_platform(true).text_system();
    let mut cx =
        HeadlessAppContext::with_platform(text_system, Arc::new(widgets_ui::AppAssets), || {
            gpui_kit::platform::current_headless_renderer()
        });
    let home = if live {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
    } else {
        PathBuf::from("/Users/denis")
    };
    cx.update(|cx| {
        gpui_kit::init(cx);
        pr_list_ui::init(cx);
        review_thread_ui::init(cx);
        workspace_ui::init(cx);
        let mut data = if live {
            launch::live_data(&home)
        } else {
            launch::sample_data(&home)
        };
        data.settings_path = None;
        if scene == Scene::Markdown {
            let mut sample = fixtures::sample(pr_model::Timestamp::now(), &home);
            if let Some(detail) = sample
                .details
                .iter_mut()
                .find(|detail| detail.summary.key == featured())
            {
                detail.body = fixtures::KITCHEN_SINK.into();
                detail.timeline = vec![review_model::TimelineItem::Description];
            }
            data.source = Arc::new(github_api::FixtureSource::new(
                sample.pull_requests,
                sample.details,
            ));
        }
        if scene == Scene::SignIn {
            data.settings.accounts.clear();
        }
        launch::install_stores(data, Some(home.clone()), cx);
        // Sample scenes show triage answers from the rule-based provider; live ones
        // use whatever the reviewer configured.
        if !live && let Some(store) = triage_ui::TriageStoreExt::triage(&*cx) {
            let provider: Arc<dyn triage::TriageProvider> =
                Arc::new(fixtures::SampleTriage::default());
            store.update(cx, |store, cx| store.set_provider(Some(provider), cx));
        }
        let mut look = settings_model::Preferences::default();
        look.appearance = appearance;
        if matches!(
            scene,
            Scene::List | Scene::RowMenu | Scene::FilterMenu | Scene::RepoMenu
        ) {
            look.set_pinned("flaticols/switchfold", true);
            look.set_repository_color(
                "flaticols/switchfold",
                Some(settings_model::RepositoryColor::Blue),
            );
            look.set_repository_color(
                "flaticols/gathering-app",
                Some(settings_model::RepositoryColor::Green),
            );
            look.group_repositories_by_owner = scene == Scene::FilterMenu;
            use app_state_ui::StoresExt as _;
            let settings = cx.stores().settings().clone();
            let saved = look.clone();
            settings.update(cx, |settings, cx| {
                settings.update_preferences(cx, |p| *p = saved)
            });
        }
        if scene == Scene::ThreadEspresso {
            look.dark_theme = settings_model::DarkTheme::Espresso;
            look.light_theme = settings_model::LightTheme::IslandsLight;
        }
        if scene == Scene::ThreadCodeFont {
            look.code_font_family = Some("Courier New".into());
            look.code_font_size = Some(15.);
        }
        theme_ui::apply(&look, None, cx);
    });

    let workspace: Rc<RefCell<Option<Entity<Workspace>>>> = Rc::default();
    let built = workspace.clone();
    // The markdown sample is long; a tall window shows all of it.
    let height = if scene == Scene::Markdown {
        2000.
    } else {
        900.
    };
    let window = cx.open_window(size(px(1440.), px(height)), move |window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        *built.borrow_mut() = Some(view.clone());
        cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
    })?;
    cx.run_until_parked();
    let workspace = workspace.borrow().clone().expect("workspace was built");

    let target = if let Some(key) = pull_request {
        key.clone()
    } else if live {
        cx.update(|cx| {
            use app_state_ui::StoresExt as _;
            let store = cx.stores().pull_requests().read(cx);
            store
                .pull_requests()
                .iter()
                .find(|pr| pr.is_in(pr_model::Inbox::ReviewRequested))
                .or(store.pull_requests().first())
                .map(|pr| pr.key.clone())
        })
        .unwrap_or_else(featured)
    } else {
        featured()
    };
    cx.update_window(window.into(), |_, window, cx| {
        let opens_review = matches!(
            scene,
            Scene::AgentRun | Scene::ReviewsTab | Scene::LiveReview
        ) || matches!(
            scene,
            Scene::Thread
                | Scene::ApprovePopover
                | Scene::MergePopover
                | Scene::FileMenu
                | Scene::OneCommit
                | Scene::RequestReview
                | Scene::TitlePopover
                | Scene::ThreadCodeFont
                | Scene::ThreadEspresso
                | Scene::Markdown
                | Scene::Unresolved
                | Scene::ThreadFirstFile
                | Scene::SplitPanes
                | Scene::GoToFile
                | Scene::Submit
                | Scene::StartAgent
                | Scene::CheckLog
        );
        if opens_review {
            workspace.update(cx, |workspace, cx| {
                workspace.open_pull_request(target.clone(), window, cx)
            });
        }
        let page = match scene {
            Scene::Accounts => Some(SettingsPageKind::Accounts),
            Scene::Repositories => Some(SettingsPageKind::Repositories),
            Scene::Agents => Some(SettingsPageKind::AgentsAndEditor),
            Scene::Triage => Some(SettingsPageKind::Triage),
            Scene::Appearance => Some(SettingsPageKind::Appearance),
            Scene::Keyboard => Some(SettingsPageKind::Keyboard),
            _ => None,
        };
        if let Some(page) = page {
            workspace.update(cx, |workspace, cx| {
                workspace.open_settings(page, window, cx)
            });
        }
    })?;
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| match scene {
        Scene::Submit => submit_review_ui::open(
            target.clone(),
            Some(ReviewVerdict::RequestChanges),
            window,
            cx,
        ),
        Scene::StartAgent => {
            if let Some(review) = workspace.read(cx).review_view(cx) {
                review.update(cx, |review, cx| review.open_agent_review(window, cx));
            }
        }
        Scene::CheckLog => check_log_ui::open(
            featured(),
            fixtures::SNAPSHOT_CHECK_ID.to_string(),
            window,
            cx,
        ),
        Scene::Palette => window.dispatch_action(Box::new(app_state_ui::actions::FocusSearch), cx),
        Scene::GoToFile => {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            window.press("alt-shift-a", cx);
            window.render_frame(cx);
            window.input("rndr", cx);
        }
        Scene::RowMenu => {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            window.right_click("flaticols/switchfold#482", cx);
        }
        Scene::FilterMenu => {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            window.click("list-filter", cx);
        }
        Scene::RepoMenu => {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            // Sidebar rows are named section-row: section 1 is Pinned.
            window.right_click("1-0", cx);
        }
        Scene::FileMenu => {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            window
                .within(gpui_kit::SharedString::from(
                    "file-crates/review/src/thread.rs",
                ))
                .right_click("toggle", cx);
        }
        Scene::OneCommit => {
            if let Some(review) = workspace.read(cx).review_view(cx) {
                review.update(cx, |review, cx| {
                    let sha = review
                        .detail()
                        .and_then(|detail| detail.commits.first())
                        .map(|commit| commit.sha.clone());
                    if let Some(sha) = sha {
                        review.show_commit(sha, cx);
                    }
                });
            }
        }
        Scene::RequestReview => {
            pr_actions_ui::open_request_review(target.clone(), window, cx);
        }
        Scene::ApprovePopover | Scene::TitlePopover | Scene::MergePopover => {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            window.click(
                match scene {
                    Scene::ApprovePopover => "approve",
                    Scene::MergePopover => "merge",
                    _ => "pull-request-title",
                },
                cx,
            );
        }
        Scene::SplitPanes => {
            use gpui_kit::test::TestWindowExt as _;
            window.render_frame(cx);
            for key in ["ctrl-b", "%", "ctrl-b", "\""] {
                window.press(key, cx);
            }
            // Each pane on another file, as a reviewer comparing them would.
            if let Some(panes) = workspace.read(cx).review_panes() {
                let panes = panes.read(cx).panes();
                let files: Vec<String> = panes
                    .first()
                    .and_then(|pane| pane.read(cx).detail())
                    .map(|detail| detail.files.iter().map(|file| file.path.clone()).collect())
                    .unwrap_or_default();
                for (pane, path) in panes.iter().zip(files.iter().skip(1)) {
                    pane.update(cx, |pane, cx| pane.reveal_file(path, cx));
                }
            }
        }
        Scene::ThreadFirstFile => {
            if let Some(review) = workspace.read(cx).review_view(cx) {
                // With the sample data the first file holds tanya's thread and its reply box.
                review.update(cx, |review, cx| {
                    let first = review
                        .detail()
                        .and_then(|detail| detail.files.first())
                        .map(|file| file.path.clone());
                    if let Some(path) = first {
                        review.reveal_file(&path, cx);
                    }
                });
            }
        }
        Scene::AgentRun | Scene::ReviewsTab => {
            let bin = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
                .unwrap_or_default();
            let mut agent = settings_model::AgentConfig::new(
                "mock",
                "Claude Code",
                bin.join("acp-mock-agent").display().to_string(),
                Vec::new(),
            );
            agent.model = Some("deep".into());
            agent.effort = Some("high".into());
            let options = agent_review_ui::StartOptions::new()
                .with_findings_server(bin.join("acp-findings-server"));
            agent_review_ui::start_review(target.clone(), agent, options.clone(), window, cx);
            if scene == Scene::ReviewsTab {
                let mut codex = settings_model::AgentConfig::new(
                    "mock-codex",
                    "Codex",
                    bin.join("acp-mock-agent").display().to_string(),
                    Vec::new(),
                );
                codex.model = Some("unsupported".into());
                agent_review_ui::start_review(target.clone(), codex, options, window, cx);
            }
        }
        _ => {}
    })?;
    cx.run_until_parked();
    if scene == Scene::StartAgent {
        // The dialog asks the real agent for its models, which takes a few seconds.
        for _ in 0..150 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            cx.run_until_parked();
        }
    }
    if scene == Scene::LiveReview {
        let started = cx.update_window(window.into(), |_, window, cx| {
            use app_state_ui::StoresExt as _;
            let agent = cx.stores().settings().read(cx).default_agent().cloned();
            agent.is_some_and(|agent| {
                agent_review_ui::start_review(
                    target.clone(),
                    agent,
                    agent_review_ui::StartOptions::new(),
                    window,
                    cx,
                )
            })
        })?;
        anyhow::ensure!(started, "the review couldn't start");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15 * 60);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            cx.run_until_parked();
            let over = cx.update(|cx| {
                let runs = agent_review_ui::runs_for(&target, cx);
                let Some(run) = runs.first() else {
                    return true;
                };
                if run.read(cx).permission().is_some() {
                    run.update(cx, |run, cx| run.answer(acp_client::Decision::Deny, cx));
                }
                let run = run.read(cx);
                if run.state().is_over() {
                    eprintln!(
                        "{:?} after {:?}; transcript {:?}",
                        run.state(),
                        run.elapsed(),
                        run.log_path()
                    );
                    true
                } else {
                    false
                }
            });
            if over || std::time::Instant::now() > deadline {
                break;
            }
        }
        cx.update_window(window.into(), |_, _, cx| {
            if let Some(review) = workspace.read(cx).review_view(cx) {
                review.update(cx, |review, cx| {
                    review.show_filter(review_thread_ui::TimelineFilter::Reviews, cx)
                });
            }
        })?;
    }
    if scene == Scene::Unresolved {
        cx.run_until_parked();
        cx.update_window(window.into(), |_, _, cx| {
            if let Some(review) = workspace.read(cx).review_view(cx) {
                review.update(cx, |review, cx| {
                    review.show_filter(review_thread_ui::TimelineFilter::Unresolved, cx)
                });
            }
        })?;
    }
    if matches!(scene, Scene::AgentRun | Scene::ReviewsTab) {
        // The mock agent runs as a real process; give it time to read files and ask.
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            cx.run_until_parked();
        }
        cx.update_window(window.into(), |_, _, cx| {
            if let Some(review) = workspace.read(cx).review_view(cx) {
                review.update(cx, |review, cx| {
                    if scene == Scene::ReviewsTab {
                        review.show_filter(review_thread_ui::TimelineFilter::Reviews, cx)
                    } else {
                        review.scroll_to_end(cx)
                    }
                });
            }
        })?;
    }
    // Overlay open animations run on wall-clock time; let them finish before capturing.
    for _ in 0..4 {
        std::thread::sleep(std::time::Duration::from_millis(150));
        cx.update_window(window.into(), |_, window, _| window.refresh())?;
        cx.run_until_parked();
    }

    let image = cx.capture_screenshot(window.into())?;
    let mode = match appearance {
        Appearance::Dark => "dark",
        _ => "light",
    };
    let source = if live { "-live" } else { "" };
    let path = out.join(format!("{}{source}-{mode}.png", scene.name()));
    image.save(&path)?;
    Ok(path)
}

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let live = args.iter().any(|arg| arg == "--live");
    args.retain(|arg| arg != "--live");
    let pull_request = match args.iter().position(|arg| arg == "--pr") {
        Some(ix) => {
            args.remove(ix);
            let arg = args.remove(ix);
            Some(
                launch::parse_pull_request_arg(&arg)
                    .ok_or_else(|| anyhow::anyhow!("--pr takes owner/name#number, not {arg}"))?,
            )
        }
        None => None,
    };
    let mut args = args.into_iter();
    let out = PathBuf::from(args.next().unwrap_or_else(|| "target/screenshots".into()));
    let only: Vec<String> = args.collect();
    std::fs::create_dir_all(&out)?;
    for appearance in [Appearance::Light, Appearance::Dark] {
        for scene in Scene::ALL {
            if !only.is_empty() && !only.iter().any(|name| scene.name().contains(name.as_str())) {
                continue;
            }
            if scene == Scene::LiveReview
                && (!live
                    || !only.iter().any(|n| n == "09-live-review")
                    || appearance == Appearance::Dark)
            {
                continue;
            }
            let path = render(scene, appearance, live, pull_request.as_ref(), &out)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

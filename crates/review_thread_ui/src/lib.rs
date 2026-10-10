//! The pull request thread: a file and thread outline, one scroll with the
//! description, pushes, diffs, comments, agent findings and checks in the order they
//! happened, the pending-review bar, and an inspector with reviewers, checks, agents
//! and dependencies.

mod cards;
mod commits;
mod filters;
mod inspector;
mod menus;
mod merge;
mod outline;
mod quote;
mod review_actions;
mod rows;
mod timeline;
mod title;
mod triage;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use app_state_ui::actions::{
    CheckOut, GoBack, OpenInZed, ReviewWithAgent, SubmitComment, SubmitReview,
};
use app_state_ui::{DetailState, PullRequestStore, PullRequestStoreEvent, StoresExt as _};
use diff_view_ui::{COLLAPSE_THRESHOLD, DiffMode};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, WindowExt as _, h_flex,
    input::{InputEvent, TextareaState},
    resizable::{h_resizable, resizable_panel},
    tree::{TreeEvent, TreeState},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ListAlignment, ListOffset, ListState,
    ParentElement as _, Pixels, Render, SharedString, Styled as _, Subscription, WeakEntity,
    Window, div, px,
};
use pr_model::PullRequestKey;
use review_model::{FindingState, PullRequestDetail, TimelineItem};
use syntax_ui::FileHighlights;
use widgets_ui::EmptyState;

pub use filters::TimelineFilter;

/// Which of a review's sidebars show. Shared by every review, so the choice stays when
/// another pull request opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewLayout {
    pub outline: bool,
    pub inspector: bool,
    /// The sidebars focus mode hid, to bring back when it ends.
    before_focus: Option<(bool, bool)>,
}

impl Default for ReviewLayout {
    fn default() -> Self {
        Self {
            outline: true,
            inspector: true,
            before_focus: None,
        }
    }
}

impl gpui_kit::Global for ReviewLayout {}

impl ReviewLayout {
    /// The current layout.
    pub fn get(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }

    /// Whether focus mode hid the sidebars.
    pub fn is_focused(&self) -> bool {
        self.before_focus.is_some()
    }

    fn update(cx: &mut App, change: impl FnOnce(&mut Self)) {
        change(cx.default_global::<Self>());
        cx.refresh_windows();
    }

    /// Shows or hides the file and thread sidebar.
    pub fn toggle_outline(cx: &mut App) {
        Self::update(cx, |layout| {
            layout.outline = !layout.outline;
            layout.before_focus = None;
        });
    }

    /// Shows or hides the reviewers and checks sidebar.
    pub fn toggle_inspector(cx: &mut App) {
        Self::update(cx, |layout| {
            layout.inspector = !layout.inspector;
            layout.before_focus = None;
        });
    }

    /// Hides both sidebars, or brings back the ones focus mode hid.
    pub fn toggle_focus(cx: &mut App) {
        Self::update(cx, |layout| match layout.before_focus.take() {
            Some((outline, inspector)) => {
                layout.outline = outline;
                layout.inspector = inspector;
            }
            None => {
                layout.before_focus = Some((layout.outline, layout.inspector));
                layout.outline = false;
                layout.inspector = false;
            }
        });
    }
}
pub use review_actions::ReviewActions;
pub use title::PullRequestTitle;

const CONTEXT: &str = "ReviewThread";

/// Registers the thread's key bindings. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", GoBack, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-o", OpenInZed, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-enter", SubmitReview, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-a", ReviewWithAgent, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-c", CheckOut, Some(CONTEXT)),
        KeyBinding::new("cmd-enter", SubmitComment, Some(CONTEXT)),
    ]);
}

/// Where a thread's diffs came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffSource {
    /// GitHub's per-file patches.
    GitHub,
    /// Computed with gix from the mapped local clone.
    LocalClone,
}

/// What the thread asks its owner to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewThreadEvent {
    /// Return to the pull request list.
    Back,
    /// Open another pull request, such as one this depends on.
    Open(PullRequestKey),
}

/// The review screen for one pull request.
pub struct ReviewThreadView {
    key: PullRequestKey,
    back_label: SharedString,
    focus_handle: FocusHandle,
    detail: Option<Rc<PullRequestDetail>>,
    load_error: Option<SharedString>,
    /// Where the diffs came from, once a local clone was tried.
    diff_source: Option<DiffSource>,
    /// The head commit the diffs were last computed for.
    diffed_head: Option<String>,
    filter: TimelineFilter,
    mode: DiffMode,
    collapsed: HashSet<String>,
    collapse_initialized: bool,
    highlights: HashMap<(String, bool), Rc<FileHighlights>>,
    files: HashMap<String, Rc<diff_model::FileDiff>>,
    composer: Entity<TextareaState>,
    reply_inputs: HashMap<String, Entity<TextareaState>>,
    new_thread: Option<(String, u32, Entity<TextareaState>)>,
    editing_finding: Option<(String, Entity<TextareaState>)>,
    /// The draft the reviewer focused last, where quotes and references from the diff go.
    last_draft: Option<WeakEntity<TextareaState>>,
    list_state: ListState,
    file_tree: Entity<TreeState>,
    file_tree_paths: Vec<String>,
    /// Folders the reviewer folded, by path, kept when the tree is rebuilt.
    folded_dirs: HashSet<String>,
    /// The file last revealed from the tree, so re-renders don't scroll to it again.
    tree_revealed: Option<String>,
    items: Rc<Vec<TimelineItem>>,
    /// Agent runs the Reviews tab shows, newest first.
    runs: Vec<Entity<agent_review_ui::AgentRun>>,
    /// The Review with an agent form, made the first time the Reviews tab shows it.
    start_form: Option<Entity<agent_review_ui::StartReviewForm>>,
    merge_form: Option<Entity<merge::MergeForm>>,
    /// Whether the form is open; `None` until the reviewer opens or hides it.
    start_form_open: Option<bool>,
    /// The single commit the timeline shows, when one is picked.
    commit: Option<commits::CommitView>,
    /// Agent runs in progress, shown as cards at the end of the other tabs.
    active_runs: Vec<Entity<agent_review_ui::AgentRun>>,
    rows: Rc<Vec<rows::TimelineRow>>,
    /// The rem size the rows were measured at; a zoom remeasures them.
    measured_rem: Option<Pixels>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ReviewThreadEvent> for ReviewThreadView {}

impl Focusable for ReviewThreadView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ReviewThreadView {
    /// Opens the thread for `key`; `back_label` names the list the Back button returns to.
    pub fn new(
        key: PullRequestKey,
        back_label: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = cx.stores().pull_requests().clone();
        store.update(cx, |store, cx| store.load_detail(&key, cx));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Leave a comment… use @ to mention people or agents")
                .rows(2)
        });
        let file_tree = cx.new(|cx| TreeState::new(cx));
        let mut subscriptions = vec![
            cx.observe(&file_tree, |this: &mut Self, _, cx| {
                this.on_file_tree_changed(cx)
            }),
            cx.subscribe(
                &file_tree,
                |this: &mut Self, _, event: &TreeEvent, _| match event {
                    TreeEvent::Collapsed(id) => {
                        this.folded_dirs
                            .insert(id.trim_end_matches('/').to_string());
                    }
                    TreeEvent::Expanded(id) => {
                        this.folded_dirs.remove(id.trim_end_matches('/'));
                    }
                },
            ),
            cx.subscribe_in(
                &store,
                window,
                |this: &mut Self, _, event: &PullRequestStoreEvent, window, cx| {
                    if let PullRequestStoreEvent::DetailChanged(key) = event
                        && key == &this.key
                    {
                        this.sync_detail(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &composer,
                window,
                |this: &mut Self, composer, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter {
                        secondary: true, ..
                    } => this.post_comment(window, cx),
                    InputEvent::Focus => this.last_draft = Some(composer.downgrade()),
                    _ => {}
                },
            ),
        ];
        // Triage answers arrive after the detail; tags and the Unresolved order follow.
        if let Some(triage) = triage_ui::TriageStoreExt::triage(&**cx) {
            subscriptions.push(cx.observe(&triage, |this: &mut Self, _, cx| {
                this.sync_rows(cx);
                cx.notify();
            }));
        }
        let mut view = Self {
            key,
            back_label: back_label.into(),
            focus_handle: cx.focus_handle(),
            detail: None,
            load_error: None,
            diff_source: None,
            diffed_head: None,
            filter: TimelineFilter::Everything,
            mode: DiffMode::Unified,
            collapsed: HashSet::new(),
            collapse_initialized: false,
            highlights: HashMap::new(),
            files: HashMap::new(),
            composer,
            reply_inputs: HashMap::new(),
            new_thread: None,
            editing_finding: None,
            last_draft: None,
            list_state: ListState::new(0, ListAlignment::Top, px(800.)),
            file_tree,
            file_tree_paths: Vec::new(),
            folded_dirs: HashSet::new(),
            tree_revealed: None,
            items: Rc::default(),
            runs: Vec::new(),
            active_runs: Vec::new(),
            commit: None,
            start_form: None,
            merge_form: None,
            start_form_open: None,
            rows: Rc::default(),
            measured_rem: None,
            _subscriptions: subscriptions,
        };
        view.sync_detail(window, cx);
        view
    }

    /// The pull request shown.
    pub fn key(&self) -> &PullRequestKey {
        &self.key
    }

    /// Scrolls the timeline to its end, where running agent reviews and the composer sit.
    pub fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
        self.sync_rows(cx);
        self.list_state.scroll_to_end();
        cx.notify();
    }

    /// The loaded review data, once available.
    pub fn detail(&self) -> Option<&PullRequestDetail> {
        self.detail.as_deref()
    }

    /// Copies the store's detail into the view and prepares per-thread inputs.
    fn sync_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx
            .stores()
            .pull_requests()
            .read(cx)
            .detail(&self.key)
            .cloned();
        match state {
            Some(DetailState::Loaded(detail)) => {
                if !self.collapse_initialized {
                    self.collapsed = filters::initially_collapsed(&detail, COLLAPSE_THRESHOLD)
                        .into_iter()
                        .collect();
                    self.collapse_initialized = true;
                }
                // A picked commit keeps its own files until the reviewer goes back.
                if self.commit.is_none() {
                    self.files = detail
                        .files
                        .iter()
                        .map(|file| (file.path.clone(), Rc::new(file.clone())))
                        .collect();
                    self.highlights.clear();
                }
                for thread in &detail.threads {
                    if !self.reply_inputs.contains_key(&thread.id) {
                        let author = thread
                            .starter()
                            .map(|actor| actor.login.clone())
                            .unwrap_or_default();
                        let input = self.new_reply_input(&thread.id, &author, window, cx);
                        self.reply_inputs.insert(thread.id.clone(), input);
                    }
                }
                let head = detail.commits.last().map(|commit| commit.sha.clone());
                if self.diff_source.is_none() || self.diffed_head != head {
                    self.diffed_head = head;
                    self.diff_locally(&detail, cx);
                }
                self.sync_file_tree(&detail, cx);
                let reloaded = self.detail.is_some();
                self.detail = Some(Rc::new(*detail));
                self.load_error = None;
                if reloaded {
                    self.list_state.remeasure();
                }
            }
            Some(DetailState::Failed(error)) => self.load_error = Some(error.into()),
            Some(DetailState::Loading) | None => {}
        }
        cx.notify();
    }

    /// Recomputes the diffs with gix when the pull request's head commit is in the mapped
    /// clone, so they match the working tree and aren't truncated like GitHub patches.
    fn diff_locally(&mut self, detail: &PullRequestDetail, cx: &mut Context<Self>) {
        let clone = cx
            .stores()
            .settings()
            .read(cx)
            .clone_path(self.key.repo())
            .map(|path| path.to_path_buf())
            .filter(|path| path.is_dir());
        let (Some(clone), Some(head)) = (clone, detail.commits.last().map(|c| c.sha.clone()))
        else {
            self.diff_source = Some(DiffSource::GitHub);
            return;
        };
        self.diff_source = Some(DiffSource::GitHub);
        let base = format!("origin/{}", detail.summary.base_ref);
        let key = self.key.clone();
        let task = cx.background_spawn(async move {
            let local = git_diff::LocalClone::open(&clone).ok()?;
            if !local.has_revision(&head) || !local.has_revision(&base) {
                return None;
            }
            local.diff_revisions(&base, &head).ok()
        });
        cx.spawn(async move |this, cx| {
            let Some(files) = task.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.diff_source = Some(DiffSource::LocalClone);
                this.store(cx)
                    .update(cx, |store, cx| store.replace_files(&key, files, cx));
            })
            .ok();
        })
        .detach();
    }

    /// Where the diffs shown came from.
    pub fn diff_source(&self) -> Option<DiffSource> {
        self.diff_source
    }

    fn new_reply_input(
        &mut self,
        thread_id: &str,
        author: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        let input = editor(format!("Reply to {author}…"), None, window, cx);
        let thread_id = thread_id.to_string();
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this: &mut Self, input, event: &InputEvent, window, cx| {
                this.note_focus(input, event);
                if submits(event) {
                    this.submit_reply(&thread_id, window, cx);
                }
            },
        );
        self._subscriptions.push(subscription);
        input
    }

    /// Remembers `input` as the draft quotes go to once the reviewer focuses it.
    fn note_focus(&mut self, input: &Entity<TextareaState>, event: &InputEvent) {
        if matches!(event, InputEvent::Focus) {
            self.last_draft = Some(input.downgrade());
        }
    }

    /// Posts the reply typed under thread `thread_id` and clears the field.
    pub(crate) fn submit_reply(
        &mut self,
        thread_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.reply_inputs.get(thread_id).cloned() else {
            return;
        };
        let body = input.read(cx).value().trim().to_string();
        if body.is_empty() {
            return;
        }
        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.reply(thread_id, body, cx);
    }

    fn viewer(&self, cx: &App) -> review_model::Actor {
        cx.stores().settings().read(cx).viewer_actor()
    }

    fn store(&self, cx: &App) -> Entity<PullRequestStore> {
        cx.stores().pull_requests().clone()
    }

    fn reply(&mut self, thread_id: &str, body: String, cx: &mut Context<Self>) {
        let viewer = self.viewer(cx);
        let key = self.key.clone();
        self.store(cx).update(cx, |store, cx| {
            store.reply(&key, thread_id, &viewer, body, cx)
        });
    }

    fn post_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.composer.read(cx).value().trim().to_string();
        if body.is_empty() {
            return;
        }
        let viewer = self.viewer(cx);
        let key = self.key.clone();
        self.store(cx)
            .update(cx, |store, cx| store.comment(&key, &viewer, body, cx));
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        self.sync_rows(cx);
        self.list_state.scroll_to_end();
    }

    fn start_line_comment(
        &mut self,
        path: String,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = editor("Comment on this line…".into(), None, window, cx);
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, _, cx| {
                this.note_focus(input, event);
                if submits(event) {
                    this.submit_line_comment(cx);
                }
            },
        );
        self._subscriptions.push(subscription);
        input.update(cx, |input, cx| input.focus(window, cx));
        self.new_thread = Some((path, line, input));
        cx.notify();
    }

    /// Starts a pending thread with the line comment being written.
    pub(crate) fn submit_line_comment(&mut self, cx: &mut Context<Self>) {
        let Some((path, line, input)) = self.new_thread.take() else {
            return;
        };
        let body = input.read(cx).value().trim().to_string();
        if body.is_empty() {
            self.new_thread = Some((path, line, input));
            return;
        }
        let viewer = self.viewer(cx);
        let key = self.key.clone();
        self.store(cx).update(cx, |store, cx| {
            store.start_thread(&key, &path, line, &viewer, body, cx)
        });
        cx.notify();
    }

    fn set_viewed(&mut self, path: &str, viewed: bool, cx: &mut Context<Self>) {
        if viewed {
            self.collapsed.insert(path.to_string());
        } else {
            self.collapsed.remove(path);
        }
        let key = self.key.clone();
        self.store(cx)
            .update(cx, |store, cx| store.set_viewed(&key, path, viewed, cx));
    }

    fn set_finding_state(&mut self, id: &str, state: FindingState, cx: &mut Context<Self>) {
        let key = self.key.clone();
        self.store(cx)
            .update(cx, |store, cx| store.set_finding_state(&key, id, state, cx));
    }

    fn edit_finding(&mut self, id: &str, body: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = editor(String::new(), Some(body), window, cx);
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, _, cx| {
                this.note_focus(input, event);
                if submits(event) {
                    this.save_finding_edit(cx);
                }
            },
        );
        self._subscriptions.push(subscription);
        input.update(cx, |input, cx| input.focus(window, cx));
        self.editing_finding = Some((id.to_string(), input));
        cx.notify();
    }

    /// Saves the finding being edited.
    pub(crate) fn save_finding_edit(&mut self, cx: &mut Context<Self>) {
        let Some((id, input)) = self.editing_finding.take() else {
            return;
        };
        let body = input.read(cx).value().trim().to_string();
        if !body.is_empty() {
            let key = self.key.clone();
            self.store(cx)
                .update(cx, |store, cx| store.edit_finding(&key, &id, body, cx));
        }
        cx.notify();
    }

    fn highlights_for(&mut self, path: &str, cx: &App) -> Rc<FileHighlights> {
        let dark = cx.theme().is_dark();
        if let Some(highlights) = self.highlights.get(&(path.to_string(), dark)) {
            return highlights.clone();
        }
        let highlights = self
            .files
            .get(path)
            .map(|file| {
                Rc::new(syntax_ui::highlight_file(
                    file,
                    &theme_ui::ActiveAppTheme::app_theme(cx).clone(),
                ))
            })
            .unwrap_or_default();
        self.highlights
            .insert((path.to_string(), dark), highlights.clone());
        highlights
    }

    /// Scrolls to `path`, expanding it and switching to Changes when the current
    /// filter does not show it.
    pub fn reveal_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.clone() else {
            return;
        };
        self.collapsed.remove(path);
        let shown = filters::items(&detail, self.filter)
            .iter()
            .any(|item| matches!(item, TimelineItem::File { path: p } if p == path));
        if !shown {
            self.set_filter(TimelineFilter::Changes);
        }
        self.sync_rows(cx);
        if let Some(item_ix) = self.header_row(path) {
            self.list_state.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
        }
        cx.notify();
    }

    /// Switches the timeline to `filter`, as the toolbar tabs do.
    pub fn show_filter(&mut self, filter: TimelineFilter, cx: &mut Context<Self>) {
        self.set_filter(filter);
        cx.notify();
    }

    /// Shows `filter` from the top.
    fn set_filter(&mut self, filter: TimelineFilter) {
        if self.filter == filter {
            return;
        }
        self.filter = filter;
        self.rows = Rc::default();
        self.list_state.reset(0);
    }

    pub(crate) fn open_in_zed(
        &mut self,
        path: Option<String>,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        pr_actions_ui::open_in_zed(&self.key, path.as_deref(), line, window, cx);
    }

    /// Opens a comment composer on head-side `line` of `path` and scrolls to it, as when
    /// Zed asks to comment on the line under its cursor. Returns `false` when the line
    /// isn't part of the diff, where GitHub doesn't take comments.
    pub fn comment_on_line(
        &mut self,
        path: &str,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.show_all_commits(cx);
        self.reveal_file(path, cx);
        self.start_line_comment(path.to_string(), line, window, cx);
        self.sync_rows(cx);
        let Some(row) = self.threads_row(path, line) else {
            self.new_thread = None;
            self.sync_rows(cx);
            return false;
        };
        // A few rows of the code above the composer keep the line in view.
        self.list_state.scroll_to(ListOffset {
            item_ix: row.saturating_sub(4),
            offset_in_item: px(0.),
        });
        cx.notify();
        true
    }

    fn ask_zed_agent(
        &mut self,
        path: &str,
        line: u32,
        subject: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        pr_actions_ui::ask_zed_agent(&self.key, path, line, subject, window, cx);
    }

    /// The pull request's title and state from the list, with placeholders where the
    /// conversation will appear, while the full review data loads.
    fn render_loading(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        use gpui_kit::component::skeleton::Skeleton;
        use theme_ui::TypographyExt as _;
        let summary = cx
            .stores()
            .pull_requests()
            .read(cx)
            .pull_request(&self.key)
            .cloned();
        let title = summary
            .as_ref()
            .map(|pr| SharedString::from(pr.title.clone()))
            .unwrap_or_default();
        v_flex()
            .size_full()
            .pt_5()
            .px_6()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_title().child(title))
                    .child(
                        div()
                            .text_title()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("#{}", self.key.number())),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .text_ui()
                    .text_color(cx.theme().muted_foreground)
                    .child(gpui_kit::component::spinner::Spinner::new())
                    .child("Loading the conversation, diffs and checks…"),
            )
            .child(
                v_flex()
                    .gap_3()
                    .pt_2()
                    .child(
                        Skeleton::new()
                            .h_24()
                            .w_full()
                            .rounded(cx.theme().radius_lg),
                    )
                    .child(Skeleton::new().h_6().w(gpui_kit::rems(30.)))
                    .child(Skeleton::new().h_6().w(gpui_kit::rems(24.)))
                    .child(
                        Skeleton::new()
                            .h_64()
                            .w_full()
                            .rounded(cx.theme().radius_lg),
                    ),
            )
    }

    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(detail) = self.detail.clone() else {
            return match &self.load_error {
                Some(error) => EmptyState::new(
                    IconName::CircleAlert,
                    "Couldn’t load this pull request",
                    error.clone(),
                )
                .into_any_element(),
                None => self.render_loading(cx).into_any_element(),
            };
        };
        let rem = window.rem_size();
        let layout = ReviewLayout::get(cx);
        h_resizable("review-panels")
            .child(
                resizable_panel()
                    .visible(layout.outline)
                    .size(rem * 16.25)
                    .size_range(px(180.)..px(520.))
                    .child(self.render_outline(&detail, cx)),
            )
            .child(
                resizable_panel().child(
                    v_flex()
                        .size_full()
                        .min_w_0()
                        .child(self.render_toolbar(&detail, cx))
                        .child(self.render_timeline(window, cx)),
                ),
            )
            .child(
                resizable_panel()
                    .visible(layout.inspector)
                    .size(rem * 20.)
                    .size_range(px(240.)..px(480.))
                    .child(self.render_inspector(&detail, cx)),
            )
            .into_any_element()
    }
}

/// A multi-line editor for replies, line comments and finding edits: two rows to start,
/// growing with the text like the comment composer.
fn editor(
    placeholder: String,
    value: Option<&str>,
    window: &mut Window,
    cx: &mut Context<ReviewThreadView>,
) -> Entity<TextareaState> {
    cx.new(|cx| {
        let state = TextareaState::new(window, cx)
            .placeholder(placeholder)
            .auto_grow(2, 12);
        match value {
            Some(value) => state.default_value(value.to_string()),
            None => state,
        }
    })
}

/// ⌘↵ submits a multi-line editor; Enter adds a line.
fn submits(event: &InputEvent) -> bool {
    matches!(
        event,
        InputEvent::PressEnter {
            secondary: true,
            ..
        }
    )
}

impl Render for ReviewThreadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("review-thread")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .on_action(cx.listener(|this, _: &GoBack, window, cx| {
                // Escape inside the composer or a reply field must not close the review
                // and lose the draft; it leaves the field instead.
                if window.has_focused_input(cx) {
                    window.focus(&this.focus_handle, cx);
                    return;
                }
                cx.emit(ReviewThreadEvent::Back)
            }))
            .on_action(cx.listener(|this, _: &OpenInZed, window, cx| {
                this.open_in_zed(None, None, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SubmitComment, window, cx| this.post_comment(window, cx)),
            )
            .on_action(cx.listener(|this, _: &SubmitReview, window, cx| {
                this.open_submit_review(None, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReviewWithAgent, window, cx| {
                this.open_agent_review(window, cx)
            }))
            .on_action(cx.listener(|this, _: &CheckOut, window, cx| this.check_out(window, cx)))
            .child(self.render_body(window, cx))
    }
}

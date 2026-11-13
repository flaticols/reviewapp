use std::collections::HashMap;
use std::path::PathBuf;

use acp_client::{
    AgentSetting, PromptInput, ReviewDepth, ReviewRequest, ReviewScope, SessionChoices,
    SettingKind, review_prompt, unified_patch,
};
use app_state_ui::StoresExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IndexPath, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Textarea, TextareaState},
    label::Label,
    notification::Notification,
    radio::Radio,
    select::{Select, SelectEvent, SelectState},
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FontWeight, Global,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window,
    div, prelude::FluentBuilder as _,
};
use pr_model::PullRequestKey;
use review_model::{Actor, PullRequestDetail, TimelineItem};
use settings_model::{AgentConfig, display_path};
use theme_ui::{ActiveAppTheme as _, TypographyExt as _};

use crate::{AgentRun, RunSetup};
use widgets_ui::EditMenu;

/// The Review with an agent form at the top of the Reviews tab: agent, model and
/// effort, scope, depth and instructions, and Start review.
pub struct StartReviewForm {
    key: PullRequestKey,
    agents: Vec<AgentConfig>,
    agent_select: Entity<SelectState<Vec<SharedString>>>,
    scope: ReviewScope,
    depth: ReviewDepth,
    instructions: Entity<TextareaState>,
    drafts: bool,
    /// What the selected agent offers for the chosen model.
    settings: SettingsState,
    model: Option<String>,
    effort: Option<String>,
    model_select: Option<Entity<SelectState<Vec<SharedString>>>>,
    effort_select: Option<Entity<SelectState<Vec<SharedString>>>>,
    _probe: Option<Task<()>>,
    _select_subscriptions: Vec<Subscription>,
}

/// The agent's session settings, as far as the dialog knows them.
#[derive(Clone, Debug)]
enum SettingsState {
    Loading,
    Ready(Vec<AgentSetting>),
    Failed(String),
}

/// Settings read from agents this run of the app, by agent id and the model they were
/// read with; starting an agent to ask takes seconds.
#[derive(Default)]
struct AgentSettingsCache(HashMap<(String, Option<String>), Result<Vec<AgentSetting>, String>>);

impl Global for AgentSettingsCache {}

impl StartReviewForm {
    /// A form for `key`, preselecting the default agent.
    pub fn new(key: PullRequestKey, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.stores().settings().read(cx);
        let agents: Vec<AgentConfig> = settings
            .agents()
            .iter()
            .filter(|agent| agent.is_enabled)
            .cloned()
            .collect();
        let busy = |agent: &AgentConfig| crate::is_reviewing(&key, &agent.name, cx);
        let default_ix = settings
            .default_agent()
            .and_then(|default| agents.iter().position(|agent| agent.id == default.id))
            .filter(|ix| !busy(&agents[*ix]))
            .or_else(|| agents.iter().position(|agent| !busy(agent)));
        let drafts = !settings.preferences().agents_post_directly;
        let names: Vec<SharedString> = agents
            .iter()
            .map(|agent| agent.name.clone().into())
            .collect();
        let agent_select =
            cx.new(|cx| SelectState::new(names, default_ix.map(IndexPath::new), window, cx));
        cx.observe(&agent_select, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(
            &agent_select,
            window,
            |this, _, _: &SelectEvent<Vec<SharedString>>, window, cx| {
                this.agent_changed(window, cx)
            },
        )
        .detach();
        let instructions = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("What should the agent focus on? (optional)")
                .rows(2)
                .context_menu(false)
        });
        let mut form = Self {
            key,
            agents,
            agent_select,
            scope: ReviewScope::WholePullRequest,
            depth: ReviewDepth::Thorough,
            instructions,
            drafts,
            settings: SettingsState::Loading,
            model: None,
            effort: None,
            model_select: None,
            effort_select: None,
            _probe: None,
            _select_subscriptions: Vec::new(),
        };
        form.agent_changed(window, cx);
        form
    }

    /// Starts from the selected agent's last model and effort and reads what it offers.
    fn agent_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent = self.selected_agent(cx).cloned();
        self.model = agent.as_ref().and_then(|agent| agent.model.clone());
        self.effort = agent.as_ref().and_then(|agent| agent.effort.clone());
        self.load_settings(window, cx);
    }

    /// Reads the selected agent's settings for the chosen model, from the cache or by
    /// starting the agent.
    fn load_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(agent) = self.selected_agent(cx).cloned() else {
            self.show_settings(SettingsState::Ready(Vec::new()), window, cx);
            return;
        };
        let cache_key = (agent.id.clone(), self.model.clone());
        if let Some(cached) = cx
            .try_global::<AgentSettingsCache>()
            .and_then(|cache| cache.0.get(&cache_key))
            .cloned()
        {
            let state = match cached {
                Ok(settings) => SettingsState::Ready(settings),
                Err(error) => SettingsState::Failed(error),
            };
            self.show_settings(state, window, cx);
            return;
        }
        self.settings = SettingsState::Loading;
        let command_line = launch_command(&agent);
        let env = vec![("PATH".to_string(), tool_path::path_env())];
        let cwd = cx
            .stores()
            .settings()
            .read(cx)
            .clone_path(self.key.repo())
            .filter(|path| path.is_dir())
            .map(|path| path.to_path_buf())
            .unwrap_or_else(std::env::temp_dir);
        let choices = SessionChoices::new(self.model.clone(), None);
        let probe = cx.background_spawn(async move {
            acp_client::probe_settings(&command_line, &env, cwd, choices).await
        });
        self._probe = Some(cx.spawn_in(window, async move |this, cx| {
            let result = probe.await;
            this.update_in(cx, |this, window, cx| {
                cx.default_global::<AgentSettingsCache>()
                    .0
                    .insert(cache_key, result.clone());
                let state = match result {
                    Ok(settings) => SettingsState::Ready(settings),
                    Err(error) => SettingsState::Failed(error),
                };
                this.show_settings(state, window, cx);
            })
            .ok();
        }));
        cx.notify();
    }

    /// Shows `state`, rebuilding the model and effort menus around the chosen values.
    /// A saved value the agent no longer offers falls back to the agent's default.
    fn show_settings(&mut self, state: SettingsState, window: &mut Window, cx: &mut Context<Self>) {
        self._select_subscriptions.clear();
        self.model_select = None;
        self.effort_select = None;
        if let SettingsState::Ready(settings) = &state {
            let model = settings.iter().find(|s| s.kind == SettingKind::Model);
            let effort = settings.iter().find(|s| s.kind == SettingKind::Effort);
            if let Some(model) = model {
                let value = pick(model, self.model.as_deref());
                self.model = Some(value.clone());
                let select = self.choice_select(model, &value, window, cx);
                let choices = model.choices.clone();
                self._select_subscriptions.push(cx.subscribe_in(
                    &select,
                    window,
                    move |this, _, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                        let SelectEvent::Confirm(Some(label)) = event else {
                            return;
                        };
                        let Some(choice) =
                            choices.iter().find(|c| c.name.as_str() == label.as_ref())
                        else {
                            return;
                        };
                        if this.model.as_deref() != Some(choice.value.as_str()) {
                            this.model = Some(choice.value.clone());
                            this.load_settings(window, cx);
                        }
                    },
                ));
                self.model_select = Some(select);
            }
            match effort {
                Some(effort) => {
                    let value = pick(effort, self.effort.as_deref());
                    self.effort = Some(value.clone());
                    let select = self.choice_select(effort, &value, window, cx);
                    let choices = effort.choices.clone();
                    self._select_subscriptions.push(cx.subscribe_in(
                        &select,
                        window,
                        move |this, _, event: &SelectEvent<Vec<SharedString>>, _, cx| {
                            let SelectEvent::Confirm(Some(label)) = event else {
                                return;
                            };
                            if let Some(choice) =
                                choices.iter().find(|c| c.name.as_str() == label.as_ref())
                            {
                                this.effort = Some(choice.value.clone());
                                cx.notify();
                            }
                        },
                    ));
                    self.effort_select = Some(select);
                }
                None => self.effort = None,
            }
        }
        self.settings = state;
        cx.notify();
    }

    fn choice_select(
        &self,
        setting: &AgentSetting,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<Vec<SharedString>>> {
        let names: Vec<SharedString> = setting
            .choices
            .iter()
            .map(|choice| choice.name.clone().into())
            .collect();
        let selected = setting
            .choices
            .iter()
            .position(|choice| choice.value == value);
        cx.new(|cx| SelectState::new(names, selected.map(IndexPath::new), window, cx))
    }

    fn selected_agent(&self, cx: &App) -> Option<&AgentConfig> {
        let ix = self.agent_select.read(cx).selected_index(cx)?.row;
        self.agents.get(ix)
    }

    fn detail(&self, cx: &App) -> Option<PullRequestDetail> {
        cx.stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&self.key)
            .cloned()
    }

    /// Paths the agent reviews under `scope`.
    fn scope_files(detail: &PullRequestDetail, scope: ReviewScope) -> Vec<String> {
        match scope {
            ReviewScope::WholePullRequest => detail.files.iter().map(|f| f.path.clone()).collect(),
            ReviewScope::UnviewedFiles => detail
                .files
                .iter()
                .filter(|f| !detail.is_viewed(&f.path))
                .map(|f| f.path.clone())
                .collect(),
            ReviewScope::SinceLastReview => {
                let latest_push = detail
                    .timeline
                    .iter()
                    .rposition(|item| matches!(item, TimelineItem::Pushed { .. }));
                let files: Vec<String> = latest_push
                    .map(|start| {
                        detail.timeline[start..]
                            .iter()
                            .filter_map(|item| match item {
                                TimelineItem::File { path } => Some(path.clone()),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if files.is_empty() {
                    detail.files.iter().map(|f| f.path.clone()).collect()
                } else {
                    files
                }
            }
        }
    }

    fn latest_push_commits(detail: &PullRequestDetail) -> usize {
        detail
            .timeline
            .iter()
            .rev()
            .find_map(|item| match item {
                TimelineItem::Pushed { commits, .. } => Some(commits.len()),
                _ => None,
            })
            .unwrap_or(detail.commits.len())
    }

    /// Where the agent will work, for display.
    fn workspace(&self, cx: &App) -> String {
        workspace_label(&self.key, cx)
    }

    /// Starts the run and closes the dialog. Returns `false` when nothing could start.
    pub fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(mut agent) = self.selected_agent(cx).cloned() else {
            return false;
        };
        // Only values the agent reported are saved; while its settings are unknown the
        // last saved ones stay as they were.
        if matches!(self.settings, SettingsState::Ready(_)) {
            agent.model = self.model.clone();
            agent.effort = self.effort.clone();
            let (id, model, effort) = (agent.id.clone(), agent.model.clone(), agent.effort.clone());
            cx.stores().settings().clone().update(cx, |settings, cx| {
                settings.set_agent_choices(&id, model, effort, cx)
            });
        }
        let options = StartOptions {
            scope: self.scope,
            depth: self.depth,
            instructions: self.instructions.read(cx).value().to_string(),
            auto_add: !self.drafts,
            findings_server: std::env::current_exe().ok(),
        };
        start_review(self.key.clone(), agent, options, window, cx)
    }
}

/// How to run an agent review.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct StartOptions {
    pub scope: ReviewScope,
    pub depth: ReviewDepth,
    pub instructions: String,
    /// Put findings straight into the pending review instead of keeping them as drafts.
    pub auto_add: bool,
    /// The program that serves the findings MCP tool with `--findings-mcp <socket>`.
    pub findings_server: Option<std::path::PathBuf>,
}

impl StartOptions {
    /// Reviews the whole pull request, keeps findings as drafts, and serves findings
    /// from this executable.
    pub fn new() -> Self {
        Self {
            scope: ReviewScope::WholePullRequest,
            depth: ReviewDepth::Thorough,
            instructions: String::new(),
            auto_add: false,
            findings_server: std::env::current_exe().ok(),
        }
    }

    /// Serves findings from `program` instead of this executable.
    pub fn with_findings_server(mut self, program: std::path::PathBuf) -> Self {
        self.findings_server = Some(program);
        self
    }
}

impl Default for StartOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Starts `agent` reviewing `key` and shows its run in the thread. Returns `false` when
/// the pull request isn't loaded or its diff can't be prepared.
pub fn start_review(
    key: PullRequestKey,
    agent: AgentConfig,
    options: StartOptions,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some(detail) = cx
        .stores()
        .pull_requests()
        .read(cx)
        .loaded_detail(&key)
        .cloned()
    else {
        return false;
    };
    let scope_files = StartReviewForm::scope_files(&detail, options.scope);
    let files: Vec<&diff_model::FileDiff> = detail
        .files
        .iter()
        .filter(|f| scope_files.contains(&f.path))
        .collect();
    let head_sha = detail.commits.last().map(|commit| commit.sha.clone());
    let review_tree = review_tree_for(&key, &detail, cx);
    let (cwd, cwd_label) = match &review_tree {
        Some(tree) => {
            let home = cx
                .stores()
                .settings()
                .read(cx)
                .home()
                .map(|h| h.to_path_buf());
            let label = display_path(&tree.destination(), home.as_deref());
            (tree.destination(), label)
        }
        None => match write_diff_folder(&key, &files) {
            Ok(folder) => (folder, "a temporary folder with the diff".to_string()),
            Err(error) => {
                window.push_notification(
                    Notification::error(format!(
                        "Couldn’t prepare the diff for the agent: {error}"
                    )),
                    cx,
                );
                return false;
            }
        },
    };
    let share = cx
        .stores()
        .settings()
        .read(cx)
        .preferences()
        .share_discussion_with_agents;
    let repository = key.repo().to_string();
    let mut input = PromptInput::new(&repository, key.number(), &detail.summary.title, files);
    input.description = &detail.body;
    input.base_ref = &detail.summary.base_ref;
    input.head_ref = &detail.summary.head_ref;
    input.instructions = &options.instructions;
    input.depth = options.depth;
    input.head_sha = head_sha.as_deref().filter(|_| review_tree.is_some());
    let diff_in_prompt = options.depth == ReviewDepth::Quick || review_tree.is_none();
    if share {
        input.discussion = detail
            .threads
            .iter()
            .flat_map(|thread| {
                thread.comments.iter().map(move |comment| {
                    format!(
                        "{} {}: {}",
                        thread.anchor_label(),
                        comment.author.login,
                        comment.body
                    )
                })
            })
            .collect();
    }
    let prompt = review_prompt(&input);
    let path = tool_path::path_env();
    let ask = cx
        .stores()
        .settings()
        .read(cx)
        .preferences()
        .ask_before_commands;
    let choices =
        SessionChoices::new(agent.model.clone(), agent.effort.clone()).asking_before_commands(ask);
    let mut request = ReviewRequest::new(launch_command(&agent), cwd, prompt)
        .with_env("PATH", path)
        .with_choices(choices)
        .with_follow_ups();
    if let Some(program) = options.findings_server {
        request = request.with_findings_server(program);
    }
    let actor = Actor::agent(agent.name.clone());
    let setup = RunSetup {
        cwd_label,
        scope_files,
        auto_add: options.auto_add,
        request,
        review_tree,
        diff_in_prompt,
    };
    let run = cx.new(|cx| AgentRun::start(key.clone(), actor, setup, cx));
    crate::insert_run(key, run, cx);
    true
}

/// The agent's command line with its launcher resolved: an app opened from Finder has
/// a minimal PATH.
pub(crate) fn launch_command(agent: &AgentConfig) -> String {
    let mut agent = agent.clone();
    if let Some(program) = tool_path::find_on_path(&agent.command) {
        agent.command = program.display().to_string();
    }
    agent.command_line()
}

/// The value to show for `setting`: `saved` when the agent still offers it, else the
/// agent's own default.
fn pick(setting: &AgentSetting, saved: Option<&str>) -> String {
    saved
        .filter(|value| setting.offers(value))
        .map(str::to_string)
        .unwrap_or_else(|| setting.current.clone())
}

/// The worktree an agent reviewing `key` works in: the head commit checked out from
/// the mapped clone under `~/.switchfold/review-worktrees`. `None` without a clone, or
/// before the commits load.
pub(crate) fn review_tree_for(
    key: &PullRequestKey,
    detail: &PullRequestDetail,
    cx: &App,
) -> Option<git_worktree::ReviewTree> {
    let settings = cx.stores().settings().read(cx);
    let clone = settings
        .clone_path(key.repo())
        .filter(|path| path.is_dir())?;
    let head = detail.commits.last()?;
    let root = settings.home()?.join(".switchfold/review-worktrees");
    Some(git_worktree::ReviewTree::new(
        clone.to_path_buf(),
        key.number(),
        head.sha.clone(),
        detail.summary.base_ref.clone(),
        root,
    ))
}

/// Where an agent reviewing `key` will work, for the dialog.
fn workspace_label(key: &PullRequestKey, cx: &App) -> String {
    let detail = cx
        .stores()
        .pull_requests()
        .read(cx)
        .loaded_detail(key)
        .cloned();
    let settings = cx.stores().settings().read(cx);
    match detail.and_then(|detail| review_tree_for(key, &detail, cx)) {
        Some(tree) => format!(
            "its own checkout of the head commit, {}",
            display_path(&tree.destination(), settings.home())
        ),
        None => "a temporary folder with the diff".into(),
    }
}

pub(crate) fn write_diff_folder(
    key: &PullRequestKey,
    files: &[&diff_model::FileDiff],
) -> std::io::Result<PathBuf> {
    let folder = std::env::temp_dir().join(format!(
        "switchfold-review-{}-{}-{}",
        key.repo().owner(),
        key.repo().name(),
        key.number()
    ));
    std::fs::create_dir_all(&folder)?;
    let patch: String = files.iter().map(|file| unified_patch(file)).collect();
    std::fs::write(folder.join("pull-request.diff"), patch)?;
    Ok(folder)
}

/// What the form tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartReviewEvent {
    /// A run started.
    Started,
}

impl EventEmitter<StartReviewEvent> for StartReviewForm {}

impl StartReviewForm {
    /// Whether the selected agent can start: launchable and not already reviewing.
    pub fn can_start(&self, cx: &App) -> bool {
        self.selected_agent(cx).is_some_and(|agent| {
            !crate::is_reviewing(&self.key, &agent.name, cx)
                && cx
                    .stores()
                    .settings()
                    .read(cx)
                    .agent_availability(&agent.id)
                    .is_some_and(|availability| availability.is_launchable())
        })
    }
}

impl Render for StartReviewForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let detail = self.detail(cx);
        let settings = cx.stores().settings().read(cx);
        let agent = self.selected_agent(cx).cloned();
        let availability = agent
            .as_ref()
            .and_then(|agent| settings.agent_availability(&agent.id).cloned());
        let home = settings.home().map(|home| home.to_path_buf());
        let agent_note: SharedString = match (&agent, &availability) {
            (None, _) => "Enable an agent in Settings › Agents & editor.".into(),
            (Some(agent), _) if crate::is_reviewing(&self.key, &agent.name, cx) => format!(
                "{} is already reviewing this pull request. Pick another agent to review alongside it.",
                agent.name
            )
            .into(),
            (Some(_), None) => "Checking…".into(),
            (Some(agent), Some(availability)) => {
                let binary = agent
                    .binary
                    .as_deref()
                    .filter(|_| availability.binary_found)
                    .map(|binary| display_path(binary, home.as_deref()));
                match (&availability.launcher, binary, &availability.version) {
                    (None, _, _) => {
                        format!("{} isn’t on PATH, so the agent can’t start.", agent.command).into()
                    }
                    (Some(_), Some(binary), Some(version)) => {
                        format!("ACP · {} {version} found at {binary}", command_name(agent)).into()
                    }
                    (Some(launcher), _, _) => {
                        format!("ACP · launched with {}", launcher.display()).into()
                    }
                }
            }
        };
        let ask = settings.preferences().ask_before_commands;
        let cwd_label = self.workspace(cx);
        let head = detail
            .as_ref()
            .and_then(|d| d.commits.last().map(|c| c.short_sha().to_string()))
            .unwrap_or_default();

        let scopes: Vec<(ReviewScope, String)> = detail
            .as_ref()
            .map(|detail| {
                let whole = detail.files.len();
                let unviewed = detail
                    .files
                    .iter()
                    .filter(|f| !detail.is_viewed(&f.path))
                    .count();
                let commits = Self::latest_push_commits(detail);
                vec![
                    (
                        ReviewScope::WholePullRequest,
                        format!("Whole pull request · {whole} files"),
                    ),
                    (
                        ReviewScope::UnviewedFiles,
                        format!("Unviewed files only · {unviewed} files"),
                    ),
                    (
                        ReviewScope::SinceLastReview,
                        format!("Since my last review · {commits} commits"),
                    ),
                ]
            })
            .unwrap_or_default();

        let can_start = self.can_start(cx);
        let label = |text: &'static str| Label::new(text).text_ui().font_weight(FontWeight::MEDIUM);
        let radios_scope: Vec<_> = scopes
            .into_iter()
            .map(|(scope, text)| {
                Radio::new(SharedString::from(format!("scope-{scope:?}")))
                    .label(text)
                    .checked(self.scope == scope)
                    .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                        if *checked {
                            this.scope = scope;
                            cx.notify();
                        }
                    }))
            })
            .collect();
        let radios_depth: Vec<_> = [
            (
                ReviewDepth::Thorough,
                "Thorough · explores callers and tests, runs checks",
            ),
            (
                ReviewDepth::Quick,
                "Quick · reads the diff, opens files if needed",
            ),
        ]
        .into_iter()
        .map(|(depth, text)| {
            Radio::new(SharedString::from(format!("depth-{depth:?}")))
                .label(text)
                .checked(self.depth == depth)
                .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                    if *checked {
                        this.depth = depth;
                        cx.notify();
                    }
                }))
        })
        .collect();

        v_flex()
            .gap_4()
            .p_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .gap_2()
                    .child(Icon::new(IconName::Sparkles).text_color(cx.app_theme().accent))
                    .child(div().text_heading().child("Review with an agent")),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1p5()
                            .child(label("Agent"))
                            .child(Select::new(&self.agent_select).placeholder("Choose an agent"))
                            .child(
                                div()
                                    .text_meta()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(agent_note),
                            ),
                    )
                    .when(agent.is_some(), |this| {
                        this.child(div().flex_1().min_w_0().child(self.render_settings(cx)))
                    }),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(label("Scope"))
                            .children(radios_scope),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(label("Depth"))
                            .children(radios_depth),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(label("Instructions"))
                    .child(EditMenu::new(
                        "instructions-edit-menu",
                        &self.instructions,
                        Textarea::new(&self.instructions).h_16(),
                    )),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(
                                Checkbox::new("drafts")
                                    .checked(self.drafts)
                                    .label(if self.drafts {
                                        "Add findings as drafts · nothing is posted until you add it"
                                    } else {
                                        "Add findings as drafts · off: findings go into your pending review"
                                    })
                                    .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                        this.drafts = *checked;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_start()
                                    .text_meta()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(Icon::new(IconName::Lock).small())
                                    .child(div().flex_1().min_w_0().child(format!(
                                        "Runs in {cwd_label}{}. Your own checkouts aren't touched. It can read files{}.",
                                        if head.is_empty() { String::new() } else { format!(" ({head})") },
                                        if ask { " and asks before running any command" } else { " and runs commands without asking" }
                                    ))),
                            ),
                    )
                    .child(
                        Button::new("start-review")
                            .primary()
                            .icon(IconName::Sparkles)
                            .label("Start review")
                            .disabled(!can_start)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.start(window, cx) {
                                    cx.emit(StartReviewEvent::Started);
                                }
                            })),
                    ),
            )
    }
}

impl StartReviewForm {
    /// The Model and Effort pickers, or why they aren't there.
    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let column = |label: &'static str, body: AnyElement| {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .child(Label::new(label).text_ui().font_weight(FontWeight::MEDIUM))
                .child(body)
        };
        let loading = |text: &'static str| {
            h_flex()
                .h_8()
                .gap_2()
                .text_ui()
                .text_color(muted)
                .child(Spinner::new().small())
                .child(text)
                .into_any_element()
        };
        let note = |text: String| {
            div()
                .h_8()
                .flex()
                .items_center()
                .text_ui()
                .text_color(muted)
                .child(text)
                .into_any_element()
        };
        let (model, effort) = match &self.settings {
            SettingsState::Failed(error) => {
                return v_flex()
                    .gap_1p5()
                    .child(
                        Label::new("Model")
                            .text_ui()
                            .font_weight(FontWeight::MEDIUM),
                    )
                    .child(div().text_meta().text_color(muted).child(format!(
                        "Couldn’t read the agent’s models, so it uses its defaults. {error}"
                    )))
                    .into_any_element();
            }
            SettingsState::Loading if self.model_select.is_none() => {
                (loading("Asking the agent…"), loading("Asking the agent…"))
            }
            SettingsState::Loading => (
                self.model_select
                    .as_ref()
                    .map(|select| Select::new(select).into_any_element())
                    .unwrap_or_else(|| note("Agent default".into())),
                loading("Checking this model…"),
            ),
            SettingsState::Ready(_) => (
                self.model_select
                    .as_ref()
                    .map(|select| Select::new(select).into_any_element())
                    .unwrap_or_else(|| note("Agent default".into())),
                self.effort_select
                    .as_ref()
                    .map(|select| Select::new(select).into_any_element())
                    .unwrap_or_else(|| note("Not adjustable for this model".into())),
            ),
        };
        h_flex()
            .gap_3()
            .items_start()
            .child(column("Model", model))
            .child(column("Effort", effort))
            .into_any_element()
    }
}

/// The agent's own program name, such as `claude`, for the availability note.
fn command_name(agent: &AgentConfig) -> String {
    agent
        .binary
        .as_deref()
        .and_then(|binary| binary.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| agent.command.clone())
}

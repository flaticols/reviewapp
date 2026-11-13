use std::rc::Rc;

use acp_client::{
    AgentEvent, Decision, PermissionPrompt, QuestionInput, ReviewRequest, RunControl, RunOutcome,
    SessionChoices, ToolStatus, question_prompt,
};
use app_state_ui::StoresExt as _;
use diff_model::{DiffLine, FileDiff};
use gpui_kit::component::{WindowExt as _, notification::Notification};
use gpui_kit::{App, AppContext as _, Context, Entity, Task, Window};
use pr_model::PullRequestKey;
use review_model::Actor;
use settings_model::{AgentConfig, display_path};

use crate::RunState;
use crate::start_form::{launch_command, review_tree_for, write_diff_folder};

/// What the reviewer asks an agent about a file's lines.
#[non_exhaustive]
pub struct AskOptions {
    /// The file the lines are in, as the reviewer sees it.
    pub file: Rc<FileDiff>,
    /// The selected lines, top to bottom.
    pub lines: Vec<DiffLine>,
    /// How the card names the lines, such as `Lines 14–28`.
    pub label: String,
    /// The head-side line the card and a comment on the lines sit under.
    pub anchor: u32,
    pub question: String,
    /// The program that serves the `suggest_comment` tool with `--findings-mcp <socket>`.
    pub findings_server: Option<std::path::PathBuf>,
}

impl AskOptions {
    /// `question` about `lines` of `file`, served by this binary's MCP tools.
    pub fn new(
        file: Rc<FileDiff>,
        lines: Vec<DiffLine>,
        label: impl Into<String>,
        anchor: u32,
        question: impl Into<String>,
    ) -> Self {
        Self {
            file,
            lines,
            label: label.into(),
            anchor,
            question: question.into(),
            findings_server: std::env::current_exe().ok(),
        }
    }
}

/// One question to an agent about lines of a pull request's diff. Owns the ACP session
/// and keeps the answer and the comment the agent suggests.
pub struct AgentQuestion {
    key: PullRequestKey,
    agent: Actor,
    repository: String,
    path: String,
    label: String,
    anchor: u32,
    question: String,
    cwd_label: String,
    state: RunState,
    answer: String,
    /// A tool ran since the last reply chunk, so the next one starts a new paragraph.
    answer_break: bool,
    /// The tool call in progress, as the agent titles it.
    doing: Option<String>,
    suggestion: Option<String>,
    suggestion_added: bool,
    permission: Option<PermissionPrompt>,
    control: Option<RunControl>,
    _events: Option<Task<()>>,
    _prepare: Option<Task<()>>,
}

impl AgentQuestion {
    fn start(
        key: PullRequestKey,
        agent: Actor,
        options: &AskOptions,
        cwd_label: String,
        request: ReviewRequest,
        review_tree: Option<git_worktree::ReviewTree>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut question = Self {
            repository: key.repo().to_string(),
            key,
            agent,
            path: options.file.path.clone(),
            label: options.label.clone(),
            anchor: options.anchor,
            question: options.question.clone(),
            cwd_label,
            state: RunState::Starting,
            answer: String::new(),
            answer_break: false,
            doing: None,
            suggestion: None,
            suggestion_added: false,
            permission: None,
            control: None,
            _events: None,
            _prepare: None,
        };
        match review_tree {
            Some(tree) => {
                question.state = RunState::Preparing;
                let prepare = cx.background_spawn(async move { tree.prepare() });
                question._prepare = Some(cx.spawn(async move |this, cx| {
                    let result = prepare.await;
                    this.update(cx, |this, cx| {
                        if this.state != RunState::Preparing {
                            return;
                        }
                        match result {
                            Ok(cwd) => {
                                let mut request = request;
                                request.cwd = cwd;
                                this.state = RunState::Starting;
                                this.launch(request, cx);
                            }
                            Err(error) => this.apply(
                                AgentEvent::Failed(format!(
                                    "Couldn’t check out the pull request for the agent: {error}"
                                )),
                                cx,
                            ),
                        }
                        cx.refresh_windows();
                    })
                    .ok();
                }));
            }
            None => question.launch(request, cx),
        }
        question
    }

    fn launch(&mut self, request: ReviewRequest, cx: &mut Context<Self>) {
        let (events, control) = crate::launcher::launch(request, cx);
        self.control = control;
        self._events = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let finished = matches!(event, AgentEvent::Finished(_) | AgentEvent::Failed(_));
                if this.update(cx, |this, cx| this.apply(event, cx)).is_err() || finished {
                    break;
                }
            }
        }));
    }

    /// The pull request the lines belong to.
    pub fn key(&self) -> &PullRequestKey {
        &self.key
    }

    /// The agent asked.
    pub fn agent(&self) -> &Actor {
        &self.agent
    }

    /// The file the lines are in.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The lines asked about, such as `Lines 14–28`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The head-side line the question sits under.
    pub fn anchor(&self) -> u32 {
        self.anchor
    }

    /// What the reviewer asked.
    pub fn question(&self) -> &str {
        &self.question
    }

    /// Where the agent works, for display.
    pub fn cwd_label(&self) -> &str {
        &self.cwd_label
    }

    /// Where the session stands.
    pub fn state(&self) -> &RunState {
        &self.state
    }

    /// The agent's answer so far, in Markdown.
    pub fn answer(&self) -> &str {
        &self.answer
    }

    /// The tool call in progress, while there is one.
    pub fn doing(&self) -> Option<&str> {
        self.doing.as_deref()
    }

    /// The comment the agent suggests for the lines, once it has one.
    pub fn suggestion(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }

    /// Whether the suggestion went into the reviewer's pending review.
    pub fn suggestion_added(&self) -> bool {
        self.suggestion_added
    }

    /// Records that the suggestion went into the reviewer's pending review.
    pub fn mark_suggestion_added(&mut self, cx: &mut Context<Self>) {
        self.suggestion_added = true;
        cx.refresh_windows();
    }

    /// The pending permission prompt, if any.
    pub fn permission(&self) -> Option<&PermissionPrompt> {
        self.permission.as_ref()
    }

    /// Answers the pending permission prompt.
    pub fn answer_permission(&mut self, decision: Decision, cx: &mut Context<Self>) {
        let Some(prompt) = self.permission.take() else {
            return;
        };
        crate::permission::answer(prompt, decision, &self.repository, cx);
        self.state = RunState::Running;
        cx.refresh_windows();
    }

    /// Asks the agent to stop. What it answered so far stays.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(prompt) = self.permission.take() {
            prompt.respond(Decision::Deny);
        }
        match &mut self.control {
            Some(control) => {
                control.cancel();
                self.state = RunState::Stopping;
            }
            // Still checking out, or a session that can't be stopped: it just ends here.
            None => {
                self._events = None;
                self._prepare = None;
                self.state = RunState::Finished(RunOutcome::Cancelled);
            }
        }
        cx.refresh_windows();
    }

    /// Stops the agent if it is still working and removes the question.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if !self.state.is_over() {
            self.stop(cx);
        }
        let this = cx.entity();
        crate::remove_question(&self.key.clone(), &this, cx);
    }

    fn apply(&mut self, event: AgentEvent, cx: &mut Context<Self>) {
        match event {
            AgentEvent::Connected { .. } => self.state = RunState::Running,
            AgentEvent::Message(chunk) => {
                if std::mem::take(&mut self.answer_break) && !self.answer.trim().is_empty() {
                    self.answer.push_str("\n\n");
                }
                self.answer.push_str(&chunk);
            }
            AgentEvent::Tool(activity) => {
                self.answer_break = true;
                match activity.status {
                    Some(ToolStatus::Completed | ToolStatus::Failed) => self.doing = None,
                    _ => {
                        if let Some(doing) = activity
                            .command
                            .map(|command| format!("$ {command}"))
                            .or(activity.title)
                        {
                            self.doing = Some(doing);
                        }
                    }
                }
            }
            AgentEvent::Permission(prompt) => {
                if let Some(prompt) = crate::permission::screen(prompt, &self.repository, cx) {
                    self.permission = Some(prompt);
                    self.state = RunState::AwaitingPermission;
                }
            }
            // Questions don't ask for follow-ups, so no turn ends without the session.
            AgentEvent::TurnEnded(_) => {}
            AgentEvent::Suggestion(body) => {
                let body = body.trim();
                if !body.is_empty() {
                    self.suggestion = Some(body.to_string());
                }
            }
            // A question has no plan to follow, shows no reasoning and reports no
            // findings: the answer is the reply.
            AgentEvent::Thought(_) | AgentEvent::Plan(_) | AgentEvent::Finding(_) => {}
            AgentEvent::Finished(outcome) => {
                self.permission = None;
                self.doing = None;
                self.state = match outcome {
                    RunOutcome::Refused => {
                        RunState::Failed("The agent declined to answer this question.".into())
                    }
                    outcome => RunState::Finished(outcome),
                };
            }
            AgentEvent::Failed(message) => {
                self.permission = None;
                self.doing = None;
                self.state = RunState::Failed(message);
            }
        }
        cx.notify();
        cx.refresh_windows();
    }
}

/// Asks `agent` the question in `options` about lines of pull request `key`, in the
/// pull request's checkout when its repository has a clone. Returns the question, which
/// streams the answer; `None` when the pull request isn't loaded or the agent's folder
/// couldn't be prepared, which a notification explains.
pub fn ask_about_lines(
    key: PullRequestKey,
    agent: AgentConfig,
    options: AskOptions,
    window: &mut Window,
    cx: &mut App,
) -> Option<Entity<AgentQuestion>> {
    let detail = cx
        .stores()
        .pull_requests()
        .read(cx)
        .loaded_detail(&key)
        .cloned()?;
    let head_sha = detail.commits.last().map(|commit| commit.sha.clone());
    let review_tree = review_tree_for(&key, &detail, cx);
    let (cwd, cwd_label) = match &review_tree {
        Some(tree) => {
            let home = cx
                .stores()
                .settings()
                .read(cx)
                .home()
                .map(|home| home.to_path_buf());
            let label = display_path(&tree.destination(), home.as_deref());
            (tree.destination(), label)
        }
        // The folder is the one a review of this pull request reads, so it holds the
        // whole change, not only the file asked about.
        None => match write_diff_folder(&key, &detail.files.iter().collect::<Vec<_>>()) {
            Ok(folder) => (folder, "a temporary folder with the diff".to_string()),
            Err(error) => {
                window.push_notification(
                    Notification::error(format!(
                        "Couldn’t prepare the diff for the agent: {error}"
                    )),
                    cx,
                );
                return None;
            }
        },
    };
    let preferences = cx.stores().settings().read(cx).preferences().clone();
    let repository = key.repo().to_string();
    let mut input = QuestionInput::new(
        &repository,
        key.number(),
        &detail.summary.title,
        &options.file,
        options.lines.iter().collect(),
        &options.question,
    );
    input.description = &detail.body;
    input.base_ref = &detail.summary.base_ref;
    input.head_ref = &detail.summary.head_ref;
    input.head_sha = head_sha.as_deref().filter(|_| review_tree.is_some());
    input.can_suggest = options.findings_server.is_some();
    if preferences.share_discussion_with_agents {
        input.discussion = detail
            .threads
            .iter()
            .filter(|thread| thread.path == options.file.path)
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
    let choices = SessionChoices::new(agent.model.clone(), agent.effort.clone())
        .asking_before_commands(preferences.ask_before_commands);
    let mut request = ReviewRequest::new(launch_command(&agent), cwd, question_prompt(&input))
        .with_env("PATH", tool_path::path_env())
        .with_choices(choices);
    if let Some(program) = options.findings_server.clone() {
        request = request.with_findings_server(program);
    }
    // Preparing the checkout resets it. While another agent works there it is already
    // at the head commit, and a reset would pull files from under that agent.
    let busy = crate::running_count(&key, cx) > 0
        || crate::questions_for(&key, cx)
            .iter()
            .any(|question| !question.read(cx).state().is_over());
    let review_tree =
        review_tree.filter(|tree| !(busy && tree.destination().join(".git").exists()));
    let actor = Actor::agent(agent.name.clone());
    let question = cx.new(|cx| {
        AgentQuestion::start(
            key.clone(),
            actor,
            &options,
            cwd_label,
            request,
            review_tree,
            cx,
        )
    });
    crate::insert_question(key, question.clone(), cx);
    Some(question)
}

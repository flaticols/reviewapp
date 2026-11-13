use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use acp_client::{
    AgentEvent, Decision, PermissionPrompt, PlanStatus, PlanStep, ReviewRequest, RunControl,
    RunOutcome, ToolActivity, ToolKind, ToolStatus,
};
use app_state_ui::StoresExt as _;
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::{App, AppContext as _, Context, Entity, Task, Window};
use pr_model::{PullRequestKey, Timestamp};
use review_model::{Actor, AgentFinding, AgentReviewSummary};

/// Where a run stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunState {
    /// Checking out the pull request's head for the agent to work in.
    Preparing,
    Starting,
    Running,
    /// Waiting for the user to answer a permission prompt.
    AwaitingPermission,
    Stopping,
    /// The agent finished its turn or was stopped.
    Finished(RunOutcome),
    Failed(String),
}

impl RunState {
    /// Whether the run has ended, one way or another.
    pub fn is_over(&self) -> bool {
        matches!(self, RunState::Finished(_) | RunState::Failed(_))
    }
}

/// One tool call in a run's activity log.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Activity {
    pub id: String,
    pub title: String,
    pub kind: Option<ToolKind>,
    pub status: ToolStatus,
    pub command: Option<String>,
    /// Files the tool reported touching, as absolute paths.
    pub paths: Vec<std::path::PathBuf>,
    /// When it started, since the run began.
    pub at: Duration,
}

/// Activity entries kept per run; older ones scroll away.
const ACTIVITY_LIMIT: usize = 200;

/// One agent review of one pull request. Owns the ACP session and applies its events
/// to the pull request store.
pub struct AgentRun {
    key: PullRequestKey,
    agent: Actor,
    repository: String,
    cwd_label: String,
    scope_files: Vec<String>,
    /// Files in scope a tool call has touched: read, searched, diffed or commented on.
    files_seen: BTreeSet<String>,
    reading: Option<String>,
    activity: Vec<Activity>,
    plan: Vec<PlanStep>,
    message: String,
    /// A tool ran since the last reply chunk, so the next one starts a new paragraph.
    message_break: bool,
    thought: String,
    /// The folder the agent works in, for reading paths out of its commands.
    root: std::path::PathBuf,
    model: Option<String>,
    effort: Option<String>,
    finished_after: Option<Duration>,
    state: RunState,
    permission: Option<PermissionPrompt>,
    findings: u32,
    auto_add: bool,
    started: Instant,
    control: Option<RunControl>,
    /// The turn ended and the agent's session waits for the reviewer's reply.
    awaiting_reply: bool,
    /// Where the reviewer writes that reply, made the first time it shows.
    reply_input: Option<Entity<TextareaState>>,
    /// The first completed turn put the review on the timeline; later turns don't again.
    summary_recorded: bool,
    _events: Option<Task<()>>,
    _prepare: Option<Task<()>>,
    _clock: Task<()>,
    /// The run's transcript: the prompt, then every event. Written as it happens so a
    /// review can be checked after the fact.
    log: Option<std::fs::File>,
    log_path: Option<std::path::PathBuf>,
}

/// What a run needs besides the pull request and the agent.
#[non_exhaustive]
pub struct RunSetup {
    /// Where the agent works, for display.
    pub cwd_label: String,
    /// Paths in scope, relative to the repository root.
    pub scope_files: Vec<String>,
    /// Put findings straight into the viewer's pending review.
    pub auto_add: bool,
    pub request: ReviewRequest,
    /// Checks out the head commit first and runs the agent there.
    pub review_tree: Option<git_worktree::ReviewTree>,
    /// The prompt carries the whole diff, so every file in scope counts as looked at.
    pub diff_in_prompt: bool,
}

impl AgentRun {
    /// Starts `setup.request` for `key`. With a review tree, the head commit is checked
    /// out first and the agent starts in it.
    pub fn start(
        key: PullRequestKey,
        agent: Actor,
        setup: RunSetup,
        cx: &mut Context<Self>,
    ) -> Self {
        let RunSetup {
            cwd_label,
            scope_files,
            auto_add,
            request,
            review_tree,
            diff_in_prompt,
        } = setup;
        let model = request.choices.model.clone();
        let effort = request.choices.effort.clone();
        let clock = Self::clock(cx);
        let store = cx.stores().pull_requests().clone();
        let reviewing_key = key.clone();
        let reviewing_agent = agent.clone();
        store.update(cx, |store, cx| {
            store.set_agent_reviewing(&reviewing_key, &reviewing_agent, true, cx)
        });
        let files_seen = if diff_in_prompt {
            scope_files.iter().cloned().collect()
        } else {
            BTreeSet::new()
        };
        let mut run = Self {
            repository: key.repo().to_string(),
            key,
            agent,
            cwd_label,
            scope_files,
            files_seen,
            reading: None,
            activity: Vec::new(),
            plan: Vec::new(),
            message: String::new(),
            message_break: false,
            thought: String::new(),
            root: request.cwd.clone(),
            model,
            effort,
            finished_after: None,
            state: RunState::Starting,
            permission: None,
            findings: 0,
            auto_add,
            started: Instant::now(),
            control: None,
            awaiting_reply: false,
            reply_input: None,
            summary_recorded: false,
            _events: None,
            _prepare: None,
            _clock: clock,
            log: None,
            log_path: None,
        };
        run.open_log(cx);
        match review_tree {
            Some(tree) => {
                run.state = RunState::Preparing;
                let prepare = cx.background_spawn(async move { tree.prepare() });
                run._prepare = Some(cx.spawn(async move |this, cx| {
                    let result = prepare.await;
                    this.update(cx, |this, cx| {
                        if this.state != RunState::Preparing {
                            return;
                        }
                        match result {
                            Ok(cwd) => {
                                let mut request = request;
                                this.root = cwd.clone();
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
                        cx.notify();
                    })
                    .ok();
                }));
            }
            None => run.launch(request, cx),
        }
        run
    }

    /// Where the run's transcript is written, under `~/.switchfold/logs/agent-runs`.
    pub fn log_path(&self) -> Option<&std::path::Path> {
        self.log_path.as_deref()
    }

    fn open_log(&mut self, cx: &App) {
        let Some(home) = cx.stores().settings().read(cx).home() else {
            return;
        };
        let dir = home.join(".switchfold/logs/agent-runs");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        let name = format!(
            "{}-{}-{}-{stamp}.log",
            self.key.repo().name(),
            self.key.number(),
            self.agent.login.to_lowercase().replace(' ', "-")
        );
        let path = dir.join(name);
        if std::fs::create_dir_all(&dir).is_ok()
            && let Ok(file) = std::fs::File::create(&path)
        {
            self.log = Some(file);
            self.log_path = Some(path);
        }
    }

    fn log(&mut self, entry: impl std::fmt::Display) {
        use std::io::Write as _;
        if let Some(file) = &mut self.log {
            writeln!(
                file,
                "[{:>6.1}s] {entry}",
                self.started.elapsed().as_secs_f32()
            )
            .ok();
        }
    }

    /// Redraws every second while the run is going, for its elapsed time.
    fn clock(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let running = this.update(cx, |this, cx| {
                    cx.notify();
                    !this.state.is_over()
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        })
    }

    /// Starts the agent and follows its events.
    fn launch(&mut self, request: ReviewRequest, cx: &mut Context<Self>) {
        self.log(format_args!(
            "start {} in {} with {:?}\n--- prompt ---\n{}\n--- end of prompt ---",
            request.command_line,
            request.cwd.display(),
            request.choices,
            request.prompt
        ));
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

    /// The pull request under review.
    pub fn key(&self) -> &PullRequestKey {
        &self.key
    }

    /// The agent running the review.
    pub fn agent(&self) -> &Actor {
        &self.agent
    }

    /// Where the agent works, for display.
    pub fn cwd_label(&self) -> &str {
        &self.cwd_label
    }

    /// Where the run stands.
    pub fn state(&self) -> &RunState {
        &self.state
    }

    /// The file being read now, by file name.
    pub fn reading(&self) -> Option<&str> {
        self.reading
            .as_deref()
            .map(|path| path.rsplit('/').next().unwrap_or(path))
    }

    /// `(seen, total)` files in scope: those a tool call has touched so far.
    pub fn progress(&self) -> (usize, usize) {
        (self.files_seen.len(), self.scope_files.len())
    }

    /// How far along the run is, from 0 to 1: by the agent's plan when it shares one,
    /// else by the files it has touched.
    pub fn fraction(&self) -> f32 {
        if matches!(self.state, RunState::Finished(RunOutcome::Completed)) {
            return 1.;
        }
        if !self.plan.is_empty() {
            let done = self
                .plan
                .iter()
                .filter(|step| step.status == PlanStatus::Completed)
                .count();
            return done as f32 / self.plan.len() as f32;
        }
        let (seen, total) = self.progress();
        if total == 0 {
            0.
        } else {
            seen as f32 / total as f32
        }
    }

    /// The agent's plan, as last shared.
    pub fn plan(&self) -> &[PlanStep] {
        &self.plan
    }

    /// Tool calls in the order they started.
    pub fn activity(&self) -> &[Activity] {
        &self.activity
    }

    /// The agent's reply so far.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The agent's reasoning so far, for agents that share it.
    pub fn thought(&self) -> &str {
        &self.thought
    }

    /// The model and effort the run asked for, as the agent names them.
    pub fn model_and_effort(&self) -> (Option<&str>, Option<&str>) {
        (self.model.as_deref(), self.effort.as_deref())
    }

    /// Time since the run started, frozen when it ends.
    pub fn elapsed(&self) -> Duration {
        self.finished_after
            .unwrap_or_else(|| self.started.elapsed())
    }

    /// Findings reported so far.
    pub fn findings(&self) -> u32 {
        self.findings
    }

    /// The pending permission prompt, if any.
    pub fn permission(&self) -> Option<&PermissionPrompt> {
        self.permission.as_ref()
    }

    /// Answers the pending permission prompt. `AllowAlways` also remembers the command
    /// for this repository.
    pub fn answer(&mut self, decision: Decision, cx: &mut Context<Self>) {
        let Some(prompt) = self.permission.take() else {
            return;
        };
        crate::permission::answer(prompt, decision, &self.repository, cx);
        self.state = RunState::Running;
        cx.notify();
    }

    /// Whether the agent's turn ended and its session takes a reply.
    pub fn can_reply(&self) -> bool {
        self.awaiting_reply
    }

    /// The field the reviewer replies to the agent in. ⌘↩ sends it.
    pub fn reply_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        if let Some(input) = &self.reply_input {
            return input.clone();
        }
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Reply to the agent… ⌘↩ sends")
                .rows(2)
                .context_menu(false)
        });
        cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter {
                secondary: true, ..
            } = event
            {
                this.send_reply(window, cx);
            }
        })
        .detach();
        self.reply_input = Some(input.clone());
        input
    }

    /// Sends what the reviewer wrote as the session's next prompt, and shows it above
    /// the agent's answer.
    pub fn send_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.reply_input.clone() else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() || !self.awaiting_reply {
            return;
        }
        let sent = self
            .control
            .as_ref()
            .is_some_and(|control| control.follow_up(text.clone()));
        if !sent {
            self.awaiting_reply = false;
            cx.notify();
            return;
        }
        self.log(format_args!(
            "follow-up\n--- reply ---\n{text}\n--- end of reply ---"
        ));
        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.awaiting_reply = false;
        let quoted: Vec<String> = text.lines().map(|line| format!("> {line}")).collect();
        if !self.message.trim().is_empty() {
            self.message.push_str("\n\n");
        }
        self.message.push_str(&quoted.join("\n"));
        self.message.push_str("\n\n");
        self.message_break = false;
        self.finished_after = None;
        self.state = RunState::Running;
        let key = self.key.clone();
        let agent = self.agent.clone();
        cx.stores().pull_requests().clone().update(cx, |store, cx| {
            store.set_agent_reviewing(&key, &agent, true, cx)
        });
        self._clock = Self::clock(cx);
        cx.notify();
    }

    /// Asks the agent to stop. A session waiting for a reply ends.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(prompt) = self.permission.take() {
            prompt.respond(Decision::Deny);
        }
        if std::mem::take(&mut self.awaiting_reply) {
            if let Some(control) = &mut self.control {
                control.cancel();
            }
            cx.notify();
            return;
        }
        match &mut self.control {
            Some(control) => {
                control.cancel();
                self.state = RunState::Stopping;
            }
            // Still checking out: there's no agent to ask, so the run just ends.
            None => {
                self.finish(RunOutcome::Cancelled, cx);
                return;
            }
        }
        cx.notify();
    }

    /// Removes the run from the pull request's reviews.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.awaiting_reply = false;
        if let Some(control) = &mut self.control {
            control.cancel();
        }
        let key = self.key.clone();
        let this = cx.entity();
        crate::remove_run(&key, &this, cx);
    }

    fn in_scope(&self, path: &std::path::Path) -> Option<String> {
        let text = path.to_string_lossy();
        self.scope_files
            .iter()
            .find(|candidate| text.ends_with(candidate.as_str()))
            .cloned()
    }

    /// Files in scope the tool call `id` named in its locations, titles or command over
    /// all its updates, so a `sed -n 1,80p file.go` or `git diff -- dir` counts as much
    /// as a read.
    fn files_touched(&self, id: &str) -> Vec<String> {
        let Some(entry) = self.activity.iter().rev().find(|entry| entry.id == id) else {
            return Vec::new();
        };
        let mut found: Vec<String> = entry
            .paths
            .iter()
            .filter_map(|path| self.in_scope(path))
            .collect();
        for text in [Some(entry.title.as_str()), entry.command.as_deref()]
            .into_iter()
            .flatten()
        {
            found.extend(crate::coverage::files_named(
                text,
                &self.root,
                &self.scope_files,
            ));
        }
        found.sort();
        found.dedup();
        found
    }

    fn record(&mut self, activity: &ToolActivity) {
        let entry = self
            .activity
            .iter_mut()
            .rev()
            .find(|entry| entry.id == activity.id);
        match entry {
            Some(entry) => {
                if let Some(title) = &activity.title {
                    entry.title = title.clone();
                }
                if let Some(status) = activity.status {
                    entry.status = status;
                }
                if activity.kind.is_some() {
                    entry.kind = activity.kind;
                }
                if activity.command.is_some() {
                    entry.command = activity.command.clone();
                }
                entry.paths.extend(activity.paths.iter().cloned());
            }
            None => {
                self.activity.push(Activity {
                    id: activity.id.clone(),
                    title: activity.title.clone().unwrap_or_else(|| "Tool call".into()),
                    kind: activity.kind,
                    status: activity.status.unwrap_or(ToolStatus::Running),
                    command: activity.command.clone(),
                    paths: activity.paths.clone(),
                    at: self.started.elapsed(),
                });
                if self.activity.len() > ACTIVITY_LIMIT {
                    self.activity.remove(0);
                }
            }
        }
    }

    fn apply(&mut self, event: AgentEvent, cx: &mut Context<Self>) {
        match &event {
            AgentEvent::Permission(prompt) => self.log(format_args!(
                "permission {:?} {:?}",
                prompt.title(),
                prompt.command()
            )),
            other => self.log(format_args!("{other:?}")),
        }
        match event {
            AgentEvent::Connected { .. } => self.state = RunState::Running,
            AgentEvent::Message(chunk) => {
                if std::mem::take(&mut self.message_break) && !self.message.trim().is_empty() {
                    self.message.push_str("\n\n");
                }
                self.message.push_str(&chunk);
            }
            AgentEvent::Thought(chunk) => self.thought.push_str(&chunk),
            AgentEvent::Plan(steps) => self.plan = steps,
            AgentEvent::Tool(activity) => {
                self.message_break = true;
                self.record(&activity);
                let mentioned = self.files_touched(&activity.id);
                match activity.status {
                    Some(ToolStatus::Completed) => {
                        self.files_seen.extend(mentioned);
                        self.reading = None;
                    }
                    Some(ToolStatus::Failed) => self.reading = None,
                    _ => {
                        if matches!(activity.kind, Some(ToolKind::Read) | None)
                            && let Some(path) = mentioned.first()
                        {
                            self.reading = Some(path.clone());
                        }
                    }
                }
            }
            AgentEvent::Permission(prompt) => self.ask(prompt, cx),
            AgentEvent::Finding(reported) => {
                self.findings += 1;
                if let Some(path) = self.in_scope(std::path::Path::new(&reported.path)) {
                    self.files_seen.insert(path);
                }
                // Several agents can review at once, so the id names the agent too.
                let mut finding = AgentFinding::new(
                    format!(
                        "agent-{}-{}-{}",
                        self.agent.login.to_lowercase().replace(' ', "-"),
                        self.started.elapsed().as_millis(),
                        self.findings
                    ),
                    self.agent.clone(),
                    reported.path,
                    reported.line,
                    reported.body,
                );
                if self.auto_add {
                    finding.state = review_model::FindingState::Added;
                }
                let key = self.key.clone();
                cx.stores()
                    .pull_requests()
                    .clone()
                    .update(cx, |store, cx| store.add_finding(&key, finding, cx));
            }
            // Only a question about selected lines asks for a comment.
            AgentEvent::Suggestion(_) => {}
            AgentEvent::TurnEnded(outcome) => {
                self.finish(outcome, cx);
                self.awaiting_reply =
                    self.control.is_some() && !matches!(self.state, RunState::Failed(_));
            }
            AgentEvent::Finished(outcome) => {
                self.awaiting_reply = false;
                // A session stopped while it waited for a reply already showed its turn.
                if !self.state.is_over() {
                    self.finish(outcome, cx);
                }
            }
            AgentEvent::Failed(message) => {
                self.awaiting_reply = false;
                self.permission = None;
                self.finished_after = Some(self.started.elapsed());
                self.state = RunState::Failed(message);
                let key = self.key.clone();
                let agent = self.agent.clone();
                cx.stores().pull_requests().clone().update(cx, |store, cx| {
                    store.set_agent_reviewing(&key, &agent, false, cx)
                });
            }
        }
        cx.notify();
    }

    fn ask(&mut self, prompt: PermissionPrompt, cx: &mut Context<Self>) {
        if let Some(prompt) = crate::permission::screen(prompt, &self.repository, cx) {
            self.permission = Some(prompt);
            self.state = RunState::AwaitingPermission;
        }
    }

    fn finish(&mut self, outcome: RunOutcome, cx: &mut Context<Self>) {
        self.permission = None;
        self.reading = None;
        self.finished_after = Some(self.started.elapsed());
        let summary = AgentReviewSummary::new(
            self.agent.clone(),
            self.files_seen.len().max(self.scope_files.len()) as u32,
            self.findings,
            if self.auto_add { self.findings } else { 0 },
            Timestamp::now(),
        );
        let key = self.key.clone();
        let agent = self.agent.clone();
        let store = cx.stores().pull_requests().clone();
        self.state = match outcome {
            RunOutcome::Completed | RunOutcome::Truncated => {
                if std::mem::replace(&mut self.summary_recorded, true) {
                    store.update(cx, |store, cx| {
                        store.set_agent_reviewing(&key, &agent, false, cx)
                    });
                } else {
                    store.update(cx, |store, cx| store.finish_agent_review(&key, summary, cx));
                }
                RunState::Finished(outcome)
            }
            RunOutcome::Cancelled => {
                store.update(cx, |store, cx| {
                    store.set_agent_reviewing(&key, &agent, false, cx)
                });
                RunState::Finished(outcome)
            }
            RunOutcome::Refused => {
                store.update(cx, |store, cx| {
                    store.set_agent_reviewing(&key, &agent, false, cx)
                });
                RunState::Failed("The agent declined to review this pull request.".into())
            }
        };
        cx.refresh_windows();
    }
}

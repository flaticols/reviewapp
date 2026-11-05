use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use pr_cache::PullRequestCache;

use github_api::{ApiError, DraftComment, PullRequestSource, Reply, ReviewSubmission};
use gpui_kit::{App, AppContext as _, Context, EventEmitter, Task};
use serde::{Serialize, de::DeserializeOwned};
use pr_model::{
    CheckStatus, PullRequest, PullRequestKey, PullRequestState, RepoSlug, ReviewDecision,
    SavedFilter, Timestamp,
};
use review_model::{
    Actor, AgentFinding, CheckRun, Comment, FindingState, LogLine, PullRequestDetail,
    ReviewThread, ReviewVerdict, ReviewerState, TimelineItem,
};

/// How often running checks are polled.
const CHECK_POLL: Duration = Duration::from_secs(5);
/// How often relative ages are refreshed.
const CLOCK_TICK: Duration = Duration::from_secs(30);
/// How long edits to one pull request settle before they are written to the cache.
const SAVE_DELAY: Duration = Duration::from_millis(750);
/// Review data not opened for this long is deleted from the cache.
const CACHE_MAX_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 30);

/// A request to GitHub running in the background.
type Fetch<T> = Task<Result<T, ApiError>>;

/// Whether the pull request list has been loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListState {
    Loading,
    Loaded,
    Failed(String),
}

/// Whether one pull request's review data has been loaded.
#[derive(Clone, Debug)]
pub enum DetailState {
    Loading,
    Loaded(Box<PullRequestDetail>),
    Failed(String),
}

/// What changed in the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PullRequestStoreEvent {
    /// The list was loaded or refreshed.
    ListChanged,
    /// One pull request's detail changed.
    DetailChanged(PullRequestKey),
}

/// Owns pull requests, their review data and the viewer's pending review. Every
/// mutation notifies observers and emits a [`PullRequestStoreEvent`].
pub struct PullRequestStore {
    source: Arc<dyn PullRequestSource>,
    list: ListState,
    pull_requests: Vec<PullRequest>,
    details: HashMap<PullRequestKey, DetailState>,
    saved_filters: Vec<SavedFilter>,
    now: Timestamp,
    last_synced: Option<Timestamp>,
    next_comment_id: u64,
    /// Keeps ids of comments made this run apart from cached ones from earlier runs.
    session: i64,
    cache: Option<Arc<PullRequestCache>>,
    /// Pending cache writes, one per pull request; replacing one cancels the earlier.
    saves: HashMap<PullRequestKey, Task<()>>,
    /// What screens keep per pull request, such as pane layouts, by key and name.
    view_states: HashMap<(PullRequestKey, &'static str), serde_json::Value>,
    /// Pending view state writes; replacing one cancels the earlier.
    view_saves: HashMap<(PullRequestKey, &'static str), Task<()>>,
    /// People who can review, per repository, once asked for.
    assignable: HashMap<RepoSlug, Vec<Actor>>,
    /// Check logs being fetched, by pull request and check id.
    log_requests: HashSet<(PullRequestKey, String)>,
    _clock: Task<()>,
    _check_poll: Task<()>,
}

impl EventEmitter<PullRequestStoreEvent> for PullRequestStore {}

impl PullRequestStore {
    /// Creates the store and starts loading the list from `source`. With a `cache`, the
    /// list and review data saved last time show until GitHub answers.
    pub fn new(
        source: Arc<dyn PullRequestSource>,
        saved_filters: Vec<SavedFilter>,
        cache: Option<PullRequestCache>,
        cx: &mut Context<Self>,
    ) -> Self {
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK_TICK).await;
                if this
                    .update(cx, |this, cx| {
                        this.now = Timestamp::now();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let check_poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_POLL).await;
                if this.update(cx, |this, cx| this.poll_checks(cx)).is_err() {
                    break;
                }
            }
        });
        let cache = cache.map(Arc::new);
        // The list is small enough to read before the first frame, which saves a flash of
        // the loading state on every launch.
        let cached = cache.as_ref().and_then(|cache| cache.load_list());
        let mut store = Self {
            source,
            list: if cached.is_some() {
                ListState::Loaded
            } else {
                ListState::Loading
            },
            last_synced: cached.as_ref().map(|cached| cached.saved_at),
            pull_requests: cached.map(|cached| cached.data).unwrap_or_default(),
            details: HashMap::new(),
            saved_filters,
            now: Timestamp::now(),
            assignable: HashMap::new(),
            next_comment_id: 0,
            session: Timestamp::now().unix(),
            cache,
            saves: HashMap::new(),
            view_states: HashMap::new(),
            view_saves: HashMap::new(),
            log_requests: HashSet::new(),
            _clock: clock,
            _check_poll: check_poll,
        };
        store.refresh(cx);
        store
    }

    /// Reloads the list. Previously loaded pull requests stay visible meanwhile.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.pull_requests.is_empty() {
            self.list = ListState::Loading;
        }
        let source = self.source.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { source.pull_requests() })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(pull_requests) => {
                        this.pull_requests = pull_requests;
                        this.list = ListState::Loaded;
                        this.now = Timestamp::now();
                        this.last_synced = Some(this.now);
                        this.save_list(cx);
                    }
                    // Cached pull requests stay up, with their sync time, when offline.
                    Err(_) if !this.pull_requests.is_empty() => {}
                    Err(error) => this.list = ListState::Failed(error.to_string()),
                }
                cx.emit(PullRequestStoreEvent::ListChanged);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Whether the list has loaded.
    pub fn list_state(&self) -> &ListState {
        &self.list
    }

    /// Every pull request in the viewer's inboxes.
    pub fn pull_requests(&self) -> &[PullRequest] {
        &self.pull_requests
    }

    /// The pull request with `key`, from the list.
    pub fn pull_request(&self, key: &PullRequestKey) -> Option<&PullRequest> {
        self.pull_requests.iter().find(|pr| &pr.key == key)
    }

    /// The viewer's saved filters.
    pub fn saved_filters(&self) -> &[SavedFilter] {
        &self.saved_filters
    }

    /// The time relative ages are measured against; advances every 30 seconds.
    pub fn now(&self) -> Timestamp {
        self.now
    }

    /// When the list was last synced.
    pub fn last_synced(&self) -> Option<Timestamp> {
        self.last_synced
    }

    /// The review data for `key`, if it was requested.
    pub fn detail(&self, key: &PullRequestKey) -> Option<&DetailState> {
        self.details.get(key)
    }

    /// The loaded review data for `key`.
    pub fn loaded_detail(&self, key: &PullRequestKey) -> Option<&PullRequestDetail> {
        match self.details.get(key) {
            Some(DetailState::Loaded(detail)) => Some(detail),
            _ => None,
        }
    }

    /// Loads the review data for `key` unless it is loaded or loading. A cached copy
    /// shows first when there is one; GitHub's answer then replaces it, keeping what only
    /// exists here: pending comments, findings, agent reviews and viewed files.
    pub fn load_detail(&mut self, key: &PullRequestKey, cx: &mut Context<Self>) {
        if matches!(
            self.details.get(key),
            Some(DetailState::Loading | DetailState::Loaded(_))
        ) {
            return;
        }
        self.details.insert(key.clone(), DetailState::Loading);
        let (fetch, checks) = self.fetch_detail_and_checks(key, cx);
        let cache = self.cache.clone();
        let cache_key = key.clone();
        let cached = cx
            .background_spawn(async move { cache.and_then(|cache| cache.load_detail(&cache_key)) });
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            // The cached copy lands first even when GitHub is faster, so its local-only
            // data is always there for the fresh copy to carry over.
            if let Some(cached) = cached.await {
                this.update(cx, |this, cx| {
                    if matches!(this.details.get(&key), Some(DetailState::Loading)) {
                        this.details
                            .insert(key.clone(), DetailState::Loaded(Box::new(cached.data)));
                        cx.emit(PullRequestStoreEvent::DetailChanged(key.clone()));
                        cx.notify();
                    }
                })
                .ok();
            }
            let result = fetch.await;
            this.update(cx, |this, cx| {
                let state = match (result, this.details.remove(&key)) {
                    (Ok(mut fresh), Some(DetailState::Loaded(current))) => {
                        carry_local(&mut fresh, &current);
                        keep_checks_until_fresh(&mut fresh, &current);
                        DetailState::Loaded(Box::new(fresh))
                    }
                    (Ok(fresh), _) => DetailState::Loaded(Box::new(fresh)),
                    // Offline: the cached copy is better than an error.
                    (Err(_), Some(cached @ DetailState::Loaded(_))) => cached,
                    (Err(error), _) => DetailState::Failed(error.to_string()),
                };
                let loaded = matches!(state, DetailState::Loaded(_));
                this.details.insert(key.clone(), state);
                if loaded {
                    this.save_detail(&key, cx);
                }
                this.apply_checks_when_ready(&key, checks, cx);
                cx.emit(PullRequestStoreEvent::DetailChanged(key));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Starts fetching `key`'s detail and its checks at the same time.
    fn fetch_detail_and_checks(
        &self,
        key: &PullRequestKey,
        cx: &mut Context<Self>,
    ) -> (Fetch<PullRequestDetail>, Fetch<Vec<CheckRun>>) {
        let source = self.source.clone();
        let request_key = key.clone();
        let detail = cx.background_spawn(async move { source.detail(&request_key) });
        let source = self.source.clone();
        let request_key = key.clone();
        let checks = cx.background_spawn(async move { source.checks(&request_key) });
        (detail, checks)
    }

    /// What a screen saved for `key` under `name`: this run's copy, or the one on disk.
    pub fn load_view_state<T: DeserializeOwned + Send + 'static>(
        &self,
        key: &PullRequestKey,
        name: &'static str,
        cx: &App,
    ) -> Task<Option<T>> {
        if let Some(value) = self.view_states.get(&(key.clone(), name)) {
            return Task::ready(serde_json::from_value(value.clone()).ok());
        }
        let cache = self.cache.clone();
        let key = key.clone();
        cx.background_spawn(async move { cache?.load_view(&key, name) })
    }

    /// Keeps `value` for `key` under `name` and writes it to disk once saves have
    /// settled. Unlike edits to the review data, this tells no one: views already
    /// show what they save.
    pub fn save_view_state<T: Serialize>(
        &mut self,
        key: &PullRequestKey,
        name: &'static str,
        value: &T,
        cx: &mut Context<Self>,
    ) {
        let Ok(value) = serde_json::to_value(value) else {
            return;
        };
        let slot = (key.clone(), name);
        self.view_states.insert(slot.clone(), value.clone());
        let Some(cache) = self.cache.clone() else {
            return;
        };
        let key = key.clone();
        let task = cx.background_spawn({
            let timer = cx.background_executor().timer(SAVE_DELAY);
            async move {
                timer.await;
                cache.save_view(&key, name, &value).ok();
            }
        });
        self.view_saves.insert(slot, task);
    }

    /// Replaces the file diffs of `key`, keeping everything else, as when diffs are
    /// recomputed from a local clone. Paths missing from `files` keep their old diff.
    pub fn replace_files(
        &mut self,
        key: &PullRequestKey,
        files: Vec<diff_model::FileDiff>,
        cx: &mut Context<Self>,
    ) {
        self.update_detail(key, cx, |detail| {
            for local in files {
                if let Some(file) = detail.files.iter_mut().find(|f| f.path == local.path) {
                    *file = local;
                }
            }
        });
    }

    /// Marks `path` viewed or not viewed.
    pub fn set_viewed(
        &mut self,
        key: &PullRequestKey,
        path: &str,
        viewed: bool,
        cx: &mut Context<Self>,
    ) {
        self.update_detail(key, cx, |detail| {
            detail.viewed.retain(|candidate| candidate != path);
            if viewed {
                detail.viewed.push(path.to_string());
            }
        });
    }

    /// Adds the viewer's reply to a thread as part of the pending review.
    pub fn reply(
        &mut self,
        key: &PullRequestKey,
        thread_id: &str,
        viewer: &Actor,
        body: String,
        cx: &mut Context<Self>,
    ) {
        let comment = self.new_comment(viewer, body).pending();
        self.update_detail(key, cx, |detail| {
            if let Some(thread) = detail.threads.iter_mut().find(|t| t.id == thread_id) {
                thread.comments.push(comment);
            }
        });
    }

    /// Starts a new pending thread on a line of the head side.
    pub fn start_thread(
        &mut self,
        key: &PullRequestKey,
        path: &str,
        line: u32,
        viewer: &Actor,
        body: String,
        cx: &mut Context<Self>,
    ) {
        let comment = self.new_comment(viewer, body).pending();
        let id = format!("thread-{}", comment.id);
        self.update_detail(key, cx, |detail| {
            detail
                .threads
                .push(ReviewThread::new(id, path, line, vec![comment]));
        });
    }

    /// Posts a comment on the conversation right away, outside the pending review.
    pub fn comment(
        &mut self,
        key: &PullRequestKey,
        viewer: &Actor,
        body: String,
        cx: &mut Context<Self>,
    ) {
        let comment = self.new_comment(viewer, body);
        self.update_detail(key, cx, |detail| {
            detail.timeline.push(TimelineItem::Commented { comment });
        });
    }

    /// Resolves or reopens a thread.
    pub fn set_resolved(
        &mut self,
        key: &PullRequestKey,
        thread_id: &str,
        resolved: bool,
        cx: &mut Context<Self>,
    ) {
        self.update_detail(key, cx, |detail| {
            if let Some(thread) = detail.threads.iter_mut().find(|t| t.id == thread_id) {
                thread.is_resolved = resolved;
            }
        });
    }

    /// Moves an agent finding into or out of the pending review.
    pub fn set_finding_state(
        &mut self,
        key: &PullRequestKey,
        finding_id: &str,
        state: FindingState,
        cx: &mut Context<Self>,
    ) {
        self.update_detail(key, cx, |detail| {
            if let Some(finding) = detail.findings.iter_mut().find(|f| f.id == finding_id) {
                finding.state = state;
            }
        });
    }

    /// Replaces the text of an agent finding before it is posted.
    pub fn edit_finding(
        &mut self,
        key: &PullRequestKey,
        finding_id: &str,
        body: String,
        cx: &mut Context<Self>,
    ) {
        self.update_detail(key, cx, |detail| {
            if let Some(finding) = detail.findings.iter_mut().find(|f| f.id == finding_id) {
                finding.body = body;
            }
        });
    }

    /// Adds a finding an agent reported during a run, and a timeline entry for it.
    pub fn add_finding(
        &mut self,
        key: &PullRequestKey,
        finding: AgentFinding,
        cx: &mut Context<Self>,
    ) {
        self.update_detail(key, cx, |detail| {
            detail.timeline.push(TimelineItem::Finding {
                id: finding.id.clone(),
            });
            detail.findings.push(finding);
        });
    }

    /// Records that an agent review is running or finished for the reviewer list.
    pub fn set_agent_reviewing(
        &mut self,
        key: &PullRequestKey,
        agent: &Actor,
        reviewing: bool,
        cx: &mut Context<Self>,
    ) {
        self.update_detail(key, cx, |detail| {
            match detail.reviewers.iter_mut().find(|r| &r.actor == agent) {
                Some(reviewer) => {
                    reviewer.state = if reviewing {
                        ReviewerState::Reviewing
                    } else {
                        ReviewerState::Commented
                    }
                }
                None if reviewing => detail.reviewers.push(review_model::Reviewer::new(
                    agent.clone(),
                    ReviewerState::Reviewing,
                )),
                None => {}
            }
        });
    }

    /// Appends a finished agent review to the timeline.
    pub fn finish_agent_review(
        &mut self,
        key: &PullRequestKey,
        summary: review_model::AgentReviewSummary,
        cx: &mut Context<Self>,
    ) {
        let agent = summary.agent.clone();
        self.update_detail(key, cx, |detail| {
            detail.last_agent_review = Some(summary.clone());
            detail.timeline.push(TimelineItem::AgentReviewed(summary));
        });
        self.set_agent_reviewing(key, &agent, false, cx);
    }

    /// Drops the viewer's pending comments and returns added findings to drafts.
    pub fn discard_pending(&mut self, key: &PullRequestKey, cx: &mut Context<Self>) {
        self.update_detail(key, cx, |detail| {
            for thread in &mut detail.threads {
                thread.comments.retain(|comment| !comment.is_pending);
            }
            detail.threads.retain(|thread| !thread.comments.is_empty());
            for finding in &mut detail.findings {
                if finding.state == FindingState::Added {
                    finding.state = FindingState::Draft;
                }
            }
        });
    }

    /// Submits the viewer's review with its pending comments and added findings. The
    /// returned task resolves once GitHub accepted it; local state changes only then.
    pub fn submit_review(
        &mut self,
        key: &PullRequestKey,
        viewer: &Actor,
        verdict: ReviewVerdict,
        summary: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), ApiError>> {
        let Some(detail) = self.loaded_detail(key) else {
            return Task::ready(Err(ApiError::new("The pull request isn’t loaded.")));
        };
        let submission = submission_for(detail, verdict, summary.clone());
        let source = self.source.clone();
        let key = key.clone();
        let viewer = viewer.clone();
        cx.spawn(async move |this, cx| {
            let request_key = key.clone();
            cx.background_spawn(async move { source.submit_review(&request_key, &submission) })
                .await?;
            this.update(cx, |this, cx| {
                let summary_comment =
                    (!summary.trim().is_empty()).then(|| this.new_comment(&viewer, summary));
                let now = Timestamp::now();
                this.update_detail(&key, cx, |detail| {
                    apply_submitted_review(detail, &viewer, verdict, summary_comment, now)
                });
            })
            .map_err(|_| ApiError::new("The window closed before the review was saved."))
        })
    }

    /// Re-runs a check's failed jobs and shows it running until the next poll.
    pub fn rerun_check(&mut self, key: &PullRequestKey, check_id: &str, cx: &mut Context<Self>) {
        let source = self.source.clone();
        let request_key = key.clone();
        let request_check = check_id.to_string();
        cx.background_spawn(async move { source.rerun_failed_jobs(&request_key, &request_check) })
            .detach();
        self.update_detail(key, cx, |detail| {
            if let Some(check) = detail.checks.iter_mut().find(|c| c.id == check_id) {
                check.status = CheckStatus::Running;
                check.elapsed_secs = 0;
                for step in &mut check.steps {
                    if step.status != CheckStatus::Passed {
                        step.status = CheckStatus::Queued;
                        step.duration_secs = None;
                        step.log.clear();
                    }
                }
            }
            refresh_head_ci(detail);
        });
    }

    /// Fetches fresh check runs for `key` now, outside the polling schedule.
    pub fn refresh_checks(&mut self, key: &PullRequestKey, cx: &mut Context<Self>) {
        let source = self.source.clone();
        let request_key = key.clone();
        let checks = cx.background_spawn(async move { source.checks(&request_key) });
        self.apply_checks_when_ready(key, checks, cx);
    }

    /// Fetches the log of check `check_id` on `key` and puts its tail on the failed step,
    /// unless it is being fetched already. A failed fetch leaves the step without one.
    pub fn load_check_log(&mut self, key: &PullRequestKey, check_id: &str, cx: &mut Context<Self>) {
        let request = (key.clone(), check_id.to_string());
        if !self.log_requests.insert(request.clone()) {
            return;
        }
        let source = self.source.clone();
        let log = cx.background_spawn({
            let (key, check_id) = request.clone();
            async move { source.check_log(&key, &check_id) }
        });
        cx.spawn(async move |this, cx| {
            let result = log.await;
            this.update(cx, |this, cx| {
                this.log_requests.remove(&request);
                let (key, check_id) = &request;
                match result {
                    Ok(lines) => {
                        this.update_detail(key, cx, |detail| put_check_log(detail, check_id, lines))
                    }
                    Err(_) => cx.notify(),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Whether the log of check `check_id` on `key` is being fetched.
    pub fn check_log_loading(&self, key: &PullRequestKey, check_id: &str) -> bool {
        self.log_requests
            .contains(&(key.clone(), check_id.to_string()))
    }

    /// Applies `checks` to `key`'s loaded detail once they arrive; a failed fetch leaves
    /// the shown checks alone.
    fn apply_checks_when_ready(
        &self,
        key: &PullRequestKey,
        checks: Fetch<Vec<CheckRun>>,
        cx: &mut Context<Self>,
    ) {
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let Ok(fresh) = checks.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.update_detail(&key, cx, |detail| apply_checks(detail, fresh));
            })
            .ok();
        })
        .detach();
    }

    /// Fetches fresh check runs for every loaded pull request that has a running check.
    fn poll_checks(&mut self, cx: &mut Context<Self>) {
        let running: Vec<PullRequestKey> = self
            .details
            .iter()
            .filter_map(|(key, state)| match state {
                DetailState::Loaded(detail)
                    if detail.checks.iter().any(|c| !c.status.is_finished()) =>
                {
                    Some(key.clone())
                }
                _ => None,
            })
            .collect();
        for key in running {
            self.refresh_checks(&key, cx);
        }
    }

    fn new_comment(&mut self, viewer: &Actor, body: String) -> Comment {
        self.next_comment_id += 1;
        Comment::new(
            format!("local-{}-{}", self.session, self.next_comment_id),
            viewer.clone(),
            body,
            Timestamp::now(),
        )
    }

    /// Approves `key`. With its detail loaded, pending comments and added findings post
    /// with the approval; from the list, the approval posts on its own.
    pub fn approve(
        &mut self,
        key: &PullRequestKey,
        viewer: &Actor,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), ApiError>> {
        if self.loaded_detail(key).is_some() {
            return self.submit_review(key, viewer, ReviewVerdict::Approve, String::new(), cx);
        }
        let source = self.source.clone();
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let request_key = key.clone();
            cx.background_spawn(async move {
                source.submit_review(
                    &request_key,
                    &ReviewSubmission::new(ReviewVerdict::Approve, "", Vec::new(), Vec::new()),
                )
            })
            .await?;
            this.update(cx, |this, cx| {
                if let Some(pr) = this.pull_requests.iter_mut().find(|pr| pr.key == key) {
                    pr.review = ReviewDecision::Approved;
                }
                cx.emit(PullRequestStoreEvent::ListChanged);
                cx.notify();
            })
            .map_err(|_| ApiError::new("The window closed before the approval was saved."))
        })
    }

    /// Closes `key`, or reopens it when `open`, then shows the new state everywhere.
    pub fn set_open(
        &mut self,
        key: &PullRequestKey,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), ApiError>> {
        let source = self.source.clone();
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let request_key = key.clone();
            cx.background_spawn(async move { source.set_open(&request_key, open) })
                .await?;
            this.update(cx, |this, cx| {
                let state = if open {
                    PullRequestState::Open
                } else {
                    PullRequestState::Closed
                };
                if let Some(pr) = this.pull_requests.iter_mut().find(|pr| pr.key == key) {
                    pr.state = state;
                }
                this.update_detail(&key, cx, |detail| detail.summary.state = state);
                cx.emit(PullRequestStoreEvent::ListChanged);
                cx.notify();
            })
            .map_err(|_| ApiError::new("The window closed before the change was saved."))
        })
    }

    /// Fetches `key` again and replaces what is shown, keeping what only exists here,
    /// as after merging or updating its branch.
    pub fn reload_detail(&mut self, key: &PullRequestKey, cx: &mut Context<Self>) {
        let (fetch, checks) = self.fetch_detail_and_checks(key, cx);
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let Ok(mut fresh) = fetch.await else {
                return;
            };
            this.update(cx, |this, cx| {
                if let Some(DetailState::Loaded(current)) = this.details.get(&key) {
                    carry_local(&mut fresh, current);
                    keep_checks_until_fresh(&mut fresh, current);
                }
                this.details
                    .insert(key.clone(), DetailState::Loaded(Box::new(fresh)));
                this.save_detail(&key, cx);
                this.apply_checks_when_ready(&key, checks, cx);
                cx.emit(PullRequestStoreEvent::DetailChanged(key));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The loaded merge facts of `key`, or an error saying they aren't known yet.
    fn merge_info(&self, key: &PullRequestKey) -> Result<review_model::MergeInfo, ApiError> {
        self.loaded_detail(key)
            .and_then(|detail| detail.merge.clone())
            .ok_or_else(|| {
                ApiError::new("Switchfold doesn’t know yet how this pull request merges.")
            })
    }

    /// Merges `key` with `method`, guarded by the head commit Switchfold has seen. On
    /// success it shows as merged and reloads.
    pub fn merge(
        &mut self,
        key: &PullRequestKey,
        method: review_model::MergeMethod,
        title: String,
        message: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), ApiError>> {
        let info = match self.merge_info(key) {
            Ok(info) => info,
            Err(error) => return Task::ready(Err(error)),
        };
        let source = self.source.clone();
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let request_key = key.clone();
            cx.background_spawn(async move {
                source.merge(&request_key, method, &title, &message, &info.head_sha)
            })
            .await?;
            this.update(cx, |this, cx| {
                if let Some(pr) = this.pull_requests.iter_mut().find(|pr| pr.key == key) {
                    pr.state = PullRequestState::Merged;
                }
                this.update_detail(&key, cx, |detail| {
                    detail.summary.state = PullRequestState::Merged
                });
                this.reload_detail(&key, cx);
                cx.emit(PullRequestStoreEvent::ListChanged);
                cx.notify();
            })
            .map_err(|_| ApiError::new("The window closed before the merge was confirmed."))
        })
    }

    /// Turns auto-merge on for `key` with `method`, or off with `None`, then reloads.
    pub fn set_auto_merge(
        &mut self,
        key: &PullRequestKey,
        method: Option<review_model::MergeMethod>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), ApiError>> {
        let info = match self.merge_info(key) {
            Ok(info) => info,
            Err(error) => return Task::ready(Err(error)),
        };
        let source = self.source.clone();
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            cx.background_spawn(async move { source.set_auto_merge(&info.node_id, method) })
                .await?;
            this.update(cx, |this, cx| this.reload_detail(&key, cx))
                .map_err(|_| ApiError::new("The window closed before the change was saved."))
        })
    }

    /// Brings `key`'s branch up to date with its base, by merge or by rebase, then
    /// reloads.
    pub fn update_branch(
        &mut self,
        key: &PullRequestKey,
        rebase: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), ApiError>> {
        let info = match self.merge_info(key) {
            Ok(info) => info,
            Err(error) => return Task::ready(Err(error)),
        };
        let source = self.source.clone();
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            cx.background_spawn(async move {
                source.update_branch(&info.node_id, &info.head_sha, rebase)
            })
            .await?;
            this.update(cx, |this, cx| this.reload_detail(&key, cx))
                .map_err(|_| ApiError::new("The window closed before the change was saved."))
        })
    }

    /// Asks `reviewers` to review `key`; they show as pending reviewers once GitHub
    /// accepts.
    pub fn request_reviewers(
        &mut self,
        key: &PullRequestKey,
        reviewers: Vec<Actor>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), ApiError>> {
        let source = self.source.clone();
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let request_key = key.clone();
            let logins: Vec<String> = reviewers.iter().map(|r| r.login.clone()).collect();
            cx.background_spawn(async move { source.request_reviewers(&request_key, &logins) })
                .await?;
            this.update(cx, |this, cx| {
                this.update_detail(&key, cx, |detail| {
                    for reviewer in reviewers {
                        if !detail
                            .reviewers
                            .iter()
                            .any(|r| r.actor.login == reviewer.login)
                        {
                            detail.reviewers.push(review_model::Reviewer::new(
                                reviewer,
                                ReviewerState::Pending,
                            ));
                        }
                    }
                });
            })
            .map_err(|_| ApiError::new("The window closed before the request was sent."))
        })
    }

    /// The changes commit `sha` of `repo` makes, from GitHub.
    pub fn commit_files(
        &self,
        repo: &RepoSlug,
        sha: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<diff_model::FileDiff>, ApiError>> {
        let source = self.source.clone();
        let (repo, sha) = (repo.clone(), sha.to_string());
        cx.background_spawn(async move { source.commit_files(&repo, &sha) })
    }

    /// People who can be asked to review in `repo`, read from GitHub once per repository.
    pub fn assignable_users(
        &mut self,
        repo: &RepoSlug,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<Actor>, ApiError>> {
        if let Some(people) = self.assignable.get(repo) {
            return Task::ready(Ok(people.clone()));
        }
        let source = self.source.clone();
        let repo = repo.clone();
        cx.spawn(async move |this, cx| {
            let request_repo = repo.clone();
            let people = cx
                .background_spawn(async move { source.assignable_users(&request_repo) })
                .await?;
            this.update(cx, |this, _| {
                this.assignable.insert(repo, people.clone());
            })
            .ok();
            Ok(people)
        })
    }

    fn update_detail(
        &mut self,
        key: &PullRequestKey,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut PullRequestDetail),
    ) {
        if let Some(DetailState::Loaded(detail)) = self.details.get_mut(key) {
            update(detail);
            self.save_detail(key, cx);
            cx.emit(PullRequestStoreEvent::DetailChanged(key.clone()));
            cx.notify();
        }
    }

    fn save_list(&mut self, cx: &mut Context<Self>) {
        let Some(cache) = self.cache.clone() else {
            return;
        };
        let pull_requests = self.pull_requests.clone();
        let saved_at = self.now;
        cx.background_spawn(async move {
            cache.save_list(&pull_requests, saved_at).ok();
            cache.prune(CACHE_MAX_AGE);
        })
        .detach();
    }

    /// Writes `key`'s review data once edits have settled for [`SAVE_DELAY`], so a burst
    /// of findings or check updates costs one write.
    fn save_detail(&mut self, key: &PullRequestKey, cx: &mut Context<Self>) {
        let Some(cache) = self.cache.clone() else {
            return;
        };
        let key = key.clone();
        let task = cx.spawn({
            let key = key.clone();
            async move |this, cx| {
                cx.background_executor().timer(SAVE_DELAY).await;
                let Ok(Some(detail)) = this.update(cx, |this, _| this.loaded_detail(&key).cloned())
                else {
                    return;
                };
                cx.background_spawn(async move {
                    cache.save_detail(&key, &detail, Timestamp::now()).ok();
                })
                .await;
            }
        });
        self.saves.insert(key, task);
    }
}

/// Moves what only exists in this app from `local` onto `fresh`, a copy just fetched
/// from GitHub: pending comments and threads, agent findings and reviews, agents in the
/// reviewer list, viewed files, and diffs recomputed from a local clone when the head
/// commit hasn't moved.
fn carry_local(fresh: &mut PullRequestDetail, local: &PullRequestDetail) {
    fresh.viewed = local.viewed.clone();
    let head = |detail: &PullRequestDetail| detail.commits.last().map(|c| c.sha.clone());
    if head(fresh).is_some() && head(fresh) == head(local) {
        fresh.files = local.files.clone();
    }
    for thread in &local.threads {
        let pending: Vec<Comment> = thread
            .comments
            .iter()
            .filter(|comment| comment.is_pending)
            .cloned()
            .collect();
        if pending.is_empty() {
            continue;
        }
        match fresh.threads.iter_mut().find(|t| t.id == thread.id) {
            Some(existing) => existing.comments.extend(pending),
            None => {
                let mut kept = thread.clone();
                kept.comments = pending;
                fresh.threads.push(kept);
            }
        }
    }
    for finding in &local.findings {
        if detail_lacks(&fresh.findings, &finding.id) {
            fresh.findings.push(finding.clone());
            fresh.timeline.push(TimelineItem::Finding {
                id: finding.id.clone(),
            });
        }
    }
    if fresh.last_agent_review.is_none() {
        fresh.last_agent_review = local.last_agent_review.clone();
        fresh.timeline.extend(
            local
                .timeline
                .iter()
                .filter(|item| matches!(item, TimelineItem::AgentReviewed(_)))
                .cloned(),
        );
    }
    for reviewer in &local.reviewers {
        if reviewer.actor.is_agent && !fresh.reviewers.iter().any(|r| r.actor == reviewer.actor) {
            fresh.reviewers.push(reviewer.clone());
        }
    }
}

/// What the review posts: new threads become line comments, pending replies to existing
/// threads become replies, and added findings become line comments.
fn submission_for(
    detail: &PullRequestDetail,
    verdict: ReviewVerdict,
    summary: String,
) -> ReviewSubmission {
    let mut new_comments = Vec::new();
    let mut replies = Vec::new();
    for thread in &detail.threads {
        let root_is_pending = thread.comments.first().is_some_and(|c| c.is_pending);
        for comment in thread.comments.iter().filter(|c| c.is_pending) {
            if root_is_pending {
                new_comments.push(DraftComment::new(
                    thread.path.clone(),
                    thread.line,
                    comment.body.clone(),
                ));
            } else if let Some(root) = thread.comments.first() {
                replies.push(Reply::new(root.id.clone(), comment.body.clone()));
            }
        }
    }
    new_comments.extend(
        detail
            .findings
            .iter()
            .filter(|f| f.state == FindingState::Added)
            .map(|f| DraftComment::new(f.path.clone(), f.line, f.body.clone())),
    );
    ReviewSubmission::new(verdict, summary, new_comments, replies)
}

/// Ids of the viewer's comments that post with the review.
#[cfg(test)]
pub(crate) fn pending_comment_ids(detail: &PullRequestDetail) -> Vec<String> {
    detail
        .threads
        .iter()
        .flat_map(|thread| &thread.comments)
        .filter(|comment| comment.is_pending)
        .map(|comment| comment.id.clone())
        .collect()
}

fn apply_submitted_review(
    detail: &mut PullRequestDetail,
    viewer: &Actor,
    verdict: ReviewVerdict,
    summary: Option<Comment>,
    now: Timestamp,
) {
    for comment in detail
        .threads
        .iter_mut()
        .flat_map(|thread| &mut thread.comments)
    {
        comment.is_pending = false;
    }
    let posted: Vec<AgentFinding> = detail
        .findings
        .iter()
        .filter(|f| f.state == FindingState::Added)
        .cloned()
        .collect();
    for finding in posted {
        let comment = Comment::new(
            format!("posted-{}", finding.id),
            viewer.clone(),
            finding.body.clone(),
            now,
        );
        detail.threads.push(ReviewThread::new(
            format!("thread-{}", finding.id),
            finding.path.clone(),
            finding.line,
            vec![comment],
        ));
    }
    detail
        .findings
        .retain(|finding| finding.state != FindingState::Added);
    detail.timeline.retain(
        |item| !matches!(item, TimelineItem::Finding { id } if detail_lacks(&detail.findings, id)),
    );
    detail.timeline.push(TimelineItem::Reviewed {
        actor: viewer.clone(),
        verdict,
        at: now,
    });
    if let Some(comment) = summary {
        detail.timeline.push(TimelineItem::Commented { comment });
    }
    let state = match verdict {
        ReviewVerdict::Approve => ReviewerState::Approved,
        ReviewVerdict::RequestChanges => ReviewerState::ChangesRequested,
        ReviewVerdict::Comment => ReviewerState::Commented,
    };
    if let Some(reviewer) = detail.reviewers.iter_mut().find(|r| r.is_viewer) {
        reviewer.state = state;
    }
}

fn detail_lacks(findings: &[AgentFinding], id: &str) -> bool {
    !findings.iter().any(|finding| finding.id == id)
}

/// Shows `current`'s checks on `fresh` when it came without any, so they don't blink
/// out while fresh checks load.
fn keep_checks_until_fresh(fresh: &mut PullRequestDetail, current: &PullRequestDetail) {
    if fresh.checks.is_empty() {
        apply_checks(fresh, current.checks.clone());
    }
}

/// Puts a job's log tail on its first failed step that has no log yet.
fn put_check_log(detail: &mut PullRequestDetail, check_id: &str, lines: Vec<LogLine>) {
    let step = detail
        .checks
        .iter_mut()
        .find(|check| check.id == check_id)
        .and_then(|check| {
            check
                .steps
                .iter_mut()
                .find(|step| step.status == CheckStatus::Failed && step.log.is_empty())
        });
    if let Some(step) = step {
        step.log = lines;
    }
}

/// Replaces `detail`'s checks with `fresh`. A log fetched earlier stays on its step
/// while the fresh copy has none, and failing or running checks get a timeline entry.
fn apply_checks(detail: &mut PullRequestDetail, mut fresh: Vec<CheckRun>) {
    for check in &mut fresh {
        let Some(old) = detail.checks.iter().find(|old| old.id == check.id) else {
            continue;
        };
        for step in check.steps.iter_mut().filter(|step| step.log.is_empty()) {
            if let Some(old_step) = old.steps.iter().find(|old| old.name == step.name) {
                step.log = old_step.log.clone();
            }
        }
    }
    detail.timeline.retain(|item| match item {
        TimelineItem::Check { id } => fresh.iter().any(|check| &check.id == id),
        _ => true,
    });
    let shown: Vec<String> = detail
        .timeline
        .iter()
        .filter_map(|item| match item {
            TimelineItem::Check { id } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let joining: Vec<TimelineItem> = fresh
        .iter()
        .filter(|check| matches!(check.status, CheckStatus::Failed | CheckStatus::Running))
        .filter(|check| !shown.contains(&check.id))
        .map(|check| TimelineItem::Check {
            id: check.id.clone(),
        })
        .collect();
    detail.timeline.extend(joining);
    detail.checks = fresh;
    refresh_head_ci(detail);
}

/// Sets the head commit's and the summary's CI status from the check runs.
fn refresh_head_ci(detail: &mut PullRequestDetail) {
    let statuses: Vec<CheckStatus> = detail.checks.iter().map(|c| c.status).collect();
    let combined = if statuses.contains(&CheckStatus::Failed) {
        Some(CheckStatus::Failed)
    } else if statuses
        .iter()
        .any(|s| matches!(s, CheckStatus::Running | CheckStatus::Queued))
    {
        Some(CheckStatus::Running)
    } else if statuses.is_empty() {
        None
    } else {
        Some(CheckStatus::Passed)
    };
    if let Some(head) = detail.commits.last_mut() {
        head.ci = combined;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pr_model::RepoSlug;

    fn sample() -> PullRequestDetail {
        let data = fixtures::sample(
            Timestamp::from_unix(2_000_000_000),
            std::path::Path::new("/u"),
        );
        data.details
            .into_iter()
            .find(|d| {
                d.summary.key == PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482)
            })
            .unwrap()
    }

    #[test]
    fn submitting_posts_pending_comments_and_added_findings() {
        let mut detail = sample();
        assert_eq!(pending_comment_ids(&detail).len(), 2);
        let submission = submission_for(&detail, ReviewVerdict::Comment, String::new());
        assert_eq!(submission.replies.len(), 2);
        assert_eq!(submission.new_comments.len(), 1);
        let threads_before = detail.threads.len();
        apply_submitted_review(
            &mut detail,
            &Actor::person("flaticols"),
            ReviewVerdict::RequestChanges,
            None,
            Timestamp::from_unix(2_000_000_100),
        );
        assert!(pending_comment_ids(&detail).is_empty());
        assert_eq!(detail.threads.len(), threads_before + 1);
        assert!(
            detail
                .findings
                .iter()
                .all(|f| f.state != FindingState::Added)
        );
        let viewer = detail.reviewers.iter().find(|r| r.is_viewer).unwrap();
        assert_eq!(viewer.state, ReviewerState::ChangesRequested);
        assert!(matches!(
            detail.timeline.last(),
            Some(TimelineItem::Reviewed {
                verdict: ReviewVerdict::RequestChanges,
                ..
            })
        ));
    }

    #[test]
    fn a_fresh_copy_keeps_what_only_exists_here() {
        let mut local = sample();
        local.viewed = vec!["src/lib.rs".into()];
        local.findings.push(AgentFinding::new(
            "f-local",
            Actor::agent("Codex"),
            "src/lib.rs",
            3,
            "Off by one",
        ));
        let mut fresh = sample();
        for thread in &mut fresh.threads {
            thread.comments.retain(|comment| !comment.is_pending);
        }
        fresh.threads.retain(|thread| !thread.comments.is_empty());
        fresh.findings.clear();
        fresh
            .timeline
            .retain(|item| !matches!(item, TimelineItem::Finding { .. }));
        fresh.viewed.clear();
        carry_local(&mut fresh, &local);
        assert_eq!(pending_comment_ids(&fresh), pending_comment_ids(&local));
        assert_eq!(fresh.findings.len(), local.findings.len());
        assert_eq!(
            fresh
                .timeline
                .iter()
                .filter(|item| matches!(item, TimelineItem::Finding { .. }))
                .count(),
            local.findings.len()
        );
        assert_eq!(fresh.viewed, local.viewed);
    }

    #[test]
    fn a_new_head_keeps_github_diffs() {
        let local = sample();
        let mut fresh = sample();
        fresh
            .commits
            .push(review_model::Commit::new("newhead", "Push", None));
        fresh.files.truncate(1);
        carry_local(&mut fresh, &local);
        assert_eq!(fresh.files.len(), 1);
    }

    #[test]
    fn a_failed_check_fails_the_head_commit() {
        let mut detail = sample();
        let failed = fixtures::snapshot_check(CheckStatus::Failed, 62);
        apply_checks(&mut detail, vec![failed]);
        assert_eq!(detail.commits.last().unwrap().ci, Some(CheckStatus::Failed));
    }

    fn check_items(detail: &PullRequestDetail) -> Vec<String> {
        detail
            .timeline
            .iter()
            .filter_map(|item| match item {
                TimelineItem::Check { id } => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    fn failed_step_log(check: &CheckRun) -> Vec<String> {
        check
            .steps
            .iter()
            .filter(|step| step.status == CheckStatus::Failed)
            .flat_map(|step| step.log.iter().map(|line| line.text.clone()))
            .collect()
    }

    #[test]
    fn applied_checks_replace_the_old_ones() {
        let mut detail = sample();
        apply_checks(
            &mut detail,
            vec![fixtures::snapshot_check(CheckStatus::Failed, 62)],
        );
        apply_checks(&mut detail, Vec::new());
        assert!(detail.checks.is_empty());
    }

    #[test]
    fn a_dropped_check_leaves_the_timeline() {
        let mut detail = sample();
        apply_checks(
            &mut detail,
            vec![fixtures::snapshot_check(CheckStatus::Failed, 62)],
        );
        apply_checks(&mut detail, Vec::new());
        assert_eq!(check_items(&detail), Vec::<String>::new());
    }

    #[test]
    fn a_failing_check_joins_the_timeline_once() {
        let mut detail = sample();
        let failed = fixtures::snapshot_check(CheckStatus::Failed, 62);
        apply_checks(&mut detail, vec![failed.clone()]);
        apply_checks(&mut detail, vec![failed.clone()]);
        assert_eq!(check_items(&detail), vec![failed.id]);
    }

    #[test]
    fn cached_checks_stay_until_fresh_ones_land() {
        let mut current = sample();
        apply_checks(
            &mut current,
            vec![fixtures::snapshot_check(CheckStatus::Failed, 62)],
        );
        let mut fresh = sample();
        apply_checks(&mut fresh, Vec::new());
        keep_checks_until_fresh(&mut fresh, &current);
        assert_eq!(check_items(&fresh), check_items(&current));
    }

    #[test]
    fn a_fetched_log_lands_on_the_failed_step() {
        let mut detail = sample();
        let failed = fixtures::snapshot_check(CheckStatus::Failed, 62);
        let mut polled = failed.clone();
        for step in &mut polled.steps {
            step.log.clear();
        }
        apply_checks(&mut detail, Vec::new());
        apply_checks(&mut detail, vec![polled]);
        let lines = failed
            .steps
            .iter()
            .flat_map(|step| step.log.clone())
            .collect();
        put_check_log(&mut detail, &failed.id, lines);
        assert_eq!(failed_step_log(&detail.checks[0]), failed_step_log(&failed));
    }

    #[test]
    fn a_fetched_log_survives_a_poll() {
        let mut detail = sample();
        let failed = fixtures::snapshot_check(CheckStatus::Failed, 62);
        let mut polled = failed.clone();
        for step in &mut polled.steps {
            step.log.clear();
        }
        apply_checks(&mut detail, vec![failed.clone()]);
        apply_checks(&mut detail, vec![polled]);
        assert_eq!(failed_step_log(&detail.checks[0]), failed_step_log(&failed));
    }
}

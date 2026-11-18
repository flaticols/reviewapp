//! Works through the viewer's merge queue while the app runs: every 30 seconds it asks
//! GitHub where each queued pull request stands and takes one step per repository.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use github_api::{ApiError, PullRequestSource};
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Task};
use pr_model::{PullRequestKey, RepoSlug};
use review_model::{AutoMerge, MergeStatus};
use settings_model::Preferences;

use crate::SettingsStore;
use crate::merge_plan::{Action, RowState, Skip, plan_repository};

/// How often the queue asks GitHub where its pull requests stand.
const POLL: Duration = Duration::from_secs(30);

/// What the queue did, for the window to tell the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeQueueEvent {
    Merged(PullRequestKey),
    Closed(PullRequestKey),
    Skipped(PullRequestKey, Skip),
    Failed(PullRequestKey, String),
}

/// The runner behind the merge queue. The order itself lives in the viewer's
/// preferences; this keeps what it learned from GitHub and what it is waiting on.
pub struct MergeQueue {
    source: Arc<dyn PullRequestSource>,
    settings: Entity<SettingsStore>,
    statuses: HashMap<PullRequestKey, MergeStatus>,
    rows: HashMap<PullRequestKey, RowState>,
    in_flight: HashMap<RepoSlug, PullRequestKey>,
    /// The head each pull request had before the queue rebased it.
    rebased_from: HashMap<PullRequestKey, String>,
    /// The last failure per pull request, so a repeat isn't reported again.
    errors: HashMap<PullRequestKey, String>,
    ticking: bool,
    tick_again: bool,
    _poll: Task<()>,
}

impl EventEmitter<MergeQueueEvent> for MergeQueue {}

impl MergeQueue {
    /// Creates the runner and starts its 30-second poll.
    pub fn new(
        source: Arc<dyn PullRequestSource>,
        settings: Entity<SettingsStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            source,
            settings,
            statuses: HashMap::new(),
            rows: HashMap::new(),
            in_flight: HashMap::new(),
            rebased_from: HashMap::new(),
            errors: HashMap::new(),
            ticking: false,
            tick_again: false,
            _poll: poll,
        }
    }

    /// Every queued pull request, in merge order.
    pub fn queue(&self, cx: &App) -> Vec<PullRequestKey> {
        self.settings
            .read(cx)
            .preferences()
            .merge_queue
            .iter()
            .filter_map(|id| PullRequestKey::parse(id))
            .collect()
    }

    /// Whether `key` is queued.
    pub fn contains(&self, key: &PullRequestKey, cx: &App) -> bool {
        self.settings.read(cx).preferences().is_queued(&key.id())
    }

    /// How `key` stands; `Queued` until GitHub has been asked about it.
    pub fn row(&self, key: &PullRequestKey) -> RowState {
        self.rows.get(key).copied().unwrap_or(RowState::Queued)
    }

    /// Queues `key` last in its repository and starts on it if it's first.
    pub fn add(&mut self, key: PullRequestKey, cx: &mut Context<Self>) {
        self.update_queue(cx, |preferences| preferences.set_queued(&key.id(), true));
        self.tick(cx);
    }

    /// Takes `key` out of the queue and turns off auto-merge the viewer turned on for it.
    pub fn remove(&mut self, key: &PullRequestKey, cx: &mut Context<Self>) {
        self.update_queue(cx, |preferences| preferences.set_queued(&key.id(), false));
        let viewer = self.viewer(cx);
        if let Some(status) = self.statuses.get(key)
            && status
                .merge
                .auto_merge
                .as_ref()
                .is_some_and(|auto| auto.enabled_by == viewer)
        {
            let action = Action::DisableAutoMerge {
                key: key.clone(),
                node_id: status.merge.node_id.clone(),
            };
            let source = self.source.clone();
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_spawn({
                        let action = action.clone();
                        async move { run(source.as_ref(), &action) }
                    })
                    .await;
                this.update(cx, |this, cx| this.finished(&action, result, cx))
                    .ok();
            })
            .detach();
        }
        self.forget(key);
        cx.notify();
    }

    /// Moves `key` above or below the next pull request of its repository. The new
    /// first one takes over at once.
    pub fn move_by(&mut self, key: &PullRequestKey, up: bool, cx: &mut Context<Self>) {
        self.update_queue(cx, |preferences| preferences.move_queued(&key.id(), up));
        self.in_flight.remove(key.repo());
        self.tick(cx);
    }

    /// Asks GitHub where every queued pull request stands and takes the next step in
    /// each repository. A tick asked for while one runs follows it.
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            self.tick_again = true;
            return;
        }
        let keys = self.queue(cx);
        if keys.is_empty() {
            return;
        }
        self.ticking = true;
        let source = self.source.clone();
        cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_spawn({
                    let source = source.clone();
                    let keys = keys.clone();
                    async move { source.merge_statuses(&keys) }
                })
                .await;
            let actions = this
                .update(cx, |this, cx| match fetched {
                    Ok(statuses) => this.apply(&keys, statuses, cx),
                    // Offline: keep showing what was last known.
                    Err(_) => Vec::new(),
                })
                .unwrap_or_default();
            for action in actions {
                let result = cx
                    .background_spawn({
                        let source = source.clone();
                        let action = action.clone();
                        async move { run(source.as_ref(), &action) }
                    })
                    .await;
                if this
                    .update(cx, |this, cx| this.finished(&action, result, cx))
                    .is_err()
                {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                this.ticking = false;
                cx.notify();
                if std::mem::take(&mut this.tick_again) {
                    this.tick(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Plans every repository from fresh `statuses`, takes merged and closed pull
    /// requests out, and returns what to ask GitHub.
    fn apply(
        &mut self,
        fetched: &[PullRequestKey],
        statuses: Vec<MergeStatus>,
        cx: &mut Context<Self>,
    ) -> Vec<Action> {
        self.statuses = statuses
            .into_iter()
            .map(|status| (status.key.clone(), status))
            .collect();
        let viewer = self.viewer(cx);
        // Pull requests queued after the request went out wait for the next tick.
        let queue: Vec<PullRequestKey> = self
            .queue(cx)
            .into_iter()
            .filter(|key| fetched.contains(key))
            .collect();
        let mut repositories: Vec<(RepoSlug, Vec<PullRequestKey>)> = Vec::new();
        for key in queue {
            match repositories.iter_mut().find(|(repo, _)| repo == key.repo()) {
                Some((_, keys)) => keys.push(key),
                None => repositories.push((key.repo().clone(), vec![key])),
            }
        }

        let mut requests = Vec::new();
        for (repo, keys) in repositories {
            let plan = plan_repository(
                &keys,
                &self.statuses,
                self.in_flight.get(&repo),
                &self.rebased_from,
                &viewer,
            );
            match plan.in_flight {
                Some(key) => self.in_flight.insert(repo, key),
                None => self.in_flight.remove(&repo),
            };
            for (key, row) in plan.rows {
                if let RowState::Skipped(skip) = row
                    && !matches!(self.rows.get(&key), Some(RowState::Skipped(_)))
                {
                    cx.emit(MergeQueueEvent::Skipped(key.clone(), skip));
                }
                self.rows.insert(key, row);
            }
            for action in plan.actions {
                match action {
                    Action::Remove { key, merged } => {
                        self.update_queue(cx, |preferences| {
                            preferences.set_queued(&key.id(), false)
                        });
                        self.forget(&key);
                        cx.emit(if merged {
                            MergeQueueEvent::Merged(key)
                        } else {
                            MergeQueueEvent::Closed(key)
                        });
                    }
                    request => requests.push(request),
                }
            }
        }
        cx.notify();
        requests
    }

    /// Records how a request to GitHub went.
    fn finished(&mut self, action: &Action, result: Result<(), ApiError>, cx: &mut Context<Self>) {
        let key = action.key();
        match result {
            Ok(()) => {
                self.errors.remove(key);
                // Until GitHub is asked again, remember what changed, so removing a pull
                // request right away still turns its auto-merge off.
                let viewer = self.viewer(cx);
                match action {
                    Action::Rebase { head_sha, .. } => {
                        self.rebased_from.insert(key.clone(), head_sha.clone());
                    }
                    Action::EnableAutoMerge { method, .. } => {
                        if let Some(status) = self.statuses.get_mut(key) {
                            status.merge.auto_merge = Some(AutoMerge::new(*method, viewer));
                        }
                    }
                    Action::DisableAutoMerge { .. } => {
                        if let Some(status) = self.statuses.get_mut(key) {
                            status.merge.auto_merge = None;
                        }
                    }
                    Action::Merge { .. } | Action::Remove { .. } => {}
                }
            }
            Err(error) => {
                let message = error.to_string();
                if self.errors.get(key) != Some(&message) {
                    cx.emit(MergeQueueEvent::Failed(key.clone(), message.clone()));
                }
                self.errors.insert(key.clone(), message);
            }
        }
    }

    fn forget(&mut self, key: &PullRequestKey) {
        self.statuses.remove(key);
        self.rows.remove(key);
        self.rebased_from.remove(key);
        self.errors.remove(key);
        self.in_flight.retain(|_, in_flight| in_flight != key);
    }

    fn update_queue(&self, cx: &mut Context<Self>, update: impl FnOnce(&mut Preferences)) {
        self.settings
            .update(cx, |settings, cx| settings.update_preferences(cx, update));
    }

    fn viewer(&self, cx: &App) -> String {
        self.settings
            .read(cx)
            .viewer()
            .map(|account| account.login.clone())
            .unwrap_or_default()
    }
}

/// Asks GitHub to carry out `action`. Merges keep GitHub's default commit title and
/// message. A branch whose commits don't rebase cleanly, such as one with merge commits
/// from its base, is updated with a merge instead; both pin `head_sha`, so the fallback
/// never lands on a branch that moved.
fn run(source: &dyn PullRequestSource, action: &Action) -> Result<(), ApiError> {
    match action {
        Action::Rebase {
            node_id, head_sha, ..
        } => source
            .update_branch(node_id, head_sha, true)
            .or_else(|_| source.update_branch(node_id, head_sha, false)),
        Action::Merge {
            key,
            method,
            head_sha,
        } => source.merge(key, *method, "", "", head_sha),
        Action::EnableAutoMerge {
            node_id, method, ..
        } => source.set_auto_merge(node_id, Some(*method)),
        Action::DisableAutoMerge { node_id, .. } => source.set_auto_merge(node_id, None),
        Action::Remove { .. } => Ok(()),
    }
}

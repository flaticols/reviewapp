use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use app_state_ui::{PullRequestStoreEvent, StoresExt as _};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Subscription, Task};
use pr_model::PullRequestKey;
use serde::{Deserialize, Serialize};
use settings_model::TriageSettings;
use triage::{FindingTriage, PullRequestRisk, ThreadLabel, TriageProvider};

use crate::inputs;

/// How many questions are in flight at once.
const CONCURRENCY: usize = 4;
/// How long the store waits after a failure before asking again, so a bad key or an
/// outage isn't hit on every refresh.
const PAUSE_AFTER_FAILURE: Duration = Duration::from_secs(60);
/// How long answers settle before they're written to disk.
const SAVE_DELAY: Duration = Duration::from_secs(2);

/// An answer and the fingerprint of what was asked, so a change asks again.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Answered<T> {
    fingerprint: String,
    value: T,
}

#[derive(Default, Serialize, Deserialize)]
struct Results {
    findings: HashMap<String, Answered<FindingTriage>>,
    threads: HashMap<String, Answered<ThreadLabel>>,
    pull_requests: HashMap<String, Answered<PullRequestRisk>>,
}

enum Job {
    Finding(String, String, triage::FindingInput),
    Thread(String, String, triage::ThreadInput),
    PullRequest(String, String, triage::PullRequestInput),
}

impl Job {
    fn id(&self) -> &str {
        match self {
            Self::Finding(id, ..) | Self::Thread(id, ..) | Self::PullRequest(id, ..) => id,
        }
    }
}

enum Outcome {
    Finding(FindingTriage),
    Thread(ThreadLabel),
    PullRequest(PullRequestRisk),
}

/// Triage answers for findings, threads and pull requests, and the queue that asks
/// for them. Nothing is asked without a provider, and each feature runs only while
/// its setting is on.
pub struct TriageStore {
    provider: Option<Arc<dyn TriageProvider>>,
    results: Results,
    queue: VecDeque<Job>,
    queued: HashSet<String>,
    running: usize,
    last_error: Option<String>,
    paused_until: Option<Instant>,
    path: Option<PathBuf>,
    save: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

struct TriageGlobal(Entity<TriageStore>);

impl Global for TriageGlobal {}

/// Access to the installed [`TriageStore`].
pub trait TriageStoreExt {
    /// The store, once [`TriageStore::install`] ran.
    fn triage(&self) -> Option<Entity<TriageStore>>;
}

impl TriageStoreExt for App {
    fn triage(&self) -> Option<Entity<TriageStore>> {
        self.try_global::<TriageGlobal>()
            .map(|global| global.0.clone())
    }
}

fn finding_id(key: &PullRequestKey, id: &str) -> String {
    format!("{}#finding:{id}", key.id())
}

fn thread_id(key: &PullRequestKey, id: &str) -> String {
    format!("{}#thread:{id}", key.id())
}

impl TriageStore {
    /// Creates the store, loads answers saved under `cache_dir`, and starts watching
    /// the pull request and settings stores.
    pub fn install(cache_dir: Option<PathBuf>, cx: &mut App) -> Entity<TriageStore> {
        let path = cache_dir.map(|dir| dir.join("triage.json"));
        let results = path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let pull_requests = cx.stores().pull_requests().clone();
        let settings = cx.stores().settings().clone();
        let store = cx.new(|cx: &mut Context<TriageStore>| {
            let subscriptions = vec![
                cx.subscribe(
                    &pull_requests,
                    |this: &mut TriageStore, _, event, cx| match event {
                        PullRequestStoreEvent::ListChanged => this.scan_list(cx),
                        PullRequestStoreEvent::DetailChanged(key) => this.scan_detail(key, cx),
                    },
                ),
                cx.observe(&settings, |this: &mut TriageStore, _, cx| this.rescan(cx)),
            ];
            TriageStore {
                provider: None,
                results,
                queue: VecDeque::new(),
                queued: HashSet::new(),
                running: 0,
                last_error: None,
                paused_until: None,
                path,
                save: None,
                _subscriptions: subscriptions,
            }
        });
        cx.set_global(TriageGlobal(store.clone()));
        store
    }

    /// Uses `provider` from now on, or stops asking with `None`. Answers already given
    /// stay until what they're about changes.
    pub fn set_provider(
        &mut self,
        provider: Option<Arc<dyn TriageProvider>>,
        cx: &mut Context<Self>,
    ) {
        self.provider = provider;
        self.queue.clear();
        self.queued.clear();
        self.last_error = None;
        self.paused_until = None;
        self.rescan(cx);
        cx.notify();
    }

    /// Whether a provider is set.
    pub fn has_provider(&self) -> bool {
        self.provider.is_some()
    }

    /// The last failure, until the next success.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// How many items are waiting or being asked about.
    pub fn pending(&self) -> usize {
        self.queue.len() + self.running
    }

    /// The rating of finding `id` in `key`.
    pub fn finding(&self, key: &PullRequestKey, id: &str) -> Option<FindingTriage> {
        self.results
            .findings
            .get(&finding_id(key, id))
            .map(|answered| answered.value)
    }

    /// The label of thread `id` in `key`.
    pub fn thread(&self, key: &PullRequestKey, id: &str) -> Option<ThreadLabel> {
        self.results
            .threads
            .get(&thread_id(key, id))
            .map(|answered| answered.value)
    }

    /// The rating of pull request `key`.
    pub fn pull_request(&self, key: &PullRequestKey) -> Option<PullRequestRisk> {
        self.results
            .pull_requests
            .get(&key.id())
            .map(|answered| answered.value)
    }

    fn settings(cx: &App) -> TriageSettings {
        cx.stores().settings().read(cx).preferences().triage.clone()
    }

    fn fingerprint(&self, input: String) -> Option<String> {
        let provider = self.provider.as_ref()?;
        Some(format!("{}:{}:{input}", provider.id(), provider.model()))
    }

    /// Looks at everything loaded again, as after a setting or provider change.
    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.scan_list(cx);
        let keys: Vec<PullRequestKey> = cx
            .stores()
            .pull_requests()
            .read(cx)
            .pull_requests()
            .iter()
            .map(|pr| pr.key.clone())
            .collect();
        for key in keys {
            self.scan_detail(&key, cx);
        }
    }

    fn scan_list(&mut self, cx: &mut Context<Self>) {
        if self.provider.is_none() || !Self::settings(cx).pull_requests {
            return;
        }
        let jobs: Vec<Job> = cx
            .stores()
            .pull_requests()
            .read(cx)
            .pull_requests()
            .iter()
            .filter_map(|pr| {
                let input = inputs::pull_request(pr);
                let fingerprint = self.fingerprint(input.fingerprint())?;
                let id = pr.key.id();
                let fresh = self
                    .results
                    .pull_requests
                    .get(&id)
                    .is_some_and(|answered| answered.fingerprint == fingerprint);
                (!fresh).then_some(Job::PullRequest(id, fingerprint, input))
            })
            .collect();
        self.enqueue(jobs, cx);
    }

    fn scan_detail(&mut self, key: &PullRequestKey, cx: &mut Context<Self>) {
        if self.provider.is_none() {
            return;
        }
        let settings = Self::settings(cx);
        if !settings.findings && !settings.threads {
            return;
        }
        let store = cx.stores().pull_requests().read(cx);
        let Some(detail) = store.loaded_detail(key) else {
            return;
        };
        let mut jobs = Vec::new();
        if settings.findings {
            for finding in &detail.findings {
                let input = inputs::finding(detail, finding);
                let Some(fingerprint) = self.fingerprint(input.fingerprint()) else {
                    continue;
                };
                let id = finding_id(key, &finding.id);
                if self.results.findings.get(&id).map(|a| &a.fingerprint) != Some(&fingerprint) {
                    jobs.push(Job::Finding(id, fingerprint, input));
                }
            }
        }
        if settings.threads {
            for thread in detail.threads.iter().filter(|t| !t.comments.is_empty()) {
                let input = inputs::thread(detail, thread);
                let Some(fingerprint) = self.fingerprint(input.fingerprint()) else {
                    continue;
                };
                let id = thread_id(key, &thread.id);
                if self.results.threads.get(&id).map(|a| &a.fingerprint) != Some(&fingerprint) {
                    jobs.push(Job::Thread(id, fingerprint, input));
                }
            }
        }
        self.enqueue(jobs, cx);
    }

    fn enqueue(&mut self, jobs: Vec<Job>, cx: &mut Context<Self>) {
        if self
            .paused_until
            .is_some_and(|until| Instant::now() < until)
        {
            return;
        }
        for job in jobs {
            if self.queued.insert(job.id().to_string()) {
                self.queue.push_back(job);
            }
        }
        self.pump(cx);
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        while self.running < CONCURRENCY {
            let Some(provider) = self.provider.clone() else {
                return;
            };
            let Some(job) = self.queue.pop_front() else {
                return;
            };
            self.running += 1;
            let task = cx.background_spawn(async move {
                let outcome = match &job {
                    Job::Finding(_, _, input) => {
                        provider.triage_finding(input).map(Outcome::Finding)
                    }
                    Job::Thread(_, _, input) => provider.label_thread(input).map(Outcome::Thread),
                    Job::PullRequest(_, _, input) => {
                        provider.rate_pull_request(input).map(Outcome::PullRequest)
                    }
                };
                (job, outcome)
            });
            cx.spawn(async move |this, cx| {
                let (job, outcome) = task.await;
                this.update(cx, |this, cx| this.finish(job, outcome, cx))
                    .ok();
            })
            .detach();
        }
    }

    fn finish(
        &mut self,
        job: Job,
        outcome: Result<Outcome, triage::TriageError>,
        cx: &mut Context<Self>,
    ) {
        self.running -= 1;
        self.queued.remove(job.id());
        match (job, outcome) {
            (Job::Finding(id, fingerprint, _), Ok(Outcome::Finding(value))) => {
                self.results
                    .findings
                    .insert(id, Answered { fingerprint, value });
                self.last_error = None;
            }
            (Job::Thread(id, fingerprint, _), Ok(Outcome::Thread(value))) => {
                self.results
                    .threads
                    .insert(id, Answered { fingerprint, value });
                self.last_error = None;
            }
            (Job::PullRequest(id, fingerprint, _), Ok(Outcome::PullRequest(value))) => {
                self.results
                    .pull_requests
                    .insert(id, Answered { fingerprint, value });
                self.last_error = None;
            }
            (_, Err(error)) => {
                self.last_error = Some(error.to_string());
                self.paused_until = Some(Instant::now() + PAUSE_AFTER_FAILURE);
                self.queue.clear();
                self.queued.clear();
            }
            _ => {}
        }
        self.schedule_save(cx);
        cx.notify();
        self.pump(cx);
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let Ok(Some(bytes)) = this.update(cx, |this, _| serde_json::to_vec(&this.results).ok())
            else {
                return;
            };
            cx.background_spawn(async move {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                let temporary = path.with_extension("json.tmp");
                if std::fs::write(&temporary, bytes).is_ok() {
                    std::fs::rename(&temporary, &path).ok();
                }
            })
            .await;
        }));
    }
}

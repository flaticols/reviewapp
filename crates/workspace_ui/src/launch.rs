//! What the app was asked to do at launch, and the data it starts with.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use app_state_ui::{PullRequestStore, PullRequestStoreEvent, SettingsStore, StoresExt as _};
use github_api::{FixtureSource, GhSource, PullRequestSource};
use gpui_kit::{App, AppContext as _};
use pr_cache::PullRequestCache;
use pr_model::{CheckStatus, PullRequestKey, RepoSlug, SavedFilter, SavedFilterRule, Timestamp};
use settings_model::{
    Account, AccountSource, Appearance, CloneStatus, Preferences, SettingsFile, default_agents,
};

/// How long the sample snapshot-test check runs before it fails, as in design 04.
const SNAPSHOT_CHECK_RUNTIME: Duration = Duration::from_secs(25);

/// Parsed command-line arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchArgs {
    /// Show the built-in sample workspace instead of GitHub.
    pub sample: bool,
    /// Open this pull request once the window is up.
    pub open: Option<PullRequestKey>,
    /// Use this appearance for this run without saving it.
    pub appearance: Option<Appearance>,
}

/// Parses `[--sample] [--appearance light|dark|system] [owner/name#number]`.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> LaunchArgs {
    let mut launch = LaunchArgs::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sample" => launch.sample = true,
            "--appearance" => {
                launch.appearance = match args.next().as_deref() {
                    Some("light") => Some(Appearance::Light),
                    Some("dark") => Some(Appearance::Dark),
                    Some("system") => Some(Appearance::System),
                    _ => None,
                }
            }
            other => {
                if let Some(key) = parse_pull_request_arg(other) {
                    launch.open = Some(key);
                }
            }
        }
    }
    launch
}

/// Parses `owner/name#number`.
pub fn parse_pull_request_arg(arg: &str) -> Option<PullRequestKey> {
    let (repo, number) = arg.split_once('#')?;
    Some(PullRequestKey::new(
        RepoSlug::parse(repo)?,
        number.parse().ok()?,
    ))
}

/// The stores' starting state.
pub struct StartingData {
    pub source: Arc<dyn PullRequestSource>,
    pub saved_filters: Vec<SavedFilter>,
    pub settings: SettingsFile,
    pub settings_path: Option<PathBuf>,
    pub clone_statuses: Vec<(String, CloneStatus)>,
    /// Where the signed-in account's pull requests are cached, when someone is signed in.
    pub cache: Option<PullRequestCache>,
}

/// The sample workspace from the design, kept in memory only.
pub fn sample_data(home: &Path) -> StartingData {
    let sample = fixtures::sample(Timestamp::now(), home);
    let featured = PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482);
    let source = FixtureSource::new(sample.pull_requests, sample.details).with_check_progression(
        featured,
        fixtures::snapshot_check(CheckStatus::Failed, 62),
        SNAPSHOT_CHECK_RUNTIME,
    );
    StartingData {
        source: Arc::new(source),
        saved_filters: sample.saved_filters,
        settings: sample.settings,
        settings_path: None,
        clone_statuses: sample.clone_statuses,
        cache: None,
    }
}

/// GitHub through the `gh` CLI, with settings read from disk or seeded on first run
/// from the signed-in `gh` account.
pub fn live_data(home: &Path) -> StartingData {
    let settings_path = SettingsStore::default_path(home);
    SettingsStore::adopt_earlier_settings(home, &settings_path);
    let mut settings =
        SettingsStore::read(&settings_path).unwrap_or_else(|| first_run_settings(home));
    if settings.offer_new_agents(Some(home)) {
        SettingsStore::write(&settings_path, &settings).ok();
    }
    let host = settings
        .accounts
        .first()
        .map(|account| account.host.clone())
        .unwrap_or_else(|| "github.com".into());
    let cache = settings.accounts.first().map(|account| {
        PullRequestCache::new(
            &PullRequestCache::default_root(home),
            &account.host,
            &account.login,
        )
    });
    StartingData {
        source: Arc::new(GhSource::new(host)),
        saved_filters: vec![SavedFilter::new(
            "failing-ci",
            "Failing CI",
            SavedFilterRule::FailingCi,
        )],
        settings,
        settings_path: Some(settings_path),
        clone_statuses: Vec::new(),
        cache,
    }
}

fn first_run_settings(home: &Path) -> SettingsFile {
    let accounts = github_api::gh_session()
        .map(|session| {
            let account = Account::new(session.login, session.host, AccountSource::GhCli);
            match github_api::gh_user_name() {
                Some(name) => vec![account.with_name(name)],
                None => vec![account],
            }
        })
        .unwrap_or_default();
    SettingsFile::new(
        accounts,
        Vec::new(),
        default_agents(Some(home)),
        Preferences::default(),
    )
}

/// Maps repositories that have pull requests to clones found on disk, once the list
/// has loaded. Leaves existing mappings alone.
pub fn discover_clones_after_load(home: PathBuf, cx: &mut App) {
    let store = cx.stores().pull_requests().clone();
    cx.subscribe(&store, move |store, event: &PullRequestStoreEvent, cx| {
        if *event != PullRequestStoreEvent::ListChanged {
            return;
        }
        let repos: Vec<RepoSlug> = store
            .read(cx)
            .pull_requests()
            .iter()
            .map(|pr| pr.key.repo().clone())
            .collect();
        let home = home.clone();
        let found = cx.background_spawn(async move { clone_discovery::discover(&home) });
        cx.spawn(async move |cx| {
            let found = found.await;
            cx.update(|cx| {
                let settings = cx.stores().settings().clone();
                let mapped: Vec<String> = settings
                    .read(cx)
                    .repositories()
                    .iter()
                    .map(|mapping| mapping.slug.clone())
                    .collect();
                for (slug, path) in found {
                    if repos.contains(&slug) && !mapped.contains(&slug.to_string()) {
                        settings.update(cx, |settings, cx| settings.add_repository(&path, cx).ok());
                    }
                }
            });
        })
        .detach();
    })
    .detach();
}

/// Creates the stores from `data`.
pub fn install_stores(data: StartingData, home: Option<PathBuf>, cx: &mut App) {
    let triage_cache = data.cache.as_ref().map(|cache| cache.dir().to_path_buf());
    let pull_requests =
        cx.new(|cx| PullRequestStore::new(data.source, data.saved_filters, data.cache, cx));
    let settings = cx.new(|cx| {
        SettingsStore::new(
            data.settings,
            data.settings_path,
            home,
            data.clone_statuses,
            cx,
        )
    });
    app_state_ui::Stores::install(pull_requests, settings, cx);
    triage_ui::TriageStore::install(triage_cache, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> LaunchArgs {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_flags_and_a_pull_request() {
        let launch = args(&[
            "--sample",
            "--appearance",
            "dark",
            "flaticols/switchfold#482",
        ]);
        assert!(launch.sample);
        assert_eq!(launch.appearance, Some(Appearance::Dark));
        assert_eq!(launch.open.unwrap().number(), 482);
        assert_eq!(args(&[]), LaunchArgs::default());
        assert!(parse_pull_request_arg("switchfold#482").is_none());
    }
}

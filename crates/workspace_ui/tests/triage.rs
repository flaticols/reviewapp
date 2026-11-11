//! The triage store with a provider that answers from rules: what it asks, what it
//! keeps, and that it stops after a failure.

use std::sync::Arc;

use app_state_ui::StoresExt as _;
use gpui_kit::{Entity, TestAppContext};
use pr_model::{PullRequestKey, RepoSlug};
use triage::{Severity, TriageProvider};
use triage_ui::{TriageStore, TriageStoreExt as _};
use workspace_ui::launch;

fn featured() -> PullRequestKey {
    PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482)
}

fn start(provider: Arc<fixtures::SampleTriage>, cx: &mut TestAppContext) -> Entity<TriageStore> {
    let store = cx.update(|cx| {
        gpui_kit::init(cx);
        let home = std::path::PathBuf::from("/Users/test");
        launch::install_stores(launch::sample_data(&home), Some(home), cx);
        cx.triage().expect("installed with the stores")
    });
    cx.run_until_parked();
    store.update(cx, |store, cx| {
        store.set_provider(Some(provider as Arc<dyn TriageProvider>), cx)
    });
    cx.run_until_parked();
    store
}

#[gpui_kit::test]
fn rates_everything_once_and_again_only_when_text_changes(cx: &mut TestAppContext) {
    let provider = Arc::new(fixtures::SampleTriage::default());
    let store = start(provider.clone(), cx);
    let pull_requests = cx.update(|cx| cx.stores().pull_requests().read(cx).pull_requests().len());
    assert_eq!(
        provider.calls(),
        pull_requests,
        "every pull request is rated"
    );
    let rated = store.read_with(cx, |store, _| store.pull_request(&featured()));
    assert!(rated.is_some());

    cx.update(|cx| {
        let pulls = cx.stores().pull_requests().clone();
        pulls.update(cx, |pulls, cx| pulls.load_detail(&featured(), cx));
    });
    cx.run_until_parked();
    let (findings, threads) = cx.update(|cx| {
        let detail = cx
            .stores()
            .pull_requests()
            .read(cx)
            .loaded_detail(&featured())
            .cloned()
            .unwrap();
        (detail.findings, detail.threads)
    });
    assert_eq!(
        provider.calls(),
        pull_requests + findings.len() + threads.len(),
        "each finding and thread is asked about once"
    );
    store.read_with(cx, |store, _| {
        assert!(
            findings
                .iter()
                .all(|f| store.finding(&featured(), &f.id).is_some())
        );
        assert!(
            threads
                .iter()
                .all(|t| store.thread(&featured(), &t.id).is_some())
        );
    });

    let before = provider.calls();
    store.update(cx, |store, cx| {
        store.set_provider(Some(provider.clone() as Arc<dyn TriageProvider>), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        provider.calls(),
        before,
        "unchanged text isn't asked about again"
    );

    let finding = findings[0].id.clone();
    cx.update(|cx| {
        let pulls = cx.stores().pull_requests().clone();
        pulls.update(cx, |pulls, cx| {
            pulls.edit_finding(
                &featured(),
                &finding,
                "This panics on an empty diff.".into(),
                cx,
            )
        });
    });
    cx.run_until_parked();
    assert_eq!(
        provider.calls(),
        before + 1,
        "an edited finding is asked about again"
    );
    let severity = store.read_with(cx, |store, _| {
        store.finding(&featured(), &finding).map(|f| f.severity)
    });
    assert_eq!(severity, Some(Severity::Critical));
}

#[gpui_kit::test]
fn a_failure_pauses_instead_of_retrying_every_refresh(cx: &mut TestAppContext) {
    let provider = Arc::new(fixtures::SampleTriage::failing());
    let store = start(provider.clone(), cx);
    let after_first = provider.calls();
    assert!(
        (1..=4).contains(&after_first),
        "at most one batch was in flight"
    );
    let error = store.read_with(cx, |store, _| store.last_error().map(str::to_string));
    assert_eq!(error.as_deref(), Some("TypeSafe.ai refused the API key."));
    cx.update(|cx| {
        let pulls = cx.stores().pull_requests().clone();
        pulls.update(cx, |pulls, cx| pulls.refresh(cx));
    });
    cx.run_until_parked();
    assert_eq!(
        provider.calls(),
        after_first,
        "refreshing doesn't hammer a failing provider"
    );
}

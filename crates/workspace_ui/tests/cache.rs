//! The on-disk cache: what one run saves, the next shows before GitHub answers.

use std::sync::Arc;
use std::time::Duration;

use app_state_ui::{DetailState, ListState, PullRequestStore};
use gpui_kit::{AppContext as _, Entity, TestAppContext};
use pr_cache::PullRequestCache;
use pr_model::{PullRequestKey, RepoSlug, Timestamp};
use review_model::Actor;

fn featured() -> PullRequestKey {
    PullRequestKey::new(RepoSlug::new("flaticols", "switchfold"), 482)
}

fn store(
    cache: &PullRequestCache,
    online: bool,
    cx: &mut TestAppContext,
) -> Entity<PullRequestStore> {
    let sample = fixtures::sample(Timestamp::now(), std::path::Path::new("/Users/test"));
    let source: Arc<dyn github_api::PullRequestSource> = if online {
        Arc::new(github_api::FixtureSource::new(
            sample.pull_requests,
            sample.details,
        ))
    } else {
        Arc::new(github_api::FixtureSource::new(Vec::new(), Vec::new()))
    };
    let cache = cache.clone();
    cx.update(|cx| cx.new(|cx| PullRequestStore::new(source, Vec::new(), Some(cache), cx)))
}

#[gpui_kit::test]
fn a_pending_reply_survives_a_restart(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("switchfold-cache-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let cache = PullRequestCache::new(&root, "github.com", "flaticols");

    let first = store(&cache, true, cx);
    cx.run_until_parked();
    first.update(cx, |store, cx| store.load_detail(&featured(), cx));
    cx.run_until_parked();
    let thread = first.read_with(cx, |store, _| {
        store.loaded_detail(&featured()).unwrap().threads[0]
            .id
            .clone()
    });
    first.update(cx, |store, cx| {
        store.reply(
            &featured(),
            &thread,
            &Actor::person("flaticols"),
            "Kept across restarts".into(),
            cx,
        )
    });
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    let offline = store(&cache, false, cx);
    offline.read_with(cx, |store, _| {
        assert_eq!(store.list_state(), &ListState::Loaded);
        assert!(
            !store.pull_requests().is_empty(),
            "the cached list shows at once"
        );
    });
    offline.update(cx, |store, cx| store.load_detail(&featured(), cx));
    cx.run_until_parked();
    offline.read_with(cx, |store, _| {
        let Some(DetailState::Loaded(detail)) = store.detail(&featured()) else {
            panic!("the cached detail stands in when GitHub can't be reached");
        };
        assert!(
            detail
                .threads
                .iter()
                .flat_map(|t| &t.comments)
                .any(|c| c.is_pending && c.body == "Kept across restarts")
        );
    });

    let online = store(&cache, true, cx);
    cx.run_until_parked();
    online.update(cx, |store, cx| store.load_detail(&featured(), cx));
    cx.run_until_parked();
    online.read_with(cx, |store, _| {
        let detail = store.loaded_detail(&featured()).unwrap();
        assert!(
            detail
                .threads
                .iter()
                .flat_map(|t| &t.comments)
                .any(|c| c.is_pending && c.body == "Kept across restarts"),
            "GitHub's copy keeps the pending reply"
        );
    });
    std::fs::remove_dir_all(&root).ok();
}

#[gpui_kit::test]
fn a_saved_view_state_comes_back_in_the_next_run(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("switchfold-view-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    let cache = PullRequestCache::new(&root, "github.com", "flaticols");

    let first = store(&cache, true, cx);
    first.update(cx, |store, cx| {
        store.save_view_state(&featured(), "panes", &7u32, cx)
    });
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    let next = store(&cache, false, cx);
    let load = next.read_with(cx, |store, cx| {
        store.load_view_state::<u32>(&featured(), "panes", cx)
    });
    let loaded = std::rc::Rc::new(std::cell::Cell::new(None));
    cx.update(|cx| {
        let loaded = loaded.clone();
        cx.spawn(async move |_| loaded.set(load.await)).detach();
    });
    cx.run_until_parked();
    assert_eq!(loaded.get(), Some(7));
    std::fs::remove_dir_all(&root).ok();
}

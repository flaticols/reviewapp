use std::rc::Rc;

use acp_client::{AgentEvent, ReviewRequest, RunControl};
use async_channel::Receiver;
use gpui_kit::{App, AppContext as _, Global};

type Launch = Rc<dyn Fn(ReviewRequest, &mut App) -> (Receiver<AgentEvent>, Option<RunControl>)>;

/// How agent sessions start. Without one set, a session runs the agent's process over
/// ACP; a test sets one that hands back the events it wants the interface to show.
pub struct AgentLauncher(Launch);

impl Global for AgentLauncher {}

impl AgentLauncher {
    /// Starts sessions with `launch`, which returns the session's events and, when the
    /// session can be stopped, its control.
    pub fn new(
        launch: impl Fn(ReviewRequest, &mut App) -> (Receiver<AgentEvent>, Option<RunControl>) + 'static,
    ) -> Self {
        Self(Rc::new(launch))
    }
}

/// Starts the session `request` describes and returns its events and its control.
pub(crate) fn launch(
    request: ReviewRequest,
    cx: &mut App,
) -> (Receiver<AgentEvent>, Option<RunControl>) {
    if let Some(launch) = cx.try_global::<AgentLauncher>().map(|it| it.0.clone()) {
        return launch(request, cx);
    }
    let (future, events, control) = acp_client::start_review(request);
    cx.background_spawn(future).detach();
    (events, Some(control))
}

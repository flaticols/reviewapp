//! Runs a review against the scripted mock agent over real ACP stdio, with the real
//! findings MCP server, and checks every event a run produces.

use std::time::{Duration, Instant};

use acp_client::{
    AgentEvent, Decision, PlanStatus, PromptInput, ReviewRequest, RunOutcome, SessionChoices,
    SettingKind, ToolStatus, review_prompt,
};
use diff_model::{DiffLine, FileChange, FileDiff, Hunk, LineKind};

fn file(path: &str) -> FileDiff {
    FileDiff::new(
        path,
        FileChange::Modified,
        vec![Hunk::new(
            1,
            1,
            1,
            1,
            "",
            vec![
                DiffLine::new(LineKind::Removed, Some(1), None, "old"),
                DiffLine::new(LineKind::Added, None, Some(1), "new"),
            ],
        )],
    )
}

#[test]
fn review_run_streams_tools_permission_and_findings() {
    let agent = env!("CARGO_BIN_EXE_acp-mock-agent");
    let server = env!("CARGO_BIN_EXE_acp-findings-server");
    let files = [
        file("crates/review/src/hunk.rs"),
        file("crates/review/src/render.rs"),
    ];
    let prompt = review_prompt(&PromptInput::new(
        "flaticols/switchfold",
        482,
        "Render review thread",
        files.iter().collect(),
    ));
    let request = ReviewRequest::new(format!("'{agent}'"), std::env::temp_dir(), prompt)
        .with_findings_server(server.into())
        .with_choices(
            SessionChoices::new(Some("deep".into()), Some("high".into()))
                .asking_before_commands(true),
        );
    let (future, events, _control) = acp_client::start_review(request);
    std::thread::spawn(move || futures::executor::block_on(future));

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut connected = None;
    let mut completed_reads = Vec::new();
    let mut permission_command = None;
    let mut findings = Vec::new();
    let mut message = String::new();
    let mut plans = Vec::new();
    let outcome = loop {
        assert!(Instant::now() < deadline, "the run timed out");
        let event = events
            .recv_blocking()
            .expect("the run reports until it ends");
        match event {
            AgentEvent::Connected { name, .. } => connected = name,
            AgentEvent::Tool(activity) if activity.status == Some(ToolStatus::Completed) => {
                completed_reads.extend(activity.paths)
            }
            AgentEvent::Tool(_) => {}
            AgentEvent::Permission(prompt) => {
                permission_command = prompt.command().map(str::to_string);
                prompt.respond(Decision::AllowOnce);
            }
            AgentEvent::Finding(finding) => findings.push(finding),
            AgentEvent::Message(chunk) => message.push_str(&chunk),
            AgentEvent::Thought(_) => {}
            AgentEvent::Plan(steps) => plans.push(steps),
            AgentEvent::Finished(outcome) => break Ok(outcome),
            AgentEvent::Failed(error) => break Err(error),
        }
    };

    assert_eq!(outcome, Ok(RunOutcome::Completed));
    assert_eq!(connected.as_deref(), Some("acp-mock-agent"));
    assert_eq!(completed_reads.len(), 2);
    assert_eq!(
        permission_command.as_deref(),
        Some("cargo test -p review --lib thread::")
    );
    assert_eq!(plans.len(), 3);
    assert_eq!(plans[0][0].status, PlanStatus::InProgress);
    assert!(
        plans[2]
            .iter()
            .all(|step| step.status == PlanStatus::Completed)
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].path, "crates/review/src/hunk.rs");
    assert_eq!(findings[0].line, 31);
    assert_eq!(
        message,
        "Reviewed 2 files; tests ran. mode=default model=deep effort=high"
    );
}

#[test]
fn probing_reports_the_efforts_of_the_chosen_model() {
    let agent = format!("'{}'", env!("CARGO_BIN_EXE_acp-mock-agent"));
    let probe = |model: Option<&str>| {
        futures::executor::block_on(acp_client::probe_settings(
            &agent,
            &[],
            std::env::temp_dir(),
            SessionChoices::new(model.map(str::to_string), None),
        ))
        .expect("the mock agent answers")
    };
    let default = probe(None);
    let kinds: Vec<SettingKind> = default.iter().map(|setting| setting.kind).collect();
    assert_eq!(kinds, [SettingKind::Mode, SettingKind::Model]);

    let deep = probe(Some("deep"));
    let effort = deep
        .iter()
        .find(|setting| setting.kind == SettingKind::Effort)
        .expect("the deep model has an effort");
    let values: Vec<&str> = effort.choices.iter().map(|c| c.value.as_str()).collect();
    assert_eq!(values, ["low", "high"]);
    assert_eq!(effort.current, "low");
}

#[test]
fn a_rejected_model_fails_with_the_api_message() {
    let agent = format!("'{}'", env!("CARGO_BIN_EXE_acp-mock-agent"));
    let request = ReviewRequest::new(agent, std::env::temp_dir(), "Review this.")
        .with_choices(SessionChoices::new(Some("unsupported".into()), None));
    let (future, events, _control) = acp_client::start_review(request);
    std::thread::spawn(move || futures::executor::block_on(future));
    let error = loop {
        match events
            .recv_blocking()
            .expect("the run reports until it ends")
        {
            AgentEvent::Failed(error) => break error,
            AgentEvent::Finished(outcome) => panic!("expected a failure, got {outcome:?}"),
            _ => {}
        }
    };
    assert_eq!(
        error,
        "The 'unsupported' model is not supported when using this mock agent with a ChatGPT account."
    );
}

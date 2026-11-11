//! Prints the session settings an ACP agent offers, such as its models and reasoning
//! effort: `acp-probe [model=… effort=…] [prompt=…] -- <command line>`. A model or
//! effort is applied first, to see the choices that depend on it. With `prompt=…` it
//! sends that prompt instead and prints everything the agent reports.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|arg| arg == "--");
    let (chosen, command) = match split {
        Some(ix) => (&args[..ix], &args[ix + 1..]),
        None => (&args[..0], &args[..]),
    };
    let pick = |name: &str| {
        chosen
            .iter()
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
    };
    let choices = acp_client::SessionChoices::new(pick("model"), pick("effort"));
    let command_line = command.join(" ");
    let cwd = std::env::current_dir().unwrap_or_default();
    let env = vec![("PATH".to_string(), tool_path::path_env())];
    if let Some(prompt) = pick("prompt") {
        let mut request =
            acp_client::ReviewRequest::new(command_line, cwd, prompt).with_choices(choices);
        // Serve Switchfold's MCP tools from the sibling binary, as the app does.
        if let Some(server) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("acp-findings-server")))
            .filter(|server| server.exists())
        {
            request = request.with_findings_server(server);
        }
        for (name, value) in env {
            request = request.with_env(name, value);
        }
        let (future, events, _control) = acp_client::start_review(request);
        std::thread::spawn(move || smol::block_on(future));
        while let Ok(event) = events.recv_blocking() {
            let done = matches!(
                event,
                acp_client::AgentEvent::Finished(_) | acp_client::AgentEvent::Failed(_)
            );
            match event {
                acp_client::AgentEvent::Permission(prompt) => {
                    println!("permission: {} (denied)", prompt.title());
                    prompt.respond(acp_client::Decision::Deny);
                }
                other => println!("{other:?}"),
            }
            if done {
                break;
            }
        }
        return;
    }
    match smol::block_on(acp_client::probe_settings(
        &command_line,
        &env,
        cwd,
        choices,
    )) {
        Ok(settings) if settings.is_empty() => println!("no settings"),
        Ok(settings) => {
            for setting in settings {
                println!(
                    "{} ({:?}) id={} current={}",
                    setting.name, setting.kind, setting.id, setting.current
                );
                for choice in setting.choices {
                    println!("  {} = {}", choice.value, choice.name);
                }
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

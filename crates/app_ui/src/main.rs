//! Switchfold: review GitHub pull requests from local clones, with agents over ACP.
//!
//! `switchfold [--sample] [--appearance light|dark|system] [owner/name#number]`
//!
//! Also, without opening a window:
//! - `switchfold --zed-mcp`: the MCP server Zed's agent starts.
//! - `switchfold comment --file <path> --line <n>`: asks the running app to comment on
//!   that line, for a Zed task.

use app_state_ui::StoresExt as _;
use gpui_kit::AppContext as _;
use settings_model::Appearance;

use workspace_ui::{Workspace, launch};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--findings-mcp") {
        // Launched by an ACP agent as the stdio MCP server behind `report_finding`.
        let socket = args.drain(..).nth(1).unwrap_or_default();
        if let Err(error) = acp_client::findings::serve_stdio(std::path::Path::new(&socket)) {
            eprintln!("switchfold findings server: {error}");
            std::process::exit(1);
        }
        return;
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    if args.first().map(String::as_str) == Some("--zed-mcp") {
        let socket = zed_bridge::mcp::socket_path(home.as_deref().unwrap_or("/".as_ref()));
        if let Err(error) = zed_bridge::mcp::serve_stdio(&socket) {
            eprintln!("switchfold zed server: {error}");
            std::process::exit(1);
        }
        return;
    }
    if args.first().map(String::as_str) == Some("comment") {
        std::process::exit(comment_from_cli(&args[1..]));
    }
    let launch = launch::parse_args(args);
    let (links, incoming_links) = async_channel::unbounded::<String>();
    let app = gpui_kit::application().with_assets(widgets_ui::AppAssets);
    app.on_open_urls(move |urls| {
        for url in urls {
            links.send_blocking(url).ok();
        }
    });
    app.run(move |cx| {
        gpui_kit::init(cx);
        pr_list_ui::init(cx);
        review_thread_ui::init(cx);
        workspace_ui::init(cx);
        workspace_ui::install_menus(cx);

        let home_dir = home.clone().unwrap_or_else(|| "/".into());
        let data = if launch.sample {
            launch::sample_data(&home_dir)
        } else {
            launch::live_data(&home_dir)
        };
        let preferences = data.settings.preferences.clone();
        launch::install_stores(data, home.clone(), cx);
        if !launch.sample {
            launch::discover_clones_after_load(home_dir.clone(), cx);
            typesafe_ui::init(cx);
        }

        let mut look = preferences.clone();
        look.appearance = launch.appearance.unwrap_or(preferences.appearance);
        theme_ui::apply(&look, None, cx);

        let options = chrome_ui::window_options(cx);
        let (window, workspace) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Workspace::new(window, cx))
        })
        .expect("failed to open the main window");
        if !launch.sample {
            let socket = zed_bridge::mcp::socket_path(&home_dir);
            window
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        workspace.listen_for_zed(&socket, window, cx)
                    })
                })
                .ok();
        }
        let link_workspace = workspace.clone();
        cx.spawn(async move |cx| {
            while let Ok(url) = incoming_links.recv().await {
                window
                    .update(cx, |_, window, cx| {
                        link_workspace
                            .update(cx, |workspace, cx| workspace.open_link(&url, window, cx))
                    })
                    .ok();
            }
        })
        .detach();
        if let Some(key) = launch.open.clone() {
            window
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        workspace.open_pull_request(key, window, cx)
                    })
                })
                .ok();
        }
        let pinned_appearance = launch.appearance.is_some();
        window
            .update(cx, |_, window, _| {
                window
                    .observe_window_appearance(move |window, cx| {
                        let preferences = cx.stores().settings().read(cx).preferences().clone();
                        if preferences.appearance == Appearance::System && !pinned_appearance {
                            theme_ui::apply(&preferences, Some(window), cx);
                        }
                    })
                    .detach();
            })
            .ok();
        cx.activate(true);
    });
}

/// `comment --file <path> --line <n>`: hands the line to the running app (starting it if
/// needed) through its URL scheme. Returns the exit code.
fn comment_from_cli(args: &[String]) -> i32 {
    let value = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|ix| args.get(ix + 1))
    };
    let (Some(file), Some(line)) = (value("--file"), value("--line")) else {
        eprintln!("usage: switchfold comment --file <path> --line <n>");
        return 2;
    };
    let Ok(line) = line.trim().parse::<u32>() else {
        eprintln!("switchfold comment: --line takes a line number, not {line}");
        return 2;
    };
    let file = std::path::Path::new(file);
    let file = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let link = zed_bridge::SwitchfoldLink::Comment { file, line };
    match zed_bridge::open_url(&link.to_url()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("switchfold comment: {error}");
            1
        }
    }
}

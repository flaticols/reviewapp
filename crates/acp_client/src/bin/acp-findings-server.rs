//! The findings MCP server on its own, for agents and tests that run without the app.
//!
//! `acp-findings-server [--findings-mcp] <socket>`

fn main() {
    let socket = std::env::args()
        .skip(1)
        .find(|arg| arg != "--findings-mcp")
        .unwrap_or_default();
    if let Err(error) = acp_client::findings::serve_stdio(std::path::Path::new(&socket)) {
        eprintln!("acp-findings-server: {error}");
        std::process::exit(1);
    }
}

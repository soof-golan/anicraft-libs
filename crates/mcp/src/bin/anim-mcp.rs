//! `anim-mcp`: an MCP server on stdio that forwards to a running `AniCraft` or `CharCraft` over its control channel.
//!
//! Start the app with its control server (it prints the address and writes a token file), then point an MCP client at:
//!
//! ```text
//! anim-mcp --token-file /path/to/control.token
//! ```
//!
//! Nothing but protocol messages goes to stdout; diagnostics go to stderr.

use std::net::SocketAddr;
use std::path::PathBuf;

use anim_mcp::{AnimMcp, connect_bridge, connect_bridge_from_file, run_stdio};
use clap::Parser;
use color_eyre::eyre::{Result, WrapErr as _, eyre};

/// MCP (stdio) bridge to a running `AniCraft` or `CharCraft`.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Token file written by the app's control server (first line the token, second line host:port).
    #[arg(long, value_name = "FILE", conflicts_with_all = ["addr", "token"])]
    token_file: Option<PathBuf>,

    /// Address of the control server, as printed by the app.
    #[arg(long, value_name = "HOST:PORT", requires = "token")]
    addr: Option<SocketAddr>,

    /// Control token (prefer --token-file or the `ANIM_MCP_TOKEN` variable: command lines are visible to other users).
    #[arg(long, env = "ANIM_MCP_TOKEN", hide_env_values = true)]
    token: Option<String>,

    /// Server name announced to the MCP client.
    #[arg(long, default_value = "anim-mcp")]
    name: String,

    /// Serve streamable HTTP on 127.0.0.1:PORT (0 picks a free port) instead of stdio. Clients send `Authorization: Bearer <token>`.
    #[cfg(feature = "http")]
    #[arg(long, value_name = "PORT", requires = "http_token_file")]
    http: Option<u16>,

    /// Where `--http` writes its bearer token and address (the same format as the app's token file).
    #[cfg(feature = "http")]
    #[arg(long, value_name = "FILE")]
    http_token_file: Option<PathBuf>,
}

#[cfg(feature = "http")]
fn serve_http_blocking(server: AnimMcp, port: u16, token_file: &std::path::Path) -> Result<()> {
    use anim_mcp::http::{HttpListener, serve_http};
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().wrap_err("could not start the async runtime")?;
    runtime.block_on(async {
        let listener = HttpListener::bind(port).await?;
        let token = anim_mcp::generate_token()?;
        anim_mcp::write_token_file(token_file, &token, listener.addr()).wrap_err_with(|| format!("could not write {}", token_file.display()))?;
        eprintln!("anim-mcp: MCP over HTTP at {} (bearer token in {})", listener.url(), token_file.display());
        serve_http(server, listener, token, std::future::pending()).await?;
        Ok(())
    })
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let backend = match (&args.token_file, args.addr, &args.token) {
        (Some(file), _, _) => connect_bridge_from_file(file).wrap_err_with(|| format!("could not connect using {}", file.display()))?,
        (None, Some(addr), Some(token)) => connect_bridge(addr, token).wrap_err_with(|| format!("could not connect to {addr}"))?,
        _ => return Err(eyre!("give --token-file, or --addr together with --token (or ANIM_MCP_TOKEN)")),
    };
    eprintln!("anim-mcp: connected to {}", backend.description());
    let server = AnimMcp::new(backend, args.name, env!("CARGO_PKG_VERSION"));
    #[cfg(feature = "http")]
    if let (Some(port), Some(token_file)) = (args.http, &args.http_token_file) {
        return serve_http_blocking(server, port, token_file);
    }
    eprintln!("anim-mcp: serving MCP on stdio");
    run_stdio(server)?;
    Ok(())
}

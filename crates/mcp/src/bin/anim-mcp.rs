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
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let backend = match (&args.token_file, args.addr, &args.token) {
        (Some(file), _, _) => connect_bridge_from_file(file).wrap_err_with(|| format!("could not connect using {}", file.display()))?,
        (None, Some(addr), Some(token)) => connect_bridge(addr, token).wrap_err_with(|| format!("could not connect to {addr}"))?,
        _ => return Err(eyre!("give --token-file, or --addr together with --token (or ANIM_MCP_TOKEN)")),
    };
    eprintln!("anim-mcp: connected to {}; serving MCP on stdio", backend.description());
    let server = AnimMcp::new(backend, args.name, env!("CARGO_PKG_VERSION"));
    run_stdio(server)?;
    Ok(())
}

//! Running the MCP front-end on a transport.

use rmcp::ServiceExt as _;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::front::AnimMcp;

/// Why serving stopped abnormally.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ServeError(String);

/// Serves MCP over any async byte stream pair (an in-memory duplex in tests, a socket, stdio) until the peer
/// disconnects.
///
/// # Errors
/// [`ServeError`] when the handshake or the transport fails.
pub async fn serve_io<T>(mcp: AnimMcp, transport: T) -> Result<(), ServeError>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let service = mcp.serve(transport).await.map_err(|e| ServeError(e.to_string()))?;
    service.waiting().await.map(|_| ()).map_err(|e| ServeError(e.to_string()))
}

/// Serves MCP over the process's stdin and stdout until the client disconnects. Nothing else may print to stdout.
///
/// # Errors
/// [`ServeError`] when the handshake or the transport fails.
pub async fn serve_stdio(mcp: AnimMcp) -> Result<(), ServeError> {
    let service = mcp.serve(rmcp::transport::stdio()).await.map_err(|e| ServeError(e.to_string()))?;
    service.waiting().await.map(|_| ()).map_err(|e| ServeError(e.to_string()))
}

/// [`serve_stdio`] on a private current-thread runtime, for `main` functions that are not async.
///
/// # Errors
/// [`ServeError`] when the runtime cannot start, or as [`serve_stdio`].
pub fn run_stdio(mcp: AnimMcp) -> Result<(), ServeError> {
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| ServeError(format!("could not start the async runtime: {e}")))?;
    runtime.block_on(serve_stdio(mcp))
}

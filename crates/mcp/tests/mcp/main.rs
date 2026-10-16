//! The integration tests of `anim-mcp`, built as one test binary (one link instead of five). Each module is a theme;
//! shared helpers live in `common`.

mod common;

mod bridge;
mod hostile;
mod http;
mod mcp_protocol;
mod stdio_process;

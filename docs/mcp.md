# MCP in AniCraft and CharCraft

Both apps are controllable by any [Model Context Protocol](https://modelcontextprotocol.io) client (Claude, editors,
scripts) through the shared crate `anim-mcp`. Everything a user can do is an `anim-session` command, so an agent can do
all of it: the tools are generated from the app's `CommandRegistry`.

## Architecture

```text
 MCP client            AnimMcp (rmcp ServerHandler)                       the editor
 ──────────            ───────────────────────────                        ──────────
 stdio JSON-RPC ─────► holds only a Backend (a channel Sender)
 in-memory duplex                │  Request + Ticket
                 ┌───────────────┴────────────────┐
          Headless backend                 Bridge backend ── TCP, JSON lines, token ──► ControlServer
          actor thread owning              (anim-mcp binary,                            embedded in the app
          a SessionHost                     or in your process)                         ControlInbox ► SessionHost
                                                                                        pumped by the UI thread
```

Rules that shape it:

* **Actors, no locks.** The document lives on exactly one thread inside a `SessionHost`. The MCP handler holds a
  `Backend` (a `crossbeam` sender) and awaits events from a `PendingCall`; no `Arc`, `Mutex` or shared state of ours.
* **One vocabulary.** `protocol::Request` is the whole control API. The MCP front-end, the headless actor, the TCP
  control channel and tests all speak it, so a feature added to the host shows up everywhere.
* **Generic.** `anim-mcp` knows commands, undo, files, jobs. Application specifics (outline, library, properties,
  queries, render, screenshot, export, load, save) plug in through the `AppHooks<D>` trait; every method has an
  `unsupported` default.
* **A call is a ticket.** The owner answers through a `Ticket`: `progress()` any number of times, then `finish()`, now
  or on a later frame (a screenshot waits for the next frame). `is_cancelled()` turns true when the client cancels, the
  MCP request is cancelled, or the connection drops.

## Using it from an app

```rust
// Build once.
let registry = build_registry();                         // CommandRegistry<Document>
let session = Session::new(Document::default(), registry);
let mut host = SessionHost::new(HostInfo { name: "anicraft".into(), version: VERSION.into() }, session, AniHooks::new())
    .with_confinement(Confinement::new(&read_roots, &write_roots)?);   // default: nothing readable or writable

// 1. A control channel for `anim-mcp --token-file` and other tools:
let server = ControlServer::start(&ControlConfig { port: 0, token_file: Some(path) }, {
    let ctx = ctx.clone();
    move || ctx.request_repaint()
})?;
eprintln!("control server on {} (token file {})", server.addr(), path.display());

// 2. Or MCP on the app's own stdin/stdout (`anicraft --mcp-stdio`):
let (inbox, backend) = ControlInbox::in_process({ let ctx = ctx.clone(); move || ctx.request_repaint() })?;
std::thread::spawn(move || run_stdio(AnimMcp::new(backend, "anicraft", VERSION)));

// Every frame, on the UI thread:
server.inbox().pump(&mut host);         // answers requests, calls hooks.tick()
```

A **headless** CLI (`anicraft-cli mcp`) needs no window:

```rust
let session = spawn_headless(|| SessionHost::new(info, Session::new(doc, registry), hooks))?;
run_stdio(AnimMcp::new(session.backend().clone(), "anicraft-cli", VERSION))?;
```

### Hooks

| Hook | Used by | Notes |
|---|---|---|
| `status` | `session_status` | merged under `app` |
| `queries` / `query` | `query`, `query_<name>` tools | read-only, JSON Schema per query |
| `outline`, `library` | `inspect_document`, `get_library`, resources | paths in the outline work with `get_property` |
| `get_property` / `set_property` | `get_property`, `set_property`, `anim://property/{path}` | `props::{parse_path, get, set}` help over JSON-shaped data; a set runs on a copy and is one undo step |
| `render` | `render_frame` | return `Reply::with_png_rgba`; the host writes `path` if given |
| `screenshot` | `screenshot` | clone the ticket, ask egui for a `ViewportCommand::Screenshot`, finish the ticket with the PNG (`Deferred`) |
| `export_formats` / `export` | `export`, `list_export_formats` | use `run_job` to run on a worker and stay responsive; honour `ticket.is_cancelled()` |
| `load`, `new_document`, `save` | `open_document`, `new_document`, `save_document` | saves are atomic (`craft-atomic`) |
| `path_access`, `refused_while_confined` | confinement of command parameters | default: keys like `path`, `file`, `*_path` are file names |
| `tick` | every pump | finish deferred work |

## The MCP surface

**Tools.** Built-ins: `session_status`, `list_commands`, `run_command`, `run_batch`, `inspect_document`, `get_library`,
`get_property`, `set_property`, `list_queries`, `query`, `render_frame`, `screenshot`, `list_export_formats`, `export`,
`undo`, `redo`, `open_document`, `new_document`, `save_document`, `journal`. Then **one typed tool per registered
command** (`timeline.insert_keyframe` becomes `timeline_insert_keyframe`) with the command's JSON Schema, description,
read-only hint and `_meta` (`anim/command`, `anim/enabled`, `anim/menu`, `anim/shortcut`), and one `query_<name>` per
query. Commands hidden from MCP are absent.

**Results.** `content` carries pretty JSON text plus image content (PNG, base64) for renders and screenshots;
`structuredContent` carries the object. A failure is an `isError` result whose `structuredContent.error` is
`{kind, message, data?}` with `kind` one of `not_found`, `disabled`, `invalid_params`, `failed`, `denied`, `cancelled`,
`unsupported`, `unavailable`, `transport`, `internal`. Only unknown tools and malformed arguments are JSON-RPC errors.

**Atomic batches.** `run_batch` runs its steps on a scratch copy and records one undo step labelled `label` only if every
step succeeds; a failure reports the step (`error.data = {step, command, applied: false}`) and changes nothing.

**Resources.** `anim://session/status`, `anim://document/outline`, `anim://document/library`, `anim://commands`,
`anim://session/journal`, and the template `anim://property/{path}`.

**Prompts.** `getting_started`, `make_change{goal}`; add your own with `AnimMcp::with_prompts`.

**Progress and cancellation.** Send `_meta.progressToken` and long calls (batches, exports) emit
`notifications/progress`; `notifications/cancelled` cancels the call in the session (the ticket reads cancelled).

**Journal.** Every request the session owner handles is recorded (sequence, time, origin, request, outcome, error,
duration): `journal` tool, `anim://session/journal`, optional JSON-lines file via `Journal::with_file`.

## Control channel (running apps)

Loopback TCP, one JSON object per line, like EffectCraft's control channel plus a token:

1. connect to `127.0.0.1:<port>`; send `{"method":"hello","params":{"token":"<token>","version":1}}`;
2. then `{"id":7,"method":"command.run","params":{"id":"layer.add","params":{"name":"x"}}}` and read lines with that `id`:
   `{"id":7,"progress":{"fraction":0.5,"message":"..."}}`, then `{"id":7,"ok":true,"result":...,"images":[{"mime","data"}]}`
   or `{"id":7,"ok":false,"error":{...}}`; `{"method":"cancel","params":{"id":7}}` cancels.

Methods: `status`, `commands.list`, `command.run`, `batch.run`, `queries.list`, `query.run`, `document.outline`,
`document.library`, `property.get`, `property.set`, `render.frame`, `screenshot`, `export.formats`, `export.run`,
`history.undo`, `history.redo`, `document.open`, `document.new`, `document.save`, `journal.tail`.

Security: loopback only, random 256-bit token (constant-time compare) written to a file with mode 0600, at most 16
connections, request lines capped at 8 MiB, garbage closes the connection, requests are cancelled when their connection
drops. Files: paths must be absolute, free of `..`, symlinks are resolved, reads need a read or write root, writes a
write root (including command parameters named `path`, `file`, `folder`, `*_path`, checked per step in batches).

## Connecting a client

```json
{ "mcpServers": { "anicraft": { "command": "anim-mcp", "args": ["--token-file", "/home/me/.cache/anicraft/control.token"] } } }
```

For clients that connect over HTTP (feature `http`, on by default): `anim-mcp --token-file app.token --http 0
--http-token-file mcp.token` serves streamable HTTP at `http://127.0.0.1:<port>/mcp` and writes the bearer token and
address to `mcp.token`. Only loopback `Host` headers are accepted and every request needs
`Authorization: Bearer <token>`. From code: `http::{HttpListener, serve_http}`.

Or, for an app that serves MCP itself, `{ "command": "anicraft", "args": ["--mcp-stdio"] }`. Never print to stdout in
stdio mode; log to stderr.

## Testing

`crates/mcp/tests` drive the server with raw JSON-RPC over an in-memory duplex against a toy document: handshake,
tools/list, tools/call, atomic batches, properties, render/screenshot images, confinement, progress, cancellation,
background jobs, resources, prompts, journal; plus the bridge over a real loopback socket, the `anim-mcp` binary over real
pipes, and proptest-based hostile input.

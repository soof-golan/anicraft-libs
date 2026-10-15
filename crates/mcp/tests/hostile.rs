//! Hostile and random input: nothing may panic, and failed edits must leave the document exactly as it was.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::missing_const_for_fn,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::needless_pass_by_value,
    clippy::disallowed_types,
    reason = "test code; proptest strategies are built on Arc"
)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anim_mcp::{BatchStep, Confinement, HookOutcome, Request, Ticket, props, wire};
use common::{App, Toy};
use proptest::prelude::*;
use serde_json::{Value, json};

fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<f64>().prop_map(|f| json!(f)),
        "[a-z./~\\[\\]\"0-9 _-]{0,12}".prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 48, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::hash_map("[a-z_.]{1,8}", inner, 0..4).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

fn ticket() -> (Ticket, tokio::sync::mpsc::UnboundedReceiver<(u64, anim_mcp::Event)>, crossbeam_channel::Sender<()>) {
    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();
    let (cancel_tx, cancel_rx) = crossbeam_channel::bounded(1);
    (Ticket::new(0, events_tx, cancel_rx), events_rx, cancel_tx)
}

const METHODS: &[&str] = &[
    "status",
    "commands.list",
    "command.run",
    "batch.run",
    "queries.list",
    "query.run",
    "document.outline",
    "document.library",
    "property.get",
    "property.set",
    "render.frame",
    "screenshot",
    "export.formats",
    "export.run",
    "history.undo",
    "history.redo",
    "document.open",
    "document.new",
    "document.save",
    "journal.tail",
    "cancel",
    "hello",
    "",
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn parsing_requests_never_panics(method in prop_oneof![prop::sample::select(METHODS).prop_map(str::to_owned), "[a-z.]{0,16}"], params in arb_json()) {
        let _ = Request::from_wire(&method, Some(&params));
    }

    #[test]
    fn property_paths_never_panic(path in "\\PC{0,80}", mut doc in arb_json(), value in arb_json()) {
        if let Ok(segments) = props::parse_path(&path) {
            let _ = props::get(&doc, &segments);
            let _ = props::set(&mut doc, &segments, value);
        }
    }

    #[test]
    fn confinement_never_panics_and_never_escapes(path in "\\PC{0,80}") {
        let dir = tempfile::tempdir().unwrap();
        let confinement = Confinement::new(&[dir.path().to_path_buf()], &[dir.path().to_path_buf()]).unwrap();
        if let Ok(real) = confinement.check(&path, anim_mcp::Access::Write) {
            let root = dir.path().canonicalize().unwrap();
            prop_assert!(real.starts_with(&root), "{real:?} escaped {root:?}");
        }
    }

    #[test]
    fn line_reader_respects_its_bound(bytes in prop::collection::vec(any::<u8>(), 0..300), max in 1_u64..64) {
        let mut cursor = std::io::Cursor::new(bytes);
        loop {
            match wire::read_line(&mut cursor, max).unwrap() {
                wire::Line::Eof | wire::Line::TooLong => break,
                wire::Line::Text(text) => prop_assert!(u64::try_from(text.len()).unwrap() <= max),
                wire::Line::NotUtf8 => {}
            }
        }
    }

    #[test]
    fn reading_event_lines_never_panics(line in arb_json()) {
        let _ = wire::parse_event_line(&line);
    }

    /// Random batches of good and bad steps: the host never panics, and whenever a batch fails the document is untouched.
    #[test]
    fn failed_batches_leave_the_document_untouched(
        steps in prop::collection::vec(
            (prop::sample::select(&["layer.add", "layer.move", "layer.fail", "layer.clear", "doc.stats", "nope", "edit.undo"][..]), arb_json()),
            0..8,
        )
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut host = common::host(Confinement::new(&[dir.path().to_path_buf()], &[dir.path().to_path_buf()]).unwrap(), false);
        let (ticket, _events, _cancel) = ticket();
        host.session_mut().execute("layer.add", &json!({ "name": "seed" })).unwrap();
        let before: Toy = host.session().snapshot();
        let undo_before = host.session().history().undo_len();
        let request = Request::Batch { steps: steps.into_iter().map(|(id, params)| BatchStep { id: id.to_owned(), params }).collect(), label: None };
        match host.dispatch(request, &ticket) {
            Ok(HookOutcome::Ready(_) | HookOutcome::Deferred) => {
                prop_assert!(host.session().history().undo_len() <= undo_before + 1, "a batch is at most one undo step");
            }
            Err(_) => {
                prop_assert_eq!(&host.session().snapshot(), &before);
                prop_assert_eq!(host.session().history().undo_len(), undo_before);
            }
        }
    }

    #[test]
    fn arbitrary_requests_never_panic_the_host(method in prop::sample::select(METHODS), params in arb_json()) {
        let dir = tempfile::tempdir().unwrap();
        let mut host = common::host(Confinement::new(&[dir.path().to_path_buf()], &[dir.path().to_path_buf()]).unwrap(), false);
        let (ticket, _events, _cancel) = ticket();
        if let Ok(request) = Request::from_wire(method, Some(&params)) {
            let _ = host.dispatch(request, &ticket);
        }
    }
}

#[test]
fn garbage_on_the_socket_does_not_hurt_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    let garbage: Vec<Vec<u8>> = vec![
        b"\xff\xfe\xfd\n".to_vec(),
        vec![b'{'; 100_000],
        b"{\"method\":\"hello\",\"params\":{\"token\":5}}\n".to_vec(),
        b"\x00\x00\x00\n".to_vec(),
        format!("{}\n", "[".repeat(10_000)).into_bytes(),
    ];
    for bytes in garbage {
        if let Ok(mut raw) = TcpStream::connect(app.server_addr) {
            let _ = raw.write_all(&bytes);
            let mut line = String::new();
            let _ = BufReader::new(&raw).read_line(&mut line);
        }
    }
    // A well-behaved client still gets served.
    let backend = app.connect();
    assert!(backend.call(Request::Status {}).is_ok());
}

#[test]
fn an_oversized_request_line_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(dir.path(), false, 0, None);
    let mut raw = TcpStream::connect(app.server_addr).unwrap();
    let mut reader = BufReader::new(raw.try_clone().unwrap());
    raw.write_all(format!("{}\n", json!({ "method": "hello", "params": { "token": app.token } })).as_bytes()).unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let huge = vec![b'a'; usize::try_from(anim_mcp::protocol::MAX_REQUEST_BYTES).unwrap() + 10];
    let _ = raw.write_all(&huge);
    let _ = raw.write_all(b"\n");
    line.clear();
    let _ = reader.read_line(&mut line);
    assert!(line.is_empty() || line.contains("too long"), "{line}");
    assert!(app.connect().call(Request::Status {}).is_ok(), "the server survives");
}

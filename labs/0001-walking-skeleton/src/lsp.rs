//! v0.3 #5 LSP server. Speaks Language Server Protocol over stdio
//! so VS Code (and any other LSP-aware editor) can run `tr lsp` as
//! a subprocess and get diagnostics / hover / goto-def for `.ts`
//! sources, sharing the exact lex+parse+check pipeline that
//! `tr build` / `tr run` use.
//!
//! Implementation follows the rust-analyzer / dprint pattern: hand
//! the JSON-RPC framing to `lsp-server`, decode requests via
//! `lsp-types`, dispatch by request method.
//!
//! Phase order (per RFC 20260505-lsp-server-skeleton.md):
//!   L-1  initialize / shutdown handshake (this file's MVP)
//!   L-2  document state + check.rs errors → diagnostics
//!   L-3  hover (type lookup)
//!   L-4  goto-def
//!   L-5  VS Code extension scaffold + .vsix package
//!   L-6  latency tuning to < 50 ms on 1 K-line file

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
    InitializeParams, InitializeResult, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind,
};

const SERVER_NAME: &str = "tr";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    // Stdio transport — the editor spawns `tr lsp` and pipes the
    // protocol through stdin/stdout. Diagnostics + log lines go
    // to stderr (LSP spec allows that channel to carry server
    // logs).
    let (connection, io_threads) = Connection::stdio();

    // L-1 capability set: full-document sync (we'll re-typecheck
    // the whole file on every edit, simpler than incremental and
    // < 50 ms for the 1 K-line target). hover / goto-def
    // capabilities flip on as L-3 / L-4 land.
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        ..Default::default()
    };

    let server_info = ServerInfo {
        name: SERVER_NAME.into(),
        version: Some(SERVER_VERSION.into()),
    };

    // initialize handshake. lsp-server's helper waits for the
    // client's `initialize` request, then returns its raw params
    // for our reply assembly.
    let (initialize_id, _initialize_params) = connection.initialize_start()?;
    let _params: InitializeParams =
        serde_json::from_value(_initialize_params).unwrap_or_default();

    let initialize_result = InitializeResult {
        capabilities,
        server_info: Some(server_info),
    };
    connection.initialize_finish(initialize_id, serde_json::to_value(initialize_result)?)?;

    // Main loop: every Request / Notification arrives as Message.
    // L-1 handles only `shutdown` (responds, then waits for `exit`).
    // L-2+ will dispatch other request methods here.
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    // Client requested shutdown; loop exits via the
                    // sender side closing.
                    break;
                }
                // Unknown request — reply with method-not-found so the
                // client doesn't time out.
                let resp = Response::new_err(
                    req.id.clone(),
                    lsp_server::ErrorCode::MethodNotFound as i32,
                    format!("tr lsp L-1 does not yet handle request `{}`", req.method),
                );
                connection.sender.send(Message::Response(resp))?;
                let _ = req;
            }
            Message::Notification(notif) => {
                // L-2 will dispatch textDocument/didOpen / didChange
                // here. L-1 just acknowledges everything by ignoring.
                let _ = notif;
            }
            Message::Response(_resp) => {
                // tr does not currently send any client-bound requests,
                // so all incoming Responses are unsolicited. Ignore.
            }
        }
    }

    io_threads.join()?;
    Ok(())
}

// Suppress dead-code warnings on imports that L-2+ will use.
#[allow(dead_code)]
fn _unused_imports_anchor() {
    let _ = std::any::type_name::<Notification>();
    let _ = std::any::type_name::<Request>();
}

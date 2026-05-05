//! v0.3 #5 LSP server. Speaks Language Server Protocol over stdio
//! so VS Code (and any other LSP-aware editor) can run `tr lsp` as
//! a subprocess and get diagnostics / hover / goto-def for `.ts`
//! sources, sharing the exact lex+parse+check pipeline that
//! `tr build` / `tr run` use.
//!
//! Phase order (per RFC 20260505-lsp-server-skeleton.md):
//!   L-1  initialize / shutdown handshake (DONE)
//!   L-2  document state + check.rs errors → diagnostics (THIS FILE)
//!   L-3  hover (type lookup)
//!   L-4  goto-def
//!   L-5  VS Code extension scaffold + .vsix package
//!   L-6  latency tuning to < 50 ms on 1 K-line file

use std::collections::HashMap;
use std::path::PathBuf;

use lsp_server::{Connection, Message, Notification, Response};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, InitializeParams,
    InitializeResult, Position, PublishDiagnosticsParams, Range,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, Url,
};

const SERVER_NAME: &str = "tr";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

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

    let (initialize_id, initialize_params) = connection.initialize_start()?;
    let _params: InitializeParams =
        serde_json::from_value(initialize_params).unwrap_or_default();

    let initialize_result = InitializeResult {
        capabilities,
        server_info: Some(server_info),
    };
    connection.initialize_finish(initialize_id, serde_json::to_value(initialize_result)?)?;

    // L-2 — in-memory document store. Keyed by Url (file:// or
    // untitled:); value is the latest full text. didChange (Full
    // sync mode) overwrites; didClose removes.
    let mut docs: HashMap<Url, String> = HashMap::new();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    break;
                }
                let resp = Response::new_err(
                    req.id.clone(),
                    lsp_server::ErrorCode::MethodNotFound as i32,
                    format!("tr lsp does not yet handle request `{}`", req.method),
                );
                connection.sender.send(Message::Response(resp))?;
            }
            Message::Notification(notif) => {
                handle_notification(&connection, &mut docs, notif)?;
            }
            Message::Response(_) => {}
        }
    }

    // Dropping the connection releases its sender, which lets the
    // writer io thread observe channel-disconnect and exit cleanly.
    // Without this, io_threads.join() blocks forever on the writer.
    drop(connection);
    io_threads.join()?;
    Ok(())
}

fn handle_notification(
    connection: &Connection,
    docs: &mut HashMap<Url, String>,
    notif: Notification,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    match notif.method.as_str() {
        "textDocument/didOpen" => {
            let p: DidOpenTextDocumentParams =
                serde_json::from_value(notif.params)?;
            let uri = p.text_document.uri.clone();
            let text = p.text_document.text;
            docs.insert(uri.clone(), text.clone());
            publish_diagnostics(connection, &uri, &text)?;
        }
        "textDocument/didChange" => {
            let p: DidChangeTextDocumentParams =
                serde_json::from_value(notif.params)?;
            let uri = p.text_document.uri.clone();
            // Full-sync mode: changes is a single-element vec with
            // the entire new text. (Per the capability we declared.)
            if let Some(change) = p.content_changes.into_iter().next() {
                docs.insert(uri.clone(), change.text.clone());
                publish_diagnostics(connection, &uri, &change.text)?;
            }
        }
        "textDocument/didClose" => {
            let p: DidCloseTextDocumentParams =
                serde_json::from_value(notif.params)?;
            docs.remove(&p.text_document.uri);
            // Clear diagnostics on close so the editor stops showing
            // stale squiggles.
            publish_diagnostics(connection, &p.text_document.uri, "")?;
        }
        _ => {
            // initialized / didSave / configuration noise — ignore.
        }
    }
    Ok(())
}

fn publish_diagnostics(
    connection: &Connection,
    uri: &Url,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let diags = compute_diagnostics(uri, text);
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: diags,
        version: None,
    };
    let notif = Notification::new(
        "textDocument/publishDiagnostics".into(),
        params,
    );
    connection.sender.send(Message::Notification(notif))?;
    Ok(())
}

/// Run lex → parse → desugars → check on `text`. Convert each
/// resulting error string into an LSP `Diagnostic`.
///
/// L-2 minimum: every diagnostic is anchored at the file's first
/// character (Range { start: 0:0, end: 0:1 }) since check.rs's
/// errors don't carry source spans yet. The error message text
/// still surfaces in the editor's hover-over-squiggle popup, so
/// users see WHAT is wrong even when the squiggle position isn't
/// exact. L-2.b refactors check.rs to attach real spans (~80-100
/// push sites).
fn compute_diagnostics(uri: &Url, text: &str) -> Vec<Diagnostic> {
    // Catch panics from lex/parse/desugar so the server stays alive
    // even when the user types syntactically invalid code.
    let computation = std::panic::AssertUnwindSafe(|| {
        let tokens = match crate::lexer::tokenize(text) {
            Ok(t) => t,
            Err(e) => return vec![error_at_origin(format!("lex error: {e}"))],
        };
        let mut ast = match crate::parser::parse(&tokens) {
            Ok(a) => a,
            Err(e) => return vec![error_at_origin(format!("parse error: {e}"))],
        };
        ast.source = text.to_string();
        ast.warm_newline_cache();

        // Resolve cross-file imports relative to the document's
        // directory if it's a file:// URL. Failures here surface
        // as a single import-error diagnostic.
        let base_dir = uri
            .to_file_path()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        if let Err(e) = crate::modules::resolve_imports(&mut ast, &base_dir) {
            return vec![error_at_origin(format!("import error: {e}"))];
        }

        crate::ast::unwrap_exports(&mut ast);
        crate::ast::desugar_generators(&mut ast);
        crate::ast::desugar_async(&mut ast);
        crate::ast::desugar_builtin_imports(&mut ast);
        crate::ast::desugar_builtin_new(&mut ast);
        crate::ast::desugar_classes(&mut ast);
        crate::ast::lift_arrow_fns(&mut ast);
        crate::ast::infer_anonymous_closure_params(&mut ast);
        crate::ast::synthesize_forwarders(&mut ast);
        crate::ast::desugar_uninit_let(&mut ast);
        crate::ast::desugar_arguments_object(&mut ast);
        crate::ast::rewrite_split_for_i_to_iter(&mut ast);
        crate::ast::escape_analyze_array_literals(&mut ast);
        crate::ast::desugar_implicit_generics(&mut ast);
        crate::ast::apply_default_args(&mut ast);
        crate::ast::apply_rest_args(&mut ast);
        crate::ast::compute_consuming_params(&mut ast);

        crate::check::collect_errors(&ast)
            .into_iter()
            .map(error_at_origin)
            .collect()
    });

    match std::panic::catch_unwind(computation) {
        Ok(diags) => diags,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "internal error during typecheck".to_string()
            };
            vec![error_at_origin(format!("not yet supported: {msg}"))]
        }
    }
}

/// Build a single-character diagnostic at file:1:1 — the L-2
/// anchor used while errors don't carry per-site spans.
fn error_at_origin(message: String) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 1 },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: Some(SERVER_NAME.into()),
        message,
        related_information: None,
        tags: None,
        data: None,
    }
}

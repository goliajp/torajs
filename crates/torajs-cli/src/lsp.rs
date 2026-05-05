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

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, Location, MarkupContent, MarkupKind,
    OneOf, Position, PublishDiagnosticsParams, Range, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};

const SERVER_NAME: &str = "tr";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
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
                handle_request(&connection, &docs, req)?;
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
        let tokens = match torajs_core::lexer::tokenize(text) {
            Ok(t) => t,
            Err(e) => return vec![error_at_origin(format!("lex error: {e}"))],
        };
        let mut ast = match torajs_core::parser::parse(&tokens) {
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
        if let Err(e) = torajs_core::modules::resolve_imports(&mut ast, &base_dir) {
            return vec![error_at_origin(format!("import error: {e}"))];
        }

        torajs_core::ast::unwrap_exports(&mut ast);
        torajs_core::ast::desugar_generators(&mut ast);
        torajs_core::ast::desugar_async(&mut ast);
        torajs_core::ast::desugar_builtin_imports(&mut ast);
        torajs_core::ast::desugar_builtin_new(&mut ast);
        torajs_core::ast::desugar_classes(&mut ast);
        torajs_core::ast::lift_arrow_fns(&mut ast);
        torajs_core::ast::infer_anonymous_closure_params(&mut ast);
        torajs_core::ast::synthesize_forwarders(&mut ast);
        torajs_core::ast::desugar_uninit_let(&mut ast);
        torajs_core::ast::desugar_arguments_object(&mut ast);
        torajs_core::ast::rewrite_split_for_i_to_iter(&mut ast);
        torajs_core::ast::escape_analyze_array_literals(&mut ast);
        torajs_core::ast::desugar_implicit_generics(&mut ast);
        torajs_core::ast::apply_default_args(&mut ast);
        torajs_core::ast::apply_rest_args(&mut ast);
        torajs_core::ast::compute_consuming_params(&mut ast);

        // T-04 (v0.3.0) — switch to the diagnostic stream so warnings
        // surface alongside errors and per-site spans (where attached)
        // anchor the editor squiggle. Sentinel span (0, 0) still falls
        // back to file:1:1 via `byte_to_position` returning (0, 0).
        let text_for_pos = ast.source.clone();
        torajs_core::check::collect_diagnostics(&ast)
            .into_iter()
            .map(|d| diagnostic_from_core(&text_for_pos, d))
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

/// Build a single-character diagnostic at file:1:1 — used by the
/// pre-typecheck error paths (lex / parse / import resolution) where
/// no Diagnostic span is available because the typechecker hasn't
/// run yet.
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

/// T-04 (v0.3.0) — convert torajs_core::check::Diagnostic to LSP
/// Diagnostic. Span (start = 0, end = 0) is the sentinel for
/// "no source location attached" and renders as the file:1:1
/// single-char range; once a push site attaches a real span via
/// `push_err_at(eid, msg)`, the squiggle lands at the correct
/// `byte_to_position(span.start)..byte_to_position(span.end)`.
fn diagnostic_from_core(text: &str, d: torajs_core::check::Diagnostic) -> Diagnostic {
    let range = if d.span.start == 0 && d.span.end == 0 {
        Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: 1 },
        }
    } else {
        Range {
            start: byte_to_position(text, d.span.start),
            end: byte_to_position(text, d.span.end),
        }
    };
    let severity = Some(match d.severity {
        torajs_core::check::Severity::Error => DiagnosticSeverity::ERROR,
        torajs_core::check::Severity::Warning => DiagnosticSeverity::WARNING,
    });
    Diagnostic {
        range,
        severity,
        code: None,
        code_description: None,
        source: Some(SERVER_NAME.into()),
        message: d.message,
        related_information: None,
        tags: None,
        data: None,
    }
}

fn handle_request(
    connection: &Connection,
    docs: &HashMap<Url, String>,
    req: Request,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let response = match req.method.as_str() {
        "textDocument/hover" => {
            let p: HoverParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return send_err(connection, req.id, format!("hover params: {e}"));
                }
            };
            let uri = p.text_document_position_params.text_document.uri;
            let pos = p.text_document_position_params.position;
            let hover = docs
                .get(&uri)
                .and_then(|text| compute_hover(text, pos));
            Response::new_ok(req.id, hover)
        }
        "textDocument/definition" => {
            let p: GotoDefinitionParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return send_err(connection, req.id, format!("definition params: {e}"));
                }
            };
            let uri = p.text_document_position_params.text_document.uri;
            let pos = p.text_document_position_params.position;
            let location = docs
                .get(&uri)
                .and_then(|text| compute_definition(text, pos))
                .map(|range| Location { uri: uri.clone(), range });
            let resp: Option<GotoDefinitionResponse> =
                location.map(GotoDefinitionResponse::Scalar);
            Response::new_ok(req.id, resp)
        }
        _ => Response::new_err(
            req.id.clone(),
            lsp_server::ErrorCode::MethodNotFound as i32,
            format!("tr lsp does not yet handle request `{}`", req.method),
        ),
    };
    connection.sender.send(Message::Response(response))?;
    Ok(())
}

fn send_err(
    connection: &Connection,
    id: lsp_server::RequestId,
    msg: String,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let resp = Response::new_err(id, lsp_server::ErrorCode::InvalidParams as i32, msg);
    connection.sender.send(Message::Response(resp))?;
    Ok(())
}

/// L-3 — hover handler. Re-runs the typecheck pipeline (cached
/// types not yet implemented; that's L-6 perf), translates the
/// LSP (line, char) position to a byte offset, finds the smallest
/// Expr whose source span contains that offset, and looks up its
/// inferred type from the side table that `collect_types_and_errors`
/// populates as it walks. Returns None when the position doesn't
/// land on any typed Expr.
fn compute_hover(text: &str, pos: Position) -> Option<Hover> {
    let computation = std::panic::AssertUnwindSafe(|| {
        let tokens = torajs_core::lexer::tokenize(text).ok()?;
        let mut ast = torajs_core::parser::parse(&tokens).ok()?;
        ast.source = text.to_string();
        ast.warm_newline_cache();
        // No cross-file resolution + no desugars on the hover path;
        // they could mutate spans in ways that confuse the (line, col)
        // → ExprId lookup. The base parsed AST has the spans the user
        // sees in the editor.
        let (expr_types, _errs) = torajs_core::check::collect_types_and_errors(&ast);

        // Convert (line, char) to byte offset. LSP positions are
        // 0-indexed, UTF-16 code units; we treat them as UTF-8
        // byte offsets (good enough for ASCII; multibyte support
        // is a follow-up).
        let byte = position_to_byte(text, pos)?;
        let eid = smallest_containing_expr(&ast, byte)?;
        let ty = expr_types.get(&eid)?;
        let formatted = torajs_core::check::type_to_ann(ty);
        let span = ast.expr_spans.get(eid.0 as usize)?;
        let start_pos = byte_to_position(text, span.start);
        let end_pos = byte_to_position(text, span.end);
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```typescript\n{formatted}\n```"),
            }),
            range: Some(Range {
                start: start_pos,
                end: end_pos,
            }),
        })
    });
    std::panic::catch_unwind(computation).ok().flatten()
}

fn position_to_byte(text: &str, pos: Position) -> Option<u32> {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if line == pos.line && col == pos.character {
            return Some(i as u32);
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    if line == pos.line && col == pos.character {
        return Some(text.len() as u32);
    }
    None
}

fn byte_to_position(text: &str, byte: u32) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    let target = byte as usize;
    for (i, ch) in text.char_indices() {
        if i >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position { line, character: col }
}

/// L-4 — goto-def. Approximate symbol table built by source-text
/// regex over the document, capturing the byte offset of each
/// `function NAME` / `class NAME` / `type NAME` / `let NAME` /
/// `const NAME` declaration. Returns the Range of the matched
/// declaration name when the cursor is on an Ident reference
/// to that name. Limitations:
///   - No scope handling — same-name shadows resolve to the FIRST
///     declaration in source order. Acceptable for a v0.3 minimum
///     gate; proper scoping needs Checker symbol table integration
///     (deferred to L-4.b).
///   - No method / field navigation — only top-level + let-bound
///     names. Methods need class-table integration (L-4.b).
///   - No cross-file resolution — `import { foo } from "./bar"`
///     would need module-graph navigation (L-4.c, after symbol
///     scoping lands).
fn compute_definition(text: &str, pos: Position) -> Option<Range> {
    let computation = std::panic::AssertUnwindSafe(|| {
        let byte = position_to_byte(text, pos)?;
        let name = ident_at_byte(text, byte)?;
        let symbols = scan_top_level_symbols(text);
        let &decl_byte = symbols.get(&name)?;
        let decl_end = decl_byte + name.len() as u32;
        Some(Range {
            start: byte_to_position(text, decl_byte),
            end: byte_to_position(text, decl_end),
        })
    });
    std::panic::catch_unwind(computation).ok().flatten()
}

/// Find the contiguous identifier-shaped token at `byte`. Identifier
/// chars are `[A-Za-z0-9_$]`. Returns None if `byte` doesn't land on
/// such a char.
fn ident_at_byte(text: &str, byte: u32) -> Option<String> {
    let bytes = text.as_bytes();
    let i = byte as usize;
    if i >= bytes.len() {
        return None;
    }
    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    if !is_ident_byte(bytes[i]) {
        return None;
    }
    let mut start = i;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = i + 1;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    Some(std::str::from_utf8(&bytes[start..end]).ok()?.to_string())
}

/// One-pass source scan. Records the byte offset of the NAME token
/// in each top-level declaration shape:
///   - `function NAME(`
///   - `function* NAME(`
///   - `async function NAME(`
///   - `class NAME`
///   - `type NAME`
///   - `let NAME`
///   - `const NAME`
/// Same-name shadows: the FIRST occurrence wins, matching how the
/// editor "jump to top of file" intuition works for most user code.
fn scan_top_level_symbols(text: &str) -> HashMap<String, u32> {
    let mut symbols: HashMap<String, u32> = HashMap::new();
    let bytes = text.as_bytes();
    let keywords: &[&[u8]] = &[
        b"function", b"class", b"type", b"let", b"const",
    ];
    let mut i = 0usize;
    while i < bytes.len() {
        // Skip line comments.
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Skip block comments.
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        // Skip string literals (single, double, backtick) — naive,
        // doesn't handle nested template expressions but skips the
        // common cases that would otherwise produce false-positive
        // keyword matches inside strings.
        if matches!(bytes[i], b'"' | b'\'' | b'`') {
            let q = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != q {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            continue;
        }
        // Try to match a declaration keyword at this position.
        // Must be at a token boundary: previous char is whitespace /
        // start-of-file, and following char is whitespace.
        let at_boundary = i == 0
            || matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'\r' | b';' | b'}' | b'{');
        if at_boundary {
            for kw in keywords {
                if i + kw.len() < bytes.len()
                    && &bytes[i..i + kw.len()] == *kw
                    && matches!(bytes[i + kw.len()], b' ' | b'\t' | b'\n' | b'*' | b'(')
                {
                    // Found keyword. Advance past it + optional `*`
                    // (generator) + whitespace, then read the name
                    // token.
                    let mut j = i + kw.len();
                    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'*') {
                        j += 1;
                    }
                    let name_start = j;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'$')
                    {
                        j += 1;
                    }
                    if j > name_start {
                        let name = std::str::from_utf8(&bytes[name_start..j])
                            .unwrap_or("")
                            .to_string();
                        symbols.entry(name).or_insert(name_start as u32);
                    }
                    i = j;
                    break;
                }
            }
        }
        i += 1;
    }
    symbols
}

/// Walk every Expr looking for the smallest span containing `byte`.
/// O(n) over the arena — fine for hover latency on 1 K-line files;
/// L-6 may add a position index if needed.
fn smallest_containing_expr(ast: &torajs_core::ast::Ast, byte: u32) -> Option<torajs_core::ast::ExprId> {
    let mut best: Option<(torajs_core::ast::ExprId, u32)> = None;
    for (i, span) in ast.expr_spans.iter().enumerate() {
        if span.start == 0 && span.end == 0 {
            continue;
        }
        if byte >= span.start && byte < span.end {
            let width = span.end - span.start;
            match best {
                None => best = Some((torajs_core::ast::ExprId(i as u32), width)),
                Some((_, prev_w)) if width < prev_w => {
                    best = Some((torajs_core::ast::ExprId(i as u32), width));
                }
                _ => {}
            }
        }
    }
    best.map(|(id, _)| id)
}

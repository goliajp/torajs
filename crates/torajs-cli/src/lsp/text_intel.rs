//! Text-level helpers for LSP code-intel: position/byte conversion,
//! identifier lookup at byte offset, and a one-pass symbol scan that
//! powers goto-def + hover navigation. All operate on raw `&str`
//! source — they don't depend on the parser / checker pipeline.
//!
//! Extracted from `lsp.rs` (2026-05-25, god-file decomp).

use std::collections::HashMap;

use lsp_types::{Position, Range};

pub(super) fn position_to_byte(text: &str, pos: Position) -> Option<u32> {
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

pub(super) fn byte_to_position(text: &str, byte: u32) -> Position {
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
    Position {
        line,
        character: col,
    }
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
pub(super) fn compute_definition(text: &str, pos: Position) -> Option<Range> {
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
pub(super) fn ident_at_byte(text: &str, byte: u32) -> Option<String> {
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
pub(super) fn scan_top_level_symbols(text: &str) -> HashMap<String, u32> {
    let mut symbols: HashMap<String, u32> = HashMap::new();
    let bytes = text.as_bytes();
    let keywords: &[&[u8]] = &[b"function", b"class", b"type", b"let", b"const"];
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
            || matches!(
                bytes[i - 1],
                b' ' | b'\t' | b'\n' | b'\r' | b';' | b'}' | b'{'
            );
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
                        && (bytes[j].is_ascii_alphanumeric()
                            || bytes[j] == b'_'
                            || bytes[j] == b'$')
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
pub(super) fn smallest_containing_expr(
    ast: &torajs_core::ast::Ast,
    byte: u32,
) -> Option<torajs_core::ast::ExprId> {
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

//! Token-cursor and span helpers for [`Parser`] — peek (with the
//! S155 nested-generic `>>` peel), byte-position probes, the ES
//! §12.9.1 LineTerminator restricted-production scan, and the DWARF
//! span-recording arena wrappers. Verbatim move out of `parser.rs`
//! (rotation 280 file-size clean: the yield-hoist fields + two new
//! module declarations pushed it past the 500-line HARD limit);
//! private fns became `pub(super)` for the sibling-impl pack.

use super::*;

impl Parser<'_> {
    pub(super) fn peek(&self) -> &Token {
        // S155 — within nested generic type args, `>>` / `>>>` peel into
        // multiple `>`s. When `type_close_peel` is non-zero the current
        // ShrShr/ShrShrShr token has had its leading `>`(s) consumed
        // virtually; report the next virtual `>` (or `>>` after one
        // peel from `>>>`).
        if self.type_close_peel > 0 {
            match &self.tokens[self.pos].token {
                Token::ShrShr if self.type_close_peel == 1 => return &Token::Gt,
                Token::ShrShrShr if self.type_close_peel == 1 => return &Token::ShrShr,
                Token::ShrShrShr if self.type_close_peel == 2 => return &Token::Gt,
                _ => {}
            }
        }
        &self.tokens[self.pos].token
    }

    pub(super) fn at(&self) -> u32 {
        self.tokens[self.pos].span.start
    }

    /// ES §12.9.1 restricted production probe — was there a
    /// LineTerminator (LF/CR/U+2028/U+2029) in the whitespace slice
    /// between the previous consumed token and `self.tokens[at]`?
    /// Returns `false` at the start of the token stream (no "previous
    /// token" to measure from).
    ///
    /// Callers:
    /// - `parse_return`'s expr-parse gate (`return [no LT] Expr?;`)
    /// - `parse_postfix`'s `++` / `--` arm
    ///   (`LHS [no LT] (++|--)` — a leading `++` / `--` after a
    ///   newline is a *prefix* op on the next stmt, not a postfix on
    ///   the previous LHS).
    ///
    /// Cost: one linear scan over the between-token whitespace slice,
    /// which is only walked on the restricted-production sites (a few
    /// per parse). Kept simple over caching a per-token bit since the
    /// call sites are rare.
    pub(super) fn has_newline_before(&self, at: usize) -> bool {
        if at == 0 || at > self.tokens.len() {
            return false;
        }
        let prev_end = self.tokens[at - 1].span.end as usize;
        let cur_start = if at < self.tokens.len() {
            self.tokens[at].span.start as usize
        } else {
            self.source.len()
        };
        if cur_start <= prev_end {
            return false;
        }
        let bytes = self.source.as_bytes();
        let end = cur_start.min(bytes.len());
        let mut i = prev_end;
        while i < end {
            let b = bytes[i];
            if b == b'\n' || b == b'\r' {
                return true;
            }
            // U+2028 LINE SEPARATOR = E2 80 A8, U+2029 PARAGRAPH
            // SEPARATOR = E2 80 A9. Both are LineTerminators per
            // §12.3 TABLE.
            if b == 0xE2 && i + 2 < end && bytes[i + 1] == 0x80 {
                let b2 = bytes[i + 2];
                if b2 == 0xA8 || b2 == 0xA9 {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// v0.3 #4 DWARF — add an Expr to the arena AND record its source
    /// byte range. `start_pos` is the *token index* where the expr
    /// began (typically captured before recursive descent); end byte
    /// is taken from the token just consumed (`self.pos - 1`).
    /// Defaults to (0, 0) sentinel if either index is OOB so callers
    /// don't have to thread Option through.
    pub(super) fn add_expr_at(&mut self, start_pos: usize, e: Expr) -> ExprId {
        let start = self
            .tokens
            .get(start_pos)
            .map(|t| t.span.start)
            .unwrap_or(0);
        let end = if self.pos > 0 {
            self.tokens
                .get(self.pos - 1)
                .map(|t| t.span.end)
                .unwrap_or(start)
        } else {
            start
        };
        let id = self.ast.add_expr(e);
        self.ast
            .set_expr_span(id, crate::lexer::Span { start, end });
        id
    }

    /// Re-anchor an already-added expr's span to begin at
    /// `start_pos`'s token (same byte formula as [`Self::add_expr_at`]).
    /// For wrapper syntax parsed before the node's own parser ran —
    /// `async (x) => …` delegates to `parse_arrow_fn`, whose anchor
    /// sits on `(` and would drop the `async ` prefix from the
    /// recorded source range (RFC 20260719-fn-tostring-source B1).
    pub(super) fn respan_expr(&mut self, eid: ExprId, start_pos: usize) {
        let span = self.span_from(start_pos);
        self.ast.set_expr_span(eid, span);
    }

    /// Byte span from `start_pos`'s token through the token just
    /// consumed — the [`Self::add_expr_at`] formula as a value, for
    /// nodes that carry their span inline (`Stmt::FnDecl`, B1b).
    pub(super) fn span_from(&self, start_pos: usize) -> crate::lexer::Span {
        let start = self
            .tokens
            .get(start_pos)
            .map(|t| t.span.start)
            .unwrap_or(0);
        let end = if self.pos > 0 {
            self.tokens
                .get(self.pos - 1)
                .map(|t| t.span.end)
                .unwrap_or(start)
        } else {
            start
        };
        crate::lexer::Span { start, end }
    }
}

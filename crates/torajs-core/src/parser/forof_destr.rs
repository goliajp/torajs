//! for-of destructuring-pattern scanners + destructured-body wrapper,
//! split from `try_parse_for_of` (2026-07-03, fn-debt decomp).
//!
//! Bodies verbatim from the pre-split fn; mechanical rewrites: the
//! `self.pos = saved; return Ok(None);` bail tails become
//! `ForOfPatScan::Bail` (the caller restores `saved` and returns),
//! the dead `self.pos = start;` pre-bail assignments drop with the
//! now-unused `start` checkpoints, and `var_name.clone()` →
//! `var_name.to_string()` (`&str` param).

use super::*;

/// Three-state result of a for-of binding-pattern lookahead: the
/// next token does not open this pattern kind (fall through), the
/// pattern matched, or the whole for-of attempt must be abandoned
/// (caller restores its saved token position and yields to the
/// C-style `for` parser).
pub(super) enum ForOfPatScan<T> {
    NotPattern,
    Bail,
    Pat(T),
}

impl<'a> Parser<'a> {
    /// `for (let [a, b] of pairs)` array-pattern lookahead (V3-18
    /// wedge) — names collected; `: T[]` annotation discarded.
    pub(super) fn scan_forof_destr_array(&mut self) -> ForOfPatScan<Vec<String>> {
        if !matches!(self.peek(), Token::LBracket) {
            return ForOfPatScan::NotPattern;
        }
        self.pos += 1;
        let mut names: Vec<String> = Vec::new();
        let ok = loop {
            match self.peek() {
                Token::Ident(n) => {
                    names.push(n.clone());
                    self.pos += 1;
                }
                _ => break false,
            }
            match self.peek() {
                Token::Comma => {
                    self.pos += 1;
                    if matches!(self.peek(), Token::RBracket) {
                        break true;
                    }
                }
                Token::RBracket => break true,
                _ => break false,
            }
        };
        if !ok {
            return ForOfPatScan::Bail;
        }
        self.pos += 1; // consume `]`
        // Optional `: T[]` annotation on the pattern — discarded.
        if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            let _ = self.parse_type_ann();
        }
        let is_of = matches!(self.peek(), Token::Ident(n) if n == "of");
        if !is_of {
            // Not for-of after destructuring; surrender — likely
            // a let-destructuring statement which won't reach here.
            return ForOfPatScan::Bail;
        }
        ForOfPatScan::Pat(names)
    }

    /// `for (let { x, y } of pts)` object-pattern lookahead (V3-18
    /// wedge) — `field: bound` renames handled; reserved-word
    /// fields go through keyword_property_name and require an
    /// explicit rename.
    pub(super) fn scan_forof_destr_obj(&mut self) -> ForOfPatScan<Vec<(String, String)>> {
        if !matches!(self.peek(), Token::LBrace) {
            return ForOfPatScan::NotPattern;
        }
        self.pos += 1; // consume `{`
        let mut entries: Vec<(String, String)> = Vec::new();
        let ok = loop {
            let (field, field_is_kw) = match self.peek() {
                Token::Ident(n) => (n.clone(), false),
                t if Self::keyword_property_name(t).is_some() => {
                    (Self::keyword_property_name(t).unwrap().to_string(), true)
                }
                _ => break false,
            };
            self.pos += 1;
            let bound = if matches!(self.peek(), Token::Colon) {
                self.pos += 1;
                match self.peek() {
                    Token::Ident(n) => {
                        let nn = n.clone();
                        self.pos += 1;
                        nn
                    }
                    _ => break false,
                }
            } else {
                if field_is_kw {
                    // Same diagnostic as parse_object_destructuring:
                    // can't use a reserved word as a binding name.
                    break false;
                }
                field.clone()
            };
            entries.push((field, bound));
            match self.peek() {
                Token::Comma => {
                    self.pos += 1;
                    if matches!(self.peek(), Token::RBrace) {
                        break true;
                    }
                }
                Token::RBrace => break true,
                _ => break false,
            }
        };
        if !ok {
            return ForOfPatScan::Bail;
        }
        self.pos += 1; // consume `}`
        // Optional `: T` annotation on the pattern — discarded.
        if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            let _ = self.parse_type_ann();
        }
        let is_of = matches!(self.peek(), Token::Ident(n) if n == "of");
        if !is_of {
            return ForOfPatScan::Bail;
        }
        ForOfPatScan::Pat(entries)
    }

    /// Prepend per-element / per-field destructuring lets when the
    /// loop var was a pattern (V3-18 wedge) — the original `body`
    /// is wrapped in a block so block-close drops still fire
    /// normally.
    pub(super) fn wrap_forof_destr_body(
        &mut self,
        destruct_names: &Option<Vec<String>>,
        destruct_obj: &Option<Vec<(String, String)>>,
        var_name: &str,
        mut body: Stmt,
    ) -> Stmt {
        if let Some(names) = destruct_names {
            let mut pre: Vec<Stmt> = Vec::new();
            for (i, n) in names.iter().enumerate() {
                let src_ref = self.ast.add_expr(Expr::Ident(var_name.to_string()));
                let idx = self.ast.add_expr(Expr::Number(i as f64));
                let elem = self.ast.add_expr(Expr::Index {
                    obj: src_ref,
                    index: idx,
                });
                pre.push(Stmt::LetDecl {
                    mutable: false,
                    name: n.clone(),
                    type_ann: None,
                    init: elem,
                    is_var: false,
                });
            }
            pre.push(body);
            body = Stmt::Block(pre);
        } else if let Some(entries) = destruct_obj {
            let mut pre: Vec<Stmt> = Vec::new();
            for (field, bound) in entries {
                let src_ref = self.ast.add_expr(Expr::Ident(var_name.to_string()));
                let elem = self.ast.add_expr(Expr::Member {
                    obj: src_ref,
                    name: field.clone(),
                });
                pre.push(Stmt::LetDecl {
                    mutable: false,
                    name: bound.clone(),
                    type_ann: None,
                    init: elem,
                    is_var: false,
                });
            }
            pre.push(body);
            body = Stmt::Block(pre);
        }
        body
    }
}

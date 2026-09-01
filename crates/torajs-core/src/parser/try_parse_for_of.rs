//! `Parser::try_parse_for_of` extracted from `parser.rs` (chunk 163).
//!
//! Pre-extract this method was 421 LOC inside `impl Parser` block.
//! Body verbatim moves here as an `impl` block sibling.
//!
//! `try_parse_for_of` is the `for (let v of ...)` / `for await (let v
//! of ...)` parse path; it peek-lookaheads through the `for` + `(`
//! tokens to detect the of-of-form vs the C-style `for (init; cond;
//! step)`. Returns `Ok(None)` on non-for-of shape so `parse_stmt`
//! can fall through to `parse_for`. Body unchanged.
//!
//! 2026-07-03 fn-debt decomp: generator desugar + default ForOf
//! tail split into sub-fns below. RFC 20260727-dstr-decl-shape 刀 B:
//! decl-head patterns read via destr_shape's recursive PatShape
//! machine (forof_binding.rs); the flat forof_destr scanners are gone.

use super::*;

/// chunk B2/B3 — prepend the head-form prelude statements (the
/// fn-scoped `var k;` declaration and/or the for-in object hoist
/// `let __forin_obj_N = <src>;`) before the loop statement.
fn wrap_prelude(mut prelude: Vec<Stmt>, stmt: Stmt) -> Stmt {
    if prelude.is_empty() {
        stmt
    } else {
        prelude.push(stmt);
        Stmt::Block(prelude)
    }
}

impl<'a> Parser<'a> {
    /// chunk B2 (RFC 20260711 for-in) — head-form gate. Three forms:
    /// - `let` / `const`: block-scoped binding (`Some(false)`, false).
    /// - `var`: fn-scoped (`Some(true)`, false) — binds a fresh
    ///   loop-local and assigns the user's var-hoisted binding per
    ///   iteration.
    /// - bare `for (k in o)` / `for (k of a)` (`None`, true): assigns
    ///   an existing binding per iteration (no declaration). Gated on
    ///   IDENT immediately followed by contextual `of` / `in` so
    ///   C-style inits and destructuring pattern heads fall through.
    ///
    /// Consumes the decl keyword on the decl forms. `None` = not a
    /// for-of/for-in head at all.
    fn scan_forof_head(&mut self) -> Option<(Option<bool>, bool, Option<bool>)> {
        // RFC 20260809 knife 3 — `for ([await] using x of …)` head;
        // see `forof_using.rs` for the shape test.
        if let Some(hint) = self.scan_forof_using_head() {
            return Some((Some(false), false, Some(hint)));
        }
        let is_var_decl = match self.peek() {
            Token::Let | Token::Const => Some(false),
            Token::Var => Some(true),
            _ => None,
        };
        let bare_form = is_var_decl.is_none();
        if bare_form {
            // S2.24 刀 2 — `[` / `{` opens a bare assignment-pattern
            // head (`for ([a, b] of …)`); the caller scans + validates
            // it and restores on a non-of/in follow.
            if matches!(self.peek(), Token::LBracket | Token::LBrace) {
                return Some((None, true, None));
            }
            let next_is_of_in = matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.token),
                Some(Token::Ident(n)) if n == "of" || n == "in"
            );
            if !matches!(self.peek(), Token::Ident(_)) || !next_is_of_in {
                return None;
            }
        } else {
            self.pos += 1;
        }
        Some((is_var_decl, bare_form, None))
    }

    /// chunk B2 — `var` head form: the fn-scoped declaration
    /// (`var k;`, Uninit init) prepended before the loop so var-hoist
    /// lifts it and the binding leaks past the loop per §14.7.5 /
    /// §14.3.2 var semantics.
    fn make_forof_var_decl(
        &mut self,
        is_var_decl: Option<bool>,
        assign_target: &Option<String>,
    ) -> Option<Stmt> {
        if is_var_decl != Some(true) {
            return None;
        }
        let target = assign_target.as_ref()?;
        let init = self.ast.add_expr(Expr::Uninit);
        Some(Stmt::LetDecl {
            mutable: true,
            name: target.clone(),
            type_ann: None,
            init,
            is_var: true,
        })
    }

    /// chunk B2 — assignment-form body wrap: `{ k = __forvar_N; body }`
    /// so the user's binding tracks the fresh loop-local each
    /// iteration (`var` and bare head forms).
    fn wrap_assign_form(&mut self, target: &str, fresh: &str, body: Stmt) -> Stmt {
        let target_ref = self.ast.add_expr(Expr::Ident(target.to_string()));
        let fresh_ref = self.ast.add_expr(Expr::Ident(fresh.to_string()));
        let assign = self.ast.add_expr(Expr::Assign {
            target: target_ref,
            value: fresh_ref,
        });
        Stmt::Block(vec![Stmt::Expr(assign), body])
    }

    pub(super) fn try_parse_for_of(&mut self, is_async: bool) -> Result<Option<Stmt>, String> {
        let saved = self.pos;
        let Some((is_var_decl, bare_form, is_using)) = self.scan_forof_head() else {
            return Ok(None);
        };
        self.reject_forof_using_misuse(is_using, None)?;
        // S2.24 刀 2 — bare assignment-pattern head: a fresh loop
        // local receives each element; the body prepends the same
        // pattern-assignment expansion the statement form uses.
        let mut destruct_assign: Option<ExprId> = None;
        let (destruct_pat, var_name, assign_target) = if bare_form
            && matches!(self.peek(), Token::LBracket | Token::LBrace)
        {
            let Some(pat) = self.scan_forof_assign_pattern() else {
                self.pos = saved;
                return Ok(None);
            };
            destruct_assign = Some(pat);
            let id = self.mint_desugar_id();
            (None, format!("__forvar_{id}"), None)
        } else {
            let Some(head) = self.parse_forof_binding_and_pattern(saved, is_var_decl, bare_form)
            else {
                return Ok(None);
            };
            head
        };
        // P5.3 — preserve the optional `: T` annotation on the
        // binding name so check.rs / ssa_lower can pin the var's
        // type when src's element type is harder to infer (Type::Any
        // sources or user iterables).
        let var_type_ann: Option<String> = if matches!(self.peek(), Token::Colon) {
            self.pos += 1;
            Some(self.parse_type_ann()?)
        } else {
            None
        };
        let have_type_ann = var_type_ann.is_some();
        // Contextual `of` / `in` keyword — must be an Ident. Anything
        // else (`=` for a regular let-in-init, `;` for empty init, etc.)
        // means this is NOT a for-of/in and we restore.
        let kind = match self.peek() {
            Token::Ident(n) if n == "of" => Some("of"),
            Token::Ident(n) if n == "in" => Some("in"),
            _ => None,
        };
        let Some(kind) = kind else {
            self.pos = saved;
            return Ok(None);
        };
        self.pos += 1; // consume "of" / "in"
        self.reject_forof_using_misuse(is_using, Some(kind))?;
        let _ = have_type_ann; // not yet propagated; suppress unused warning
        let raw_src = self.parse_expr()?;
        let (src, forin_obj, forin_obj_hoist) = if kind == "in" {
            let (s, o, h) = self.make_forin_src(raw_src);
            (s, Some(o), Some(h))
        } else {
            (raw_src, None, None)
        };
        match self.peek() {
            Token::RParen => self.pos += 1,
            t => {
                return Err(format!(
                    "expected `)` after for-of source, got {t:?} at {}",
                    self.at()
                ));
            }
        }
        let body_start = self.pos;
        let body = self.parse_stmt()?;
        // §14.7.5 ForIn/OfStatement takes a Statement, not a
        // Declaration — same gate every other single-stmt body has
        // (r294 刀 4; test262 for-of/decl-cls family).
        self.reject_decl_in_single_stmt(&body, body_start, "a for-of loop")?;
        // RFC 20260809 knife 3 — the body opens with a per-iteration
        // UsingDecl over a fresh loop local; see `forof_using.rs`.
        let (var_name, body) = if let Some(hint_await) = is_using {
            self.wrap_forof_using_body(var_name, body, hint_await)
        } else {
            (var_name, body)
        };
        // RFC 20260727-dstr-decl-shape 刀 B — prepend the recursive
        // pattern binds when the loop var was a decl-head pattern.
        let body = self.wrap_forof_pattern_body(&destruct_pat, &var_name, body);
        // chunk B2 — `var` / bare forms: assign the user's binding
        // from the fresh loop-local at the top of each iteration.
        let body = if let Some(target) = &assign_target {
            self.wrap_assign_form(target, &var_name, body)
        } else {
            body
        };
        // S2.24 刀 2 — bare pattern head: destructure the fresh
        // loop-local into the existing bindings at the top of each
        // iteration (desugar_dstr_assign hoists it into its own temp,
        // then assigns per slot).
        let body = if let Some(pat) = destruct_assign {
            let var_ref = self.ast.add_expr(Expr::Ident(var_name.clone()));
            let mut pre = self.desugar_dstr_assign(pat, var_ref)?;
            pre.push(body);
            Stmt::Block(pre)
        } else {
            body
        };
        let var_decl = self.make_forof_var_decl(is_var_decl, &assign_target);
        // Prelude order: the fn-scoped `var k;` first, then the
        // for-in object hoist (its init may reference `k`-free exprs
        // only, but keeping decl-before-init order is the safe form).
        let prelude: Vec<Stmt> = var_decl.into_iter().chain(forin_obj_hoist).collect();

        // P-iter — `for (let v of <expr>.split(<literal_sep>))` →
        // emit Stmt::ForOfSplitIter. ssa_lower handles via stack
        // alloca'd SplitIter struct + per-iter substr borrow,
        // skipping eager Array<Substr> materialization.
        //
        // Conservative match: sep MUST be a string-literal Expr so the
        // iter's borrow of sep_data is guaranteed alive (literals are
        // STATIC_LITERAL globals with infinite refcount). Variable
        // sep falls back to the generic for-of array path below.
        if kind == "of"
            && let Expr::Call { callee, args } = self.ast.get_expr(src)
            && let Expr::Member {
                obj: parent,
                name: m_name,
            } = self.ast.get_expr(*callee)
            && m_name == "split"
            && args.len() == 1
            && matches!(self.ast.get_expr(args[0]), Expr::String(_))
        {
            let parent_id = *parent;
            let sep_id = args[0];
            return Ok(Some(wrap_prelude(
                prelude,
                Stmt::ForOfSplitIter {
                    var_name,
                    parent: parent_id,
                    sep: sep_id,
                    body: Box::new(body),
                },
            )));
        }

        // Rotation 552 (551-06) — a call to a known generator factory
        // (`for (const v of gen())`) takes the default lane below like
        // every other source. The parse-time I.2 desugar that stood
        // here (`let __it = gen(); while (true) { let __step =
        // __it.next(); if (__step.done) break; … }`) predates typed
        // for-of over a class iterator and had no IteratorClose: a
        // `break` / `return` / throw out of the loop never ran the
        // generator's `return()`, so its `finally` never ran — while
        // the same loop over `const it = gen()` (an Ident source, the
        // iter_protocol lane) closed correctly. The desugar's own
        // comment recorded the Ident case as its limitation; the lane
        // it deferred to now handles both, with §7.4.9 on every exit
        // (RFC 20260901-scope-exit-drops 刀 2).

        // P5.3 — default for-of emits Stmt::ForOf wrapped in a
        // Stmt::Block that hoists src to a fresh Ident binding (or
        // reuses the source Ident). ssa_lower dispatches on the
        // bound src's check-time type to pick array-walk vs iterator-
        // protocol. The pre-allocated `elem_expr = src_ident[i_ident]`
        // routes element loads through Expr::Index lowering — handles
        // Type::Any / Substr / typed-Array uniformly.
        let default_stmt =
            self.emit_forof_default(var_name, var_type_ann, src, body, is_async, forin_obj);
        Ok(Some(wrap_prelude(prelude, default_stmt)))
    }

    /// Default for-of tail (P5.3 `Stmt::ForOf` emission; `for await`
    /// is carried by the `is_await` flag alone) — split from
    /// `try_parse_for_of` (2026-07-03, fn-debt decomp). Body verbatim;
    /// the two `Ok(Some(..))` tails become plain `Stmt`s.
    pub(super) fn emit_forof_default(
        &mut self,
        var_name: String,
        var_type_ann: Option<String>,
        src: ExprId,
        body: Stmt,
        is_async: bool,
        forin_obj: Option<ExprId>,
    ) -> Stmt {
        let id = self.mint_desugar_id();
        let i_name = format!("__forof_i_{id}");
        // src reuse rule: if src is already an Ident, no temp needed —
        // its binding stays the owner. Else hoist into a fresh
        // `__forof_src_<id>` let so the body's repeated reads see a
        // stable name (and ssa_lower can look up its type).
        let src_is_ident = matches!(self.ast.get_expr(src), Expr::Ident(_));
        let src_ident_name = if src_is_ident {
            if let Expr::Ident(n) = self.ast.get_expr(src) {
                n.clone()
            } else {
                unreachable!()
            }
        } else {
            format!("__forof_src_{id}")
        };
        // Pre-allocate elem_expr = src_ident[i_ident]. Hole Z — the
        // for-await element await is NOT desugared to a `.value`
        // Member here: the parser has no types, and `.value` conflates
        // the await unwrap with a real user member (a Struct element
        // without a `value` field died on member lookup). The
        // `is_await` flag on Stmt::ForOf carries the async form;
        // check_stmt_for_of unwraps Promise(T) → T by type and
        // ssa_lower_stmt_for_of routes Promise-typed elements through
        // promise_get_value — every non-thenable element awaits to
        // itself per §27.2.
        let src_ref_for_index = self.ast.add_expr(Expr::Ident(src_ident_name.clone()));
        let i_ref_for_index = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let elem_expr = self.ast.add_expr(Expr::Index {
            obj: src_ref_for_index,
            index: i_ref_for_index,
        });
        let forof_stmt = Stmt::ForOf {
            var_name,
            var_type_ann,
            src_ident: src_ident_name,
            i_ident: i_name,
            elem_expr,
            body: Box::new(body),
            forin_obj,
            is_await: is_async,
        };
        if src_is_ident {
            forof_stmt
        } else {
            // Hoist src into a fresh let, then ForOf reads it by name.
            let let_src = Stmt::LetDecl {
                mutable: false,
                name: format!("__forof_src_{id}"),
                type_ann: None,
                init: src,
                is_var: false,
            };
            Stmt::Block(vec![let_src, forof_stmt])
        }
    }
}

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
//! 2026-07-03 fn-debt decomp: destructuring scans + body wrapper →
//! `forof_destr.rs`; generator desugar + default ForOf tail split
//! into sub-fns below (bodies verbatim, dedented one level).

use super::*;

use super::forof_destr::ForOfPatScan;

impl<'a> Parser<'a> {
    pub(super) fn try_parse_for_of(&mut self, is_async: bool) -> Result<Option<Stmt>, String> {
        let saved = self.pos;
        if !matches!(self.peek(), Token::Let | Token::Const) {
            return Ok(None);
        }
        self.pos += 1;
        // V3-18 wedge — for-of with array-destructuring pattern:
        // `for (let [a, b] of pairs) { ... }`. Common shape for
        // iterating tuple arrays.
        let destruct_names: Option<Vec<String>> = match self.scan_forof_destr_array() {
            ForOfPatScan::Bail => {
                self.pos = saved;
                return Ok(None);
            }
            ForOfPatScan::NotPattern => None,
            ForOfPatScan::Pat(names) => Some(names),
        };
        // V3-18 wedge — for-of with object-destructuring pattern:
        // `for (let { x, y } of pts) { ... }`. Mirror of the array
        // destr branch: hoist the iterator variable into a fresh
        // synthetic name (`__forof_destr_<id>`), then prepend
        // per-field `let bound = <iter>.field` lets to the body.
        // Reserved-word fields go through keyword_property_name.
        // Bound binding name still required to be an Ident
        // (reserved-word fields require explicit `field: name`
        // rename — same rule as parse_object_destructuring).
        let destruct_obj: Option<Vec<(String, String)>> = if destruct_names.is_none() {
            match self.scan_forof_destr_obj() {
                ForOfPatScan::Bail => {
                    self.pos = saved;
                    return Ok(None);
                }
                ForOfPatScan::NotPattern => None,
                ForOfPatScan::Pat(entries) => Some(entries),
            }
        } else {
            None
        };
        let var_name = if destruct_names.is_some() || destruct_obj.is_some() {
            let id = self.mint_desugar_id();
            format!("__forof_destr_{id}")
        } else {
            match self.peek() {
                Token::Ident(n) => {
                    let nn = n.clone();
                    self.pos += 1;
                    nn
                }
                _ => {
                    self.pos = saved;
                    return Ok(None);
                }
            }
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
        let _ = have_type_ann; // not yet propagated; suppress unused warning
        let raw_src = self.parse_expr()?;
        // for-in wraps `Object.keys(raw_src)` so the body sees a
        // string[]. for-of uses raw_src directly.
        let src = if kind == "in" {
            let object_id = self.ast.add_expr(Expr::Ident("Object".into()));
            let keys_member = self.ast.add_expr(Expr::Member {
                obj: object_id,
                name: "keys".into(),
            });
            self.ast.add_expr(Expr::Call {
                callee: keys_member,
                args: vec![raw_src],
            })
        } else {
            raw_src
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
        let body = self.parse_stmt()?;
        // V3-18 wedge — prepend per-element / per-field destructuring
        // lets when the loop var was a pattern. The original `body` is
        // wrapped in a block so block-close drops still fire normally.
        let body = self.wrap_forof_destr_body(&destruct_names, &destruct_obj, &var_name, body);

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
            return Ok(Some(Stmt::ForOfSplitIter {
                var_name,
                parent: parent_id,
                sep: sep_id,
                body: Box::new(body),
            }));
        }

        // I.2 — for-of over a user iterable. Triggered when `kind == "of"`
        // and the source is a direct call to a known generator factory
        // (parser-tracked `function*` declarations). Desugars to a
        // next-loop using the iterator-protocol shape:
        //   { let __it = <gen-call>;
        //     while (true) {
        //       let __step = __it.next();
        //       if (__step.done) { break; }
        //       let v = __step.value;
        //       <body>
        //     } }
        // Handles `for (let v of gen())` directly. Limitation: a
        // captured iterator (`let g = gen(); for (let v of g)`) hits
        // the array branch — fix needs type info to dispatch.
        if kind == "of"
            && let Expr::Call { callee, .. } = self.ast.get_expr(src)
            && let Expr::Ident(callee_name) = self.ast.get_expr(*callee)
            && let Some(yield_ty) = self.generator_fns.get(callee_name).cloned()
        {
            let callee_name = callee_name.clone();
            return Ok(Some(self.desugar_forof_generator(
                &callee_name,
                yield_ty,
                src,
                var_name,
                body,
            )));
        }

        // P5.3 — default for-of emits Stmt::ForOf wrapped in a
        // Stmt::Block that hoists src to a fresh Ident binding (or
        // reuses the source Ident). ssa_lower dispatches on the
        // bound src's check-time type to pick array-walk vs iterator-
        // protocol. The pre-allocated `elem_expr = src_ident[i_ident]`
        // routes element loads through Expr::Index lowering — handles
        // Type::Any / Substr / typed-Array uniformly.
        Ok(Some(self.emit_forof_default(
            var_name,
            var_type_ann,
            src,
            body,
            is_async,
        )))
    }

    /// Generator-factory for-of desugar (I.2) — split from
    /// `try_parse_for_of` (2026-07-03, fn-debt decomp). Builds the
    /// iterator-protocol next-loop; body verbatim, dedented one
    /// level; tail `return Ok(Some(Stmt::Block(..)))` becomes the
    /// plain `Stmt::Block` return value.
    fn desugar_forof_generator(
        &mut self,
        callee_name: &str,
        yield_ty: String,
        src: ExprId,
        var_name: String,
        body: Stmt,
    ) -> Stmt {
        let gen_class = format!("__Gen_{callee_name}");
        let step_ty = format!("__step_{callee_name}");
        let id = self.mint_desugar_id();
        let it_name = format!("__forof_it_{id}");
        let step_name = format!("__forof_step_{id}");

        let mut stmts: Vec<Stmt> = Vec::new();
        // let __it: __Gen_<callee> = <gen-call>
        stmts.push(Stmt::LetDecl {
            mutable: false,
            name: it_name.clone(),
            type_ann: Some(gen_class),
            init: src,
            is_var: false,
        });

        // Inside while(true):
        //   let __step: __step_<callee> = __it.next();
        //   if (__step.done) { break; }
        //   let v: <yield_ty> = __step.value;
        //   <body>
        let it_ref = self.ast.add_expr(Expr::Ident(it_name.clone()));
        let next_member = self.ast.add_expr(Expr::Member {
            obj: it_ref,
            name: "next".into(),
        });
        let next_call = self.ast.add_expr(Expr::Call {
            callee: next_member,
            args: Vec::new(),
        });
        let step_decl = Stmt::LetDecl {
            mutable: false,
            name: step_name.clone(),
            type_ann: Some(step_ty),
            init: next_call,
            is_var: false,
        };

        let step_ref_done = self.ast.add_expr(Expr::Ident(step_name.clone()));
        let done_member = self.ast.add_expr(Expr::Member {
            obj: step_ref_done,
            name: "done".into(),
        });
        let done_check = Stmt::If {
            cond: done_member,
            then_branch: Box::new(Stmt::Break),
            else_branch: None,
        };

        let step_ref_value = self.ast.add_expr(Expr::Ident(step_name.clone()));
        let value_member = self.ast.add_expr(Expr::Member {
            obj: step_ref_value,
            name: "value".into(),
        });
        let var_decl = Stmt::LetDecl {
            mutable: false,
            name: var_name,
            type_ann: Some(yield_ty),
            init: value_member,
            is_var: false,
        };

        let loop_body = Stmt::Block(vec![step_decl, done_check, var_decl, body]);
        let true_lit = self.ast.add_expr(Expr::Bool(true));
        let while_loop = Stmt::While {
            cond: true_lit,
            body: Box::new(loop_body),
        };
        stmts.push(while_loop);
        Stmt::Block(stmts)
    }

    /// Default for-of tail (P5.3 `Stmt::ForOf` emission incl. the
    /// `for await` `.value` wrap) — split from `try_parse_for_of`
    /// (2026-07-03, fn-debt decomp). Body verbatim; the two
    /// `Ok(Some(..))` tails become plain `Stmt`s.
    fn emit_forof_default(
        &mut self,
        var_name: String,
        var_type_ann: Option<String>,
        src: ExprId,
        body: Stmt,
        is_async: bool,
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
        // Pre-allocate elem_expr = src_ident[i_ident]. P10.3-A1 — for
        // `for await`, wrap the element access in a `.value` Member
        // (the same desugar parser uses for `await e`). check.rs's
        // Promise<T>.value arm narrows the loop binding to T; ssa_lower's
        // P10.3-prereq whitelist (d2a7c61) routes Index-shaped Promise
        // sources to promise_get_value at the dispatch site.
        let src_ref_for_index = self.ast.add_expr(Expr::Ident(src_ident_name.clone()));
        let i_ref_for_index = self.ast.add_expr(Expr::Ident(i_name.clone()));
        let index_expr = self.ast.add_expr(Expr::Index {
            obj: src_ref_for_index,
            index: i_ref_for_index,
        });
        let elem_expr = if is_async {
            self.ast.add_expr(Expr::Member {
                obj: index_expr,
                name: "value".into(),
            })
        } else {
            index_expr
        };
        let forof_stmt = Stmt::ForOf {
            var_name,
            var_type_ann,
            src_ident: src_ident_name,
            i_ident: i_name,
            elem_expr,
            body: Box::new(body),
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

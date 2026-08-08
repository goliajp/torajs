//! `Checker::check_stmt` statement dispatcher (chunk 426).
//!
//! Extracted verbatim from check.rs — the per-statement typecheck
//! walk. Every arm is a thin delegate into its `check_stmt_*`
//! sibling module (see each arm's doc pointer); the structural
//! arms (Block / Multi / ExportDecl) recurse inline.
//!
//! Bodies unchanged except: the `Stmt::Block` fresh-scope walk is
//! extracted into `check_block` to keep `check_stmt` within the
//! 200-line function hard limit.

use super::*;

impl Checker {
    // CARVE-OUT: dispatch table — match-arm-per-Stmt-variant thin
    // delegation to per-shape sibling modules (1-8 lines each,
    // `ssa_lower_expr_inner::lower` posture); length comes from
    // variant count × per-arm doc + narrow-flush bracketing, not
    // logic. Splitting the match would destroy dispatch locality.
    pub(crate) fn check_stmt(&mut self, ast: &Ast, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(eid) => self.check_stmt_expr(ast, *eid),
            Stmt::Yield(_) | Stmt::YieldInto { .. } => {
                // Phase J — see [`crate::check_stmt_misc::check_yield`].
                crate::check_stmt_misc::check_yield(self);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                // V3-18 narrow wedge + CFG-aware moved snapshot +
                // post-if narrow on diverge. See
                // [`crate::check_stmt_if::check`].
                crate::check_stmt_if::check(self, ast, *cond, then_branch, else_branch);
                // ut3 — branch join: branch narrows die here.
                self.flush_assign_narrows();
            }
            Stmt::While { cond, body } => {
                // V3-18 narrow wedge (gated on no-reassign body).
                // See [`crate::check_stmt_while::check_while`].
                // ut3 — loop back-edge: no narrow crosses in
                // either direction (same for every loop arm below).
                self.flush_assign_narrows();
                crate::check_stmt_while::check_while(self, ast, *cond, body);
                self.flush_assign_narrows();
            }
            Stmt::DoWhile { body, cond } => {
                // Body runs first; cond typechecks after.
                // See [`crate::check_stmt_while::check_do_while`].
                self.flush_assign_narrows();
                crate::check_stmt_while::check_do_while(self, ast, body, *cond);
                self.flush_assign_narrows();
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                // Scrutinee type vs case value type; nested scope per
                // case body + default. See
                // [`crate::check_stmt_misc::check_switch`].
                crate::check_stmt_misc::check_switch(self, ast, *scrutinee, cases, default);
                self.flush_assign_narrows();
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                // Fresh-scope init / cond / step / body walker. See
                // [`crate::check_stmt_for::check`].
                self.flush_assign_narrows();
                crate::check_stmt_for::check(self, ast, init, cond, step, body);
                self.flush_assign_narrows();
            }
            Stmt::Throw(eid) => {
                // M4.3 + P7.2a + P4.7 — accept 8-byte-shaped
                // throw value; consume non-Copy via
                // consume_escape. See
                // [`crate::check_stmt_throw::check`].
                crate::check_stmt_throw::check(self, ast, *eid);
            }
            Stmt::Try {
                body,
                had_catch: _,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                // 3-phase walker (body / catch with P7.2b-2 Any
                // default / optional finally). See
                // [`crate::check_stmt_try::check`].
                crate::check_stmt_try::check(
                    self,
                    ast,
                    body,
                    catch_param,
                    catch_type,
                    catch_body,
                    finally_body,
                );
                self.flush_assign_narrows();
            }
            Stmt::Break(_) | Stmt::Continue(_) => {
                // No type-side state to track; the lowerer enforces that
                // these only appear inside loops (and that a label, if
                // present, names an enclosing statement).
            }
            Stmt::Labeled { body, .. } => {
                // ES §13.13 — the label is a control-flow name with no
                // type-side meaning; typecheck the labeled statement.
                self.check_stmt(ast, body);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                // P-iter — parent/sep typecheck + var_name Substr-
                // shaped String borrow per iteration. See
                // [`crate::check_stmt_for_of_split::check`].
                self.flush_assign_narrows();
                crate::check_stmt_for_of_split::check(self, ast, var_name, *parent, *sep, body);
                self.flush_assign_narrows();
            }
            // P5.3 — generic for-of; elem_expr/i_ident typing model
            // documented in [`crate::check_stmt_for_of`].
            Stmt::ForOf {
                var_name,
                var_type_ann,
                src_ident: _,
                i_ident,
                elem_expr,
                body,
                // chunk B3 — guard obj typing is a lowering concern
                // (the hoisted obj let is checked on its own decl).
                forin_obj: _,
                is_await,
            } => {
                // P5.3 generic for-of + P5.3 Phase B Struct skip +
                // P6.4c Map/Set/MapIter/ArrIter skip. See
                // [`crate::check_stmt_for_of::check`].
                self.flush_assign_narrows();
                crate::check_stmt_for_of::check(
                    self,
                    ast,
                    var_name,
                    var_type_ann,
                    i_ident,
                    *elem_expr,
                    body,
                    *is_await,
                );
                self.flush_assign_narrows();
            }
            Stmt::Block(stmts) => self.check_block(ast, stmts),
            Stmt::Multi(stmts) => self.check_multi(ast, stmts),
            // `is_var` is intentionally ignored here: `desugar_var_hoist`
            // runs before check and rewrites every `var` into a
            // hoisted `let`-shaped decl (is_var: false), so the
            // checker never observes a true `var` — not a silent-wrong,
            // var semantics are fully resolved upstream.
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var: _,
            } => {
                // M1.2 + P0.10 empty-array narrow + annotation
                // assignability + alias classify + M-OO.5 nominal
                // info + LocalInfo declare. See
                // [`crate::check_stmt_let_decl::check`].
                crate::check_stmt_let_decl::check(self, ast, *mutable, name, type_ann, *init);
            }
            // `desugar_using` (prelude) rewrites every UsingDecl into
            // try/finally + helper calls before check runs; one
            // reaching this arm means a pipeline skipped the pass.
            // Loud, not silent — checking it as a plain const would
            // drop the dispose semantics on the floor.
            Stmt::UsingDecl { name, .. } => {
                self.errors.push_err(format!(
                    "internal: `using {name}` survived desugar_using — pipeline is missing the pass"
                ));
            }
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                // M-OO.5 class context + fresh-scope body walker +
                // param declare with declared_class propagation. See
                // [`crate::check_stmt_fn_decl::check`].
                // ut3 — a fn body executes at arbitrary times;
                // no narrow crosses its boundary.
                self.flush_assign_narrows();
                crate::check_stmt_fn_decl::check(self, ast, name, params, body);
                self.flush_assign_narrows();
            }
            Stmt::TypeDecl { .. } => {
                // Already handled in pass 0; re-encountering it during the
                // body walk is a no-op. (No nested type decls — top-level
                // only — but the AST shape allows them anywhere.)
            }
            Stmt::Return(maybe_expr) => {
                // expected_return + Nullable<T> wedge +
                // P0.9 assignability lattice + move-out
                // (consume_escape for non-Copy). See
                // [`crate::check_stmt_return::check`].
                crate::check_stmt_return::check(self, ast, *maybe_expr);
            }
            // M5.1 — desugar_classes runs before check, so by the time we
            // walk the AST every ClassDecl has been split into a TypeDecl
            // + a series of FnDecls. Reaching here means the desugar pass
            // missed something — treat as an internal-error panic instead
            // of producing a bogus "type error".
            Stmt::ClassDecl { name, .. } => {
                panic!("internal: ClassDecl `{name}` reached check.rs (desugar didn't run?)");
            }
            Stmt::ImportDecl { .. } => {
                // K.1 single-file mode: import is parse-only, no
                // semantic effect. K.2 will add the cross-file symbol
                // table check here.
            }
            Stmt::ExportDecl { inner, .. } => self.check_export_decl(ast, inner.as_deref()),
        }
    }

    /// `Stmt::Multi` — surrounding scope shared (no push); walk each
    /// contained stmt in order.
    fn check_multi(&mut self, ast: &Ast, stmts: &[Stmt]) {
        for s in stmts {
            self.check_stmt(ast, s);
        }
    }

    /// `Stmt::ExportDecl` — K.1 single-file mode: export is the modifier
    /// wrapper; typecheck the wrapped declaration if any.
    fn check_export_decl(&mut self, ast: &Ast, inner: Option<&Stmt>) {
        if let Some(inner) = inner {
            self.check_stmt(ast, inner);
        }
    }
    /// `Stmt::Expr` arm — typecheck for effect, then mint a
    /// statement-level assignment narrow for `ident = value` shapes
    /// (execution is certain here, unlike expression-level assigns;
    /// see check_assign_narrow.rs).
    fn check_stmt_expr(&mut self, ast: &Ast, eid: ExprId) {
        match self.type_of(ast, eid) {
            Err(e) => self.errors.push_err(e),
            Ok(_) => {
                if let Expr::Assign { target, value } = ast.get_expr(eid)
                    && let Expr::Ident(n) = ast.get_expr(*target)
                    && let Some(vt) = self.expr_types.get(value).cloned()
                {
                    self.apply_assign_narrow(n, &vt);
                }
            }
        }
    }

    /// Fresh-scope statement-list walk for `Stmt::Block` — push a
    /// lexical scope, check each statement, pop.
    fn check_block(&mut self, ast: &Ast, stmts: &[Stmt]) {
        self.scopes.push(HashMap::new());
        let saved_hoists = crate::check_hoist_closure_lets::enter(self, ast, stmts);
        for s in stmts {
            self.check_stmt(ast, s);
        }
        self.hoisted_closure_lets = saved_hoists;
        self.scopes.pop();
    }
}

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
    pub(crate) fn check_stmt(&mut self, ast: &Ast, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(eid) => {
                if let Err(e) = self.type_of(ast, *eid) {
                    self.errors.push_err(e);
                }
            }
            Stmt::Yield(_) | Stmt::YieldInto { .. } => {
                // Phase J — desugar_generators rewrites generator
                // bodies before typecheck. See
                // [`crate::check_stmt_misc::check_yield`].
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
            }
            Stmt::While { cond, body } => {
                // V3-18 narrow wedge (gated on no-reassign body).
                // See [`crate::check_stmt_while::check_while`].
                crate::check_stmt_while::check_while(self, ast, *cond, body);
            }
            Stmt::DoWhile { body, cond } => {
                // Body runs first; cond typechecks after.
                // See [`crate::check_stmt_while::check_do_while`].
                crate::check_stmt_while::check_do_while(self, ast, body, *cond);
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
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                // Fresh-scope init / cond / step / body walker. See
                // [`crate::check_stmt_for::check`].
                crate::check_stmt_for::check(self, ast, init, cond, step, body);
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
            }
            Stmt::Break | Stmt::Continue => {
                // No type-side state to track; the lowerer enforces that
                // these only appear inside loops.
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
                crate::check_stmt_for_of_split::check(self, ast, var_name, *parent, *sep, body);
            }
            // P5.3 — generic for-of. The parser hoists src to a fresh
            // Ident and pre-builds `elem_expr = src[i]`. Typing the
            // var_name binding goes through Expr::Index lowering on
            // elem_expr — which already infers the right element type
            // per source shape (Array<T>.value=T, String[i]=String,
            // dynobj-backed Any[i]=Any). We also declare `i_ident` as
            // a Number local so the synthetic counter typechecks.
            //
            // P5.3 Phase B exception: when src has Type::Struct (i.e.
            // a class instance), the protocol path in ssa_lower
            // bypasses elem_expr entirely — typing `src[i]` here
            // would error ("can't index into Struct"). We probe src's
            // type first; if it's a class-shape Struct, defer the
            // element type to ssa_lower (mark as Any so var_name still
            // typechecks downstream as opaque).
            Stmt::ForOf {
                var_name,
                var_type_ann,
                src_ident: _,
                i_ident,
                elem_expr,
                body,
            } => {
                // P5.3 generic for-of + P5.3 Phase B Struct skip +
                // P6.4c Map/Set/MapIter/ArrIter skip. See
                // [`crate::check_stmt_for_of::check`].
                crate::check_stmt_for_of::check(
                    self,
                    ast,
                    var_name,
                    var_type_ann,
                    i_ident,
                    *elem_expr,
                    body,
                );
            }
            Stmt::Block(stmts) => self.check_block(ast, stmts),
            Stmt::Multi(stmts) => {
                // Surrounding scope shared — no push.
                for s in stmts {
                    self.check_stmt(ast, s);
                }
            }
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
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                // M-OO.5 class context + fresh-scope body walker +
                // param declare with declared_class propagation. See
                // [`crate::check_stmt_fn_decl::check`].
                crate::check_stmt_fn_decl::check(self, ast, name, params, body);
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
            Stmt::ExportDecl { inner, .. } => {
                // K.1 single-file mode: export is the modifier wrapper;
                // typecheck the wrapped declaration if any.
                if let Some(inner) = inner {
                    self.check_stmt(ast, inner);
                }
            }
        }
    }
    /// Fresh-scope statement-list walk for `Stmt::Block` — push a
    /// lexical scope, check each statement, pop.
    fn check_block(&mut self, ast: &Ast, stmts: &[Stmt]) {
        self.scopes.push(HashMap::new());
        for s in stmts {
            self.check_stmt(ast, s);
        }
        self.scopes.pop();
    }
}

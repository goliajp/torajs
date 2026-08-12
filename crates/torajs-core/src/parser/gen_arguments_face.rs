//! RFC 20260801-arguments-method-face — the `arguments` rename both
//! generator-method parsers perform, in one place.
//!
//! A generator method's body ends up owned by the state machine, where
//! a bare `arguments` would denote `next()`'s own arguments object
//! rather than the method's. Both halves — the class member
//! (`parse_class_decl_generator`, knife 2b) and the object-literal
//! shorthand (`object_member_generator`, knife 4d) — answered that the
//! same way and had carried byte-identical copies of the walk since.

use super::Parser;
use crate::ast::{Expr, GEN_ARGV_PARAM, Param, Stmt};

impl Parser<'_> {
    /// Rename every `arguments` ident the body minted — the arena
    /// range from `body_expr_start` on — to the trailing argv param,
    /// and report whether any did. A body that declares its own
    /// `arguments` binding, or a parameter list that names one, keeps
    /// it (sloppy shadow, pre-face semantics).
    pub(super) fn rename_gen_arguments_to_argv(
        &mut self,
        body: &[Stmt],
        params: &[Param],
        body_expr_start: usize,
    ) -> bool {
        let mut local_binds = std::collections::HashSet::new();
        crate::ast_collect_bindings::collect_local_binding_names(body, &mut local_binds);
        if local_binds.contains("arguments") || params.iter().any(|p| p.name == "arguments") {
            return false;
        }
        let mut used = false;
        for e in self.ast.exprs[body_expr_start..].iter_mut() {
            if matches!(e, Expr::Ident(n) if n == "arguments") {
                *e = Expr::Ident(GEN_ARGV_PARAM.into());
                used = true;
            }
        }
        used
    }
}

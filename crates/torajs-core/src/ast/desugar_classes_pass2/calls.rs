//! The method-call arm of `rewrite_expr_arena_pass2` —
//! `Expr::Call { callee: Member { obj, name } }` over a known method
//! name becomes the static `__cm_<C>__<M>` call, the `__dispatch_<M>`
//! chain dispatcher, or stays a Member call (sibling collision). Split
//! out of [`crate::ast::desugar_classes_pass2`] when K4 (static members of
//! a unit class) needed the `This` arm to grow (function-size limit).
//! Body verbatim; the four tables travel as one struct so the call
//! carries five arguments, not eight.

use crate::ast::desugar_classes_super::ClassIndexEntry;
use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// The pass-2 lookups the call rewrite reads.
pub(super) struct Pass2Tables<'a> {
    pub(super) class_index: &'a [ClassIndexEntry],
    pub(super) method_owners: &'a HashMap<String, Vec<String>>,
    pub(super) chain_methods: &'a HashSet<String>,
    /// `cm_demote::this_enclosing_classes` — the class each expr sits
    /// in, by expr index.
    pub(super) enclosing: &'a [Option<usize>],
}

/// Rewrite the call at arena index `i` (callee `callee_id`, arguments
/// `args_clone`) when its callee is a Member naming a known method.
pub(super) fn rewrite_method_call(
    ast: &mut Ast,
    i: usize,
    callee_id: ExprId,
    args_clone: Vec<ExprId>,
    t: &Pass2Tables<'_>,
) {
    // Look at what the callee is pointing at.
    if let Expr::Member { obj, name } = &ast.exprs[callee_id.0 as usize] {
        let m_name = name.clone();
        let obj_id = *obj;
        if let Some(owners) = t.method_owners.get(&m_name) {
            // Three cases:
            // (1) Single owner — keep static dispatch via
            //     `__cm_<C>__<M>`, EXCEPT when the receiver
            //     is `this.<field>` and the field is typed
            //     as a known builtin (Array `T[]`, `string`,
            //     `number`). Those calls dispatch to the
            //     intrinsic, not the user class's method
            //     — without the guard, `class C { data:
            //     T[]; push(v) { this.data.push(v); } }`
            //     would rewrite the inner `this.data.push`
            //     to `__cm_C__push(this.data, v)` and
            //     infinite-recurse.
            // (2) Multi-owner forming a single inheritance
            //     chain (override case) — route through
            //     `__dispatch_<M>` runtime-tag dispatcher.
            // (3) Multi-owner across unrelated hierarchies
            //     (sibling collision) — leave Member as-is.
            if owners.len() == 1 {
                let enclosing_cname = t
                    .enclosing
                    .get(obj_id.0 as usize)
                    .copied()
                    .flatten()
                    .map(|ci| t.class_index[ci].1.as_str());
                let skip_for_builtin_field = crate::cm_demote::receiver_is_this_builtin_field(
                    ast,
                    obj_id,
                    owners[0].as_str(),
                    enclosing_cname,
                    t.class_index,
                );
                if skip_for_builtin_field {
                    // Leave Member; ssa_lower picks the
                    // builtin intrinsic from the field's
                    // actual type at SSA time.
                } else {
                    crate::cm_demote::record_speculative_rewrite(
                        ast,
                        i,
                        callee_id,
                        obj_id,
                        &args_clone,
                    );
                    let mangled = format!("__cm_{}__{m_name}", owners[0]);
                    let new_callee = ast.add_expr(Expr::Ident(mangled));
                    let mut new_args = Vec::with_capacity(args_clone.len() + 1);
                    new_args.push(obj_id);
                    new_args.extend(args_clone);
                    ast.exprs[i] = Expr::Call {
                        callee: new_callee,
                        args: new_args,
                    };
                }
            } else if t.chain_methods.contains(&m_name) {
                crate::cm_demote::record_speculative_rewrite(
                    ast,
                    i,
                    callee_id,
                    obj_id,
                    &args_clone,
                );
                let mangled = format!("__dispatch_{m_name}");
                let new_callee = ast.add_expr(Expr::Ident(mangled));
                let mut new_args = Vec::with_capacity(args_clone.len() + 1);
                new_args.push(obj_id);
                new_args.extend(args_clone);
                ast.exprs[i] = Expr::Call {
                    callee: new_callee,
                    args: new_args,
                };
            }
            // else: sibling collision — leave Member call AS-IS.
        }
    }
}

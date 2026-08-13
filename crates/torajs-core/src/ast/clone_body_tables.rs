//! Side-table migration for [`super::clone_body::BodyCloner`] (RFC
//! 20260804-method-rebind-generic-body blade 1).
//!
//! Every ExprId-keyed side table on [`Ast`] gets its hits copied to
//! the cloned ids. Tables filled by later pipeline stages are empty
//! (or hold no cloned-domain ids) at the clone point — copying is a
//! no-op there, so the walk covers ALL ExprId-keyed tables rather
//! than a hand-picked "filled by now" subset that would rot when a
//! new parser-stage table lands.
//!
//! ExprId-VALUED entries (`speculative_cm_rewrites`,
//! `objlit_computed_keys`): a value inside the cloned domain follows
//! the mapping; a value outside it is deep-cloned too — sharing one
//! id across two bodies is exactly the collision this module exists
//! to prevent. Those run FIRST: the deep clone extends the mapping,
//! and the keyed-copy pass snapshots pairs after it so entries on a
//! freshly-cloned value subtree migrate in the same call.

use super::clone_body::BodyCloner;
use crate::ast::ExprId;

pub(crate) fn migrate(cloner: &mut BodyCloner<'_>) {
    // ExprId-valued tables first — may extend cloner.map.
    let seed: Vec<(ExprId, ExprId)> = cloner.map.iter().map(|(o, n)| (*o, *n)).collect();
    for (old, new) in &seed {
        if let Some(v) = cloner.ast.speculative_cm_rewrites.get(old).copied() {
            let v2 = resolve_value(cloner, v);
            cloner.ast.speculative_cm_rewrites.insert(*new, v2);
        }
        if let Some(v) = cloner.ast.objlit_computed_keys.get(old).copied() {
            let v2 = resolve_value(cloner, v);
            cloner.ast.objlit_computed_keys.insert(*new, v2);
        }
        if let Some(v) = cloner.ast.cm_this_static_calls.get(old).copied() {
            let v2 = resolve_value(cloner, v);
            cloner.ast.cm_this_static_calls.insert(*new, v2);
        }
    }

    let pairs: Vec<(ExprId, ExprId)> = cloner.map.iter().map(|(o, n)| (*o, *n)).collect();

    macro_rules! copy_map {
        ($table:ident) => {
            for (old, new) in &pairs {
                if let Some(v) = cloner.ast.$table.get(old).cloned() {
                    cloner.ast.$table.insert(*new, v);
                }
            }
        };
    }
    macro_rules! copy_set {
        ($table:ident) => {
            for (old, new) in &pairs {
                if cloner.ast.$table.contains(old) {
                    cloner.ast.$table.insert(*new);
                }
            }
        };
    }

    copy_map!(fn_expr_self_names);
    copy_map!(dstr_default_names);
    copy_map!(ary_destr_groups);
    copy_map!(iter_destr_srcs);
    copy_map!(undeclared_reads);
    copy_set!(self_name_writes);
    copy_map!(call_type_args);
    copy_map!(regex_parse_errors);
    copy_map!(objlit_computed_accessors);
    copy_map!(gen_fn_exprs);
    copy_set!(objlit_method_exprs);
    copy_set!(objlit_shorthand_proto_exprs);
    copy_set!(objlit_cover_init_exprs);
    copy_set!(arrlit_trailing_comma_after_rest);
    copy_set!(dstr_default_member_loads);
    copy_set!(await_value_reads);
    copy_set!(fn_expr_exprs);
    copy_set!(template_str_calls);
    // Its neighbour above marks the same `String(sub)` node from the
    // other side, and the two have to travel together: `desugar_with`
    // clones a value expression per guard arm, so a NESTED `with` sees
    // the clone rather than the original. Without this the clone is
    // just a name spelled `String`, and the outer object answers it.
    copy_set!(synth_global_refs);
    copy_set!(fnexpr_recv_faces);
    copy_set!(async_fn_value_exprs);
    copy_set!(stack_array_literals);
}

fn resolve_value(cloner: &mut BodyCloner<'_>, v: ExprId) -> ExprId {
    if let Some(mapped) = cloner.map.get(&v) {
        *mapped
    } else {
        cloner.clone_expr(v)
    }
}

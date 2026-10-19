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
    // Clone order (new ids are minted in sequence), not hash order:
    // `resolve_value` may deep-clone, and those new exprs land in this order.
    let mut seed: Vec<(ExprId, ExprId)> = cloner.map.iter().map(|(o, n)| (*o, *n)).collect();
    seed.sort_unstable_by_key(|(_, n)| n.0);
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

    let mut pairs: Vec<(ExprId, ExprId)> = cloner.map.iter().map(|(o, n)| (*o, *n)).collect();
    pairs.sort_unstable_by_key(|(_, n)| n.0);

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
        ($($table:ident).+) => {
            for (old, new) in &pairs {
                if cloner.ast.$($table).+.contains(old) {
                    cloner.ast.$($table).+.insert(*new);
                }
            }
        };
    }

    copy_map!(fn_expr_self_names);
    copy_map!(dstr_default_names);
    copy_map!(ary_destr_groups);
    copy_map!(iter_destr_srcs);
    copy_set!(dstr_deferred_rest);
    copy_map!(undeclared_reads);
    copy_set!(module_foreign_reads);
    copy_set!(self_name_writes);
    copy_set!(const_writes);
    copy_set!(tdz_reads);
    copy_set!(with_fallthrough_idents);
    copy_set!(spliced_uninit_inits);
    copy_map!(call_type_args);
    copy_map!(regex_parse_errors);
    copy_map!(objlit_computed_accessors);
    copy_map!(gen_fn_exprs);
    copy_set!(objlit_method_exprs);
    copy_set!(objlit_shorthand_proto_exprs);
    copy_set!(objlit_cover_init_exprs);
    copy_set!(objlit_dup_proto_exprs);
    copy_set!(arrlit_trailing_comma_after_rest);
    copy_set!(dstr_default_member_loads);
    copy_set!(index_spelled_props);
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
    copy_set!(fnexpr_recv_foreign_uses);
    copy_set!(fn_shadowed_new);
    copy_set!(fnexpr_bind_local_inits);
    copy_set!(async_fn_value_exprs);
    copy_set!(async_sm_value_exprs);
    copy_set!(async_ssa.value_exprs);
    copy_set!(async_ssa.return_calls);
    copy_set!(await_yield_values);
    // The value names another expr of the same body: a cloned origin
    // gets the clone's id, one outside the clone keeps its own.
    for (old, new) in &pairs {
        if let Some(origin) = cloner.ast.yield_into_await_origins.get(old).copied() {
            let origin = cloner.map.get(&origin).copied().unwrap_or(origin);
            cloner.ast.yield_into_await_origins.insert(*new, origin);
        }
    }
    // An optional call keeps its chain base, a node under the call: the
    // cloned call is guarded by the cloned base.
    for (old, new) in &pairs {
        if let Some(base) = cloner.ast.optchain_calls.get(old).copied() {
            let base = cloner.map.get(&base).copied().unwrap_or(base);
            cloner.ast.optchain_calls.insert(*new, base);
        }
    }
    copy_set!(stack_array_literals);
    copy_set!(paren_optchains);
    copy_set!(paren_optchain_calls);
    copy_set!(regex_result_props_unread);
    copy_map!(regex_exec_record_callees);
    copy_map!(eval_var_sites);
    copy_set!(escaped_use_strict);
    copy_set!(new_target_fn_exprs);
    copy_set!(implicit_rest_arrows);
    copy_set!(param_destr_inits);
    copy_set!(spread_filled_args);
}

fn resolve_value(cloner: &mut BodyCloner<'_>, v: ExprId) -> ExprId {
    if let Some(mapped) = cloner.map.get(&v) {
        *mapped
    } else {
        cloner.clone_expr(v)
    }
}

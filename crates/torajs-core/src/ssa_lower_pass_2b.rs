//! Pass 2B (T-15.g.5) of `lower_inner` — lower lifted-closure bodies.
//! Deferred until after Pass 3 main-synth so top-level construction
//! sites (`let cb = function(v) { ... }` at module scope) have
//! populated `closure_captures`. Closures still lower in reverse
//! append order among themselves so an outer closure's body (which
//! constructs the inner closure) runs before the inner closure's
//! body.
//!
//! Extracted from `lower_inner` (chunk-335 RFC continuation, after
//! the Pass 2.5 / Pass 3 siblings in chunks 332 + 334). Pure
//! mechanical move: substrate codegen invariant.

use crate::ast::PropKey;
use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::num_width::WidthTable;
use crate::ssa::{self, BakedRegexEntry, FnNameEntry, FuncId, Module, Type};
use crate::ssa_lower::Intrinsics;

/// Chunk 720 — ES §8.4.5 NamedEvaluation for lifted arrows: an
/// anonymous function definition takes its name from the syntactic
/// position it sits in. The lift pass (`lift_arrow_fns`) runs
/// per-expression with no statement context, so the binding is
/// recovered here. Returns `__closure_N` → binding-name.
///
/// The positions themselves (declaration binder, assignment Ident
/// target incl. the chained `f = g = () => {}` form, object-literal
/// property key) come from `ast::named_eval` — shared with the
/// generator-expression hoist, which needs the same answer for slots
/// it erases before this pass ever runs. Default-export and
/// class-field initializers stay recorded follow-ups.
fn collect_named_eval(ast: &Ast) -> HashMap<String, String> {
    // The position walk itself is shared with `hoist_gen_fn_exprs`
    // (RFC 20260729-fn-value-any V4 刀 2) — see `ast::named_eval`.
    // Here it is filtered down to the lifted-closure nodes: every
    // other slot the walk recorded belongs to some non-fn value.
    let mut map = HashMap::new();
    for (eid, binding) in crate::ast::collect_named_eval_positions(ast) {
        if let Expr::Closure { fn_name, .. } = ast.get_expr(eid)
            && fn_name.starts_with("__closure_")
        {
            map.insert(fn_name.clone(), binding);
        }
    }
    // RFC 20260714-dstr-residual blade 2 — destructuring-default
    // position (§8.4.5): the desugar buries the anonymous fn inside
    // a ternary the statement walk can't see; the parser recorded
    // the binding name by ExprId and lift_arrow_fns translated it
    // to the lifted closure name.
    for (cn, bn) in &ast.closure_dstr_names {
        map.insert(cn.clone(), bn.clone());
    }
    // Chunk 796 — a named function expression's self-name wins over
    // every syntactic-position name (ES §15.5.5: NamedEvaluation
    // applies only to ANONYMOUS definitions), so overlay it last.
    for (cn, sn) in &ast.closure_self_names {
        map.insert(cn.clone(), sn.clone());
    }
    map
}

/// Definition shell for a dead orphan closure (see the gate in
/// [`run`]): mirrors the pass-1 signature so every synthesized
/// reference (boxed-entry adapter, fn-addr tables) links, and
/// terminates with `unreachable` — no construction site exists, so
/// no fn value can ever carry its address and the body cannot run.
fn synth_dead_orphan_stub(
    name: &str,
    fid: FuncId,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    fn_sigs: &[(Vec<Type>, Type)],
    signatures: &HashMap<FuncId, Type>,
) -> ssa::Function {
    let ret_ty = signatures.get(&fid).copied().unwrap_or(Type::Void);
    let mut f = ssa::Function::new(name, ret_ty);
    if let Some(sid) = fn_sig_ids.get(&fid) {
        for (i, ty) in fn_sigs[sid.0 as usize].0.iter().enumerate() {
            f.add_param(*ty, &format!("p{i}"));
        }
    }
    let entry = f.add_block();
    f.set_term(entry, crate::ssa::Terminator::Unreachable);
    f
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    closure_decls: Vec<(usize, FuncId)>,
    ast: &Ast,
    module: &mut Module,
    fn_table: &HashMap<String, FuncId>,
    signatures: &HashMap<FuncId, Type>,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    fn_dflt_lits: &crate::ssa_lower_boxed_entry::FnDfltLits,
    intrinsics: &Intrinsics,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(PropKey, String)>)>,
    closure_captures: &mut HashMap<String, Vec<(String, Type, bool)>>,
    closure_variadic_captures: &mut HashMap<String, Vec<String>>,
    call_retargets: &HashMap<ExprId, String>,
    may_throw: &std::collections::HashSet<String>,
    class_name_to_tag: &HashMap<String, u32>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    globals: &HashMap<String, Type>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<ExprId, usize>,
    contextual_any: &std::collections::HashSet<ExprId>,
    num_f64_slots: &WidthTable,
    promise_thunks: &crate::ssa_lower_promise_thunk::PromiseThunks,
    boxed_entries: &HashMap<FuncId, (FuncId, ssa::SigId)>,
) {
    let named_eval = collect_named_eval(ast);
    for (stmt_idx, fid) in closure_decls {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            span,
            ..
        } = &ast.stmts[stmt_idx]
        {
            // A capturing closure with NO capture side-channel entry
            // by now is dead code: 2B runs after every other body
            // (pass 2A user fns, pass 3 main) and its own reverse
            // append order puts enclosing bodies first, so no
            // construction site exists anywhere in the program and
            // the fn value can never come to exist. Defence in
            // depth: the module channel that used to strand such
            // orphans (a discarded lib copy's fn literal riding the
            // whole-arena lift) is closed at the source by the
            // resolver's dead-copy sweep (`modules/dead_lib_exprs`),
            // but any OTHER channel that materializes a dead expr —
            // the generic-twin clone lane guards its own with
            // `neutralize_clone` — still lands here instead of
            // panicking on the env preamble. The pass-1 shell stays
            // an empty Function no call site can reach.
            // 551-03 — an any-widened clone of a lifted body is never
            // constructed; it is live through its retargeted call and
            // its layout is the original's (`widen_clone_origin`).
            let layout_of = ast
                .widen_clone_origin
                .get(name.as_str())
                .map_or(name.as_str(), String::as_str);
            let dead_orphan = params.first().is_some_and(|p| {
                p.name == "__env"
                    && p.type_ann
                        .as_deref()
                        .and_then(crate::ssa_lower_free_helpers::decode_env_ann)
                        .is_some_and(|caps| !caps.is_empty())
            }) && !closure_captures.contains_key(layout_of);
            if dead_orphan {
                // The definition must still exist — the boxed-entry
                // adapter (synthesized for every lifted closure)
                // calls the body by FuncId, so an empty shell leaves
                // an unresolved extern at link time.
                module.funcs[fid.0 as usize] =
                    synth_dead_orphan_stub(name, fid, fn_sig_ids, fn_sigs, signatures);
                continue;
            }
            let string_id_base = module.strings.len();
            let (f, new_strings) = crate::ssa_lower_fn::lower_fn(
                name,
                params,
                return_type.as_deref(),
                body,
                ast,
                fn_table,
                signatures,
                fn_sig_ids,
                fn_dflt_lits,
                intrinsics,
                aliases,
                arr_layouts,
                baked_regex_buf,
                fn_sigs,
                struct_layouts,
                inst_memo,
                generic_struct_decls,
                string_id_base,
                closure_captures,
                closure_variadic_captures,
                call_retargets,
                may_throw,
                class_name_to_tag,
                anon_stamp_pool,
                globals,
                expr_types,
                arity_pad_count,
                contextual_any,
                num_f64_slots,
                promise_thunks,
                boxed_entries,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
            // Chunk 720 — NamedEvaluation registry row: a let-bound
            // arrow answers its binding name / arity through the
            // fn-addr registry (`.name` / `.length` / fn-print),
            // mirroring the Pass 2 fn-decl rows in `body_passes`.
            // The link layer sorts the table by fn_addr, so the
            // late push order is immaterial.
            //
            // B3c — closures WITHOUT a NamedEvaluation binding
            // (anonymous arrows / fn exprs in argument or element
            // position) now land a row too, with an empty name: the
            // runtime print faces read `name_len == 0` as the
            // `[Function]` anonymous form (unchanged behavior) while
            // `src_ptr` carries the erased source for toString (B4)
            // and `.length` answers the true arity.
            let binding = named_eval.get(name).map(String::as_str).unwrap_or("");
            let lit = ssa::StringLiteral::encode_from_str(binding);
            let name_sid = ssa::StringId(module.strings.len() as u32);
            module.strings.push(lit);
            // ES-spec `Function.length` — leading params before
            // the first default / rest (§10.2.10), synthetic
            // `__env` excluded (chunk 716 contract).
            //
            // `__this` is synthetic too: it is the receiver channel a
            // promoted fn-expr / object-literal method got, not a
            // parameter the user wrote, so §10.2.10 must not count it
            // either. The class-method row already filters both names
            // (`body_passes`); this row kept only `__env`, so every
            // promoted face read one too high — `{ f: function (n)
            // { this.n = n } }.f.length` answered 2, and an
            // object-literal getter answered 1 where the spec says 0.
            let arity = params
                .iter()
                .filter(|p| p.name != "__env" && p.name != "__this")
                .take_while(|p| p.default.is_none() && !p.is_rest)
                .count() as u32;
            // B3b — lifted closure decls carry the user arrow /
            // fn-expr span (B1b), so the same erased-source
            // intern the Pass 2 rows get applies here.
            let (src_sid, src_len) = crate::ssa_lower_inner::intern_fn_source(module, ast, *span);
            module.fn_name_globals.push(FnNameEntry {
                fn_id: fid,
                name: crate::ast::PropKey::from(binding),
                name_sid,
                arity,
                src_sid,
                src_len,
            });
        }
    }
}

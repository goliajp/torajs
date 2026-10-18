//! The adapter-synthesis TARGETS of
//! [`super::synthesize_boxed_entries`] — which FnDecls get a boxed
//! dual-entry adapter, and the per-fn facts the build reads. Split
//! from the parent (rotation 626) when the relay redirect's
//! retarget table pushed both the file and the collect loop past
//! their limits; the seam is the parent's own: admission (which
//! head shapes and factories qualify) in the loop, the facts of one
//! admitted fn in [`target_facts`], the build in `build`.

use std::collections::HashMap;

use super::admit::{factory_always_admitted, program_constructs_from_value, promise_heir_classes};
use super::build_rest::rest_slot_kind;
use super::{DfltLit, MAX_BOXED_PARAMS, relay};
use crate::ast::{Ast, Stmt};
use crate::ssa::{self, FuncId, Type};

mod head;
use head::head_shape;

/// The module tables the targets loop reads, bundled once per
/// synthesis so the loop and [`target_facts`] share one carrier.
pub(super) struct TargetTables<'a> {
    pub(super) ast: &'a Ast,
    pub(super) call_retargets: &'a crate::ssa_lower::CallRetargets,
    pub(super) fn_table: &'a HashMap<String, FuncId>,
    pub(super) fn_sigs: &'a [(Vec<Type>, Type)],
    pub(super) fn_sig_ids: &'a HashMap<FuncId, ssa::SigId>,
    pub(super) arr_layouts: &'a crate::ssa::ArrLayouts,
}

/// `true` iff the type can cross the boxed boundary in either
/// direction with the intrinsics above. `Substr` params stay
/// unboxable (the layout differs from the owned Str `anyv_to_str`
/// yields — entry stays 0, catchable TypeError).
fn boxable(ty: &Type) -> bool {
    !matches!(ty, Type::Substr)
        && (matches!(
            ty,
            Type::Any | Type::I64 | Type::I32 | Type::F64 | Type::Bool | Type::Ptr
        ) || ty.is_refcounted())
}

/// The per-FnDecl classification walk — which bodies get an adapter
/// and with what argv-window shape (env/this head, static / factory
/// head-less forms, argc/argv synthetic slots, the knife-2 receiver
/// slot, S2.39 literal defaults). Carved out of
/// [`synthesize_boxed_entries`] when the B5 predicate arm pushed it
/// past the 200-line hard limit — verbatim move.
pub(super) fn collect_boxed_targets(tables: &TargetTables<'_>) -> Vec<BoxedEntryTarget> {
    let ast = tables.ast;
    let constructs_from_value = program_constructs_from_value(ast);
    let promise_heirs = promise_heir_classes(ast);
    let mut targets: Vec<BoxedEntryTarget> = Vec::new();
    for stmt in &ast.stmts {
        let Stmt::FnDecl { name, params, .. } = stmt else {
            continue;
        };
        // Lifted closure bodies carry `__env` first; class-method
        // bodies (`__cm_<C>__<m>`) carry `__this` first — 刀 4 (RFC
        // 20260714-t262-top-clusters) reifies those too, so an
        // any-held class instance can dispatch its methods by name
        // through the class-methods table. Both are pointer-shaped
        // first params the adapter feeds its env argument into.
        let (first_is_env, first_is_this, is_twin) = head_shape(name, params);
        // RFC 20260717-class-first-class-value knife B cut 2 — static
        // method bodies (`__sm_<C>__<m>`) have NO env/this head param;
        // their adapter maps argv straight onto the user params and
        // drops its env argument (static dispatch carries no
        // receiver).
        let is_static = name.starts_with("__sm_");
        // S-NEW 刀 2 — the `__new_<C>` factories, same head-less shape
        // as a static method: their params ARE the constructor's, so
        // argv maps straight onto them. `__torajs_anyv_construct`
        // reaches a class object's factory through this adapter. A
        // Promise-heir class's factory is admitted unconditionally:
        // the inherited settle statics run NewPromiseCapability(C)
        // through the construct channel at runtime (rotation 451),
        // and no syntactic probe can see that consumer.
        // A species-family heir (`extends Array` / a TypedArray
        // kind) is admitted the same way: the §23.1.2.5 / §23.2.2.4
        // inherited `@@species` construct runs inside the family
        // methods at runtime, and no syntactic probe can see that
        // consumer either.
        let is_factory = name.starts_with("__new_")
            && (constructs_from_value
                || factory_always_admitted(ast, &promise_heirs, {
                    let c = &name["__new_".len()..];
                    c.strip_suffix("$$anywv")
                        .or_else(|| {
                            crate::check_monomorph_any_widen::struct_escape::twin_original(c)
                        })
                        .unwrap_or(c)
                }));
        // the replace lane's view twin is entered only by that lane's
        // direct call: no fn value carries it, so no adapter
        if (!first_is_env && !first_is_this && !is_static && !is_factory && !is_twin)
            || ast.is_replace_view_fn(name)
        {
            continue;
        }
        if let Some(t) = target_facts(tables, name, params, (first_is_env, first_is_this)) {
            targets.push(t);
        }
    }
    targets
}

/// The SSA signature slots ahead of the user parameters that the AST
/// param list does not spell. Two skip counts since RFC
/// 20260810-indirect-argc-abi S1: the AST param list has no hidden
/// slot, the SSA sig of an `__env`-first body has the injected I64
/// `__torajs_argc` at position 1 — cutting both sides with one count
/// was the exact AST↔sig mis-align this split exists to prevent.
/// S1-T1 — the this-first method_argv family carries the same
/// sig-only slot (shared predicate: pass 1 / setup_fn_params / here),
/// and a head-less body (a class factory carrying its constructor's
/// channel, `arguments_object_ctor_factory`) has it at 0.
fn sig_only_slots(
    ast: &Ast,
    name: &str,
    params: &[crate::ast::Param],
    first_is_env: bool,
    feeds_env: bool,
) -> usize {
    let this_hidden =
        crate::ssa_lower_pass_1::this_first_hidden_argc_pos(ast, name, params).is_some();
    let headless_hidden = !feeds_env
        && !this_hidden
        && crate::ssa_lower_pass_1::hidden_argc_pos(ast, name, params) == Some(0);
    usize::from(first_is_env) + usize::from(this_hidden) + usize::from(headless_hidden)
}

/// The per-FnDecl facts of one admitted target — everything the
/// adapter build reads. `None` refuses the fn (a shape the adapter
/// cannot serve: an unboxable param, a nested-typed rest, a sig the
/// module never registered).
fn target_facts(
    tables: &TargetTables<'_>,
    name: &str,
    params: &[crate::ast::Param],
    (first_is_env, first_is_this): (bool, bool),
) -> Option<BoxedEntryTarget> {
    let TargetTables {
        ast,
        call_retargets,
        fn_table,
        fn_sigs,
        fn_sig_ids,
        arr_layouts,
    } = *tables;
    let feeds_env = first_is_env || first_is_this;
    let &fid = fn_table.get(name)?;
    let &sig_id = fn_sig_ids.get(&fid)?;
    let (param_tys, ret_ty) = fn_sigs[sig_id.0 as usize].clone();
    let env_count = usize::from(feeds_env);
    // RFC 20260708-closure-argv-face — a full-arguments body
    // carries the raw argv pointer right after its head param
    // (the argc channel is the S1 hidden sig slot on every tier
    // since the T/H blades — no injected argc param remains).
    // A twin on the this-first argv lane (`__this` then
    // `__torajs_argv`, rotation 627): its sig is `[__this: Any, argc:
    // I64, argv: Ptr, …user]`; the receiver rides argv[0] like any
    // recv-first cell and is user param 0 of the body — fed from
    // argv[0] BEFORE the hidden slots, the window then shifted past
    // it for the rest (`recv_leads`).
    let twin_head = !feeds_env && params.first().is_some_and(|p| p.name == "__this");
    let twin_argv = twin_head && params.get(1).is_some_and(|p| p.name == "__torajs_argv");
    // A twin on the argc tier alone has the same receiver-first shape
    // without the pointer, `[__this: Any, argc: I64, …user]`; mapped
    // as a plain head its argc slot read argv[0], and a static method
    // reached through `any` answered `arguments.length` 0.
    let twin_lane =
        twin_argv || twin_head && ast.this_first_argc_fns.get(name).is_some_and(|&h| h == 1);
    let has_argv = twin_argv
        || params
            .get(env_count)
            .is_some_and(|p| p.name == "__torajs_argv");
    // param 0 is the env Ptr (when the body carries one); the
    // boxed surface covers the rest (minus the argv slot, which
    // never consumes argv positions)
    let ast_skip = env_count + usize::from(has_argv);
    let hidden = sig_only_slots(ast, name, params, first_is_env, feeds_env);
    let sig_skip = ast_skip + hidden;
    let user_tys: Vec<Type> = if twin_lane {
        std::iter::once(param_tys[0])
            .chain(param_tys[2 + usize::from(twin_argv)..].iter().cloned())
            .collect()
    } else {
        param_tys[sig_skip..].to_vec()
    };
    if user_tys.len() > MAX_BOXED_PARAMS {
        return None;
    }
    let (rest, rest_kind) = rest_slot_kind(params, &user_tys, arr_layouts)?;
    // a fn-typed slot crosses only to the all-`any` twin (`build`)
    let crosses = |t: &Type| boxable(t) || matches!(t, Type::FnSig(_));
    if !user_tys.iter().all(crosses) || !(ret_ty == Type::Void || boxable(&ret_ty)) {
        return None;
    }
    // RFC 20260823-proxy-substrate 刀 4b — a TYPED array return
    // crossing into the any world needs its element-kind chain
    // stamped, exactly like a typed array argument does. Without
    // it the boxed value is an array whose every slot the any
    // lane decodes as a NaN box: `const g: any = () => ["x"];
    // g()[0]` answered `undefined` and `JSON.stringify` answered
    // `[null]`, silently, for every function that returns one.
    let ret_arr_kind = match &ret_ty {
        Type::Arr(id) => {
            let elem = arr_layouts[id.0 as usize];
            let chain = crate::ssa_lower_arr_kind_mark::arr_kind_chain_of(arr_layouts, &elem, 0);
            (chain != 0).then_some(chain as i64)
        }
        _ => None,
    };
    // S2.39 — literal defaults on the user params, positionally
    // aligned with `user_tys`. The adapter fills these when an
    // argv slot arrives undefined (missing OR explicit — ES
    // §10.2.1.3 treats both alike), which a runtime call cannot
    // get from the caller-side default injection. Qualifying
    // literals per [`DfltLit::of_default_expr`].
    let dflt_params: Vec<&crate::ast::Param> = if twin_lane {
        params
            .iter()
            .filter(|p| p.name != "__torajs_argv")
            .collect()
    } else {
        params[ast_skip..].iter().collect()
    };
    let dflt_lits: Vec<Option<DfltLit>> = dflt_params
        .iter()
        .map(|p| {
            p.default
                .and_then(|d| DfltLit::of_default_expr(ast.get_expr(d)))
        })
        .collect();
    // RFC 20260808 knife 2 — a promoted `__this` param sitting
    // AFTER the (possibly empty) argc/argv slot run is a
    // recv-first body's receiver channel: every dispatch to a
    // recv-first cell prepends the receiver in argv[0], so the
    // count those slots describe drops by one (the receiver is
    // not an argument, §10.4.4). Pre-fix the materialized
    // `arguments` counted the bound this: length answered n+1
    // and arguments[0] read the receiver. S3.7 — the predicate
    // keys on the `__this` shape itself, not on the injected
    // slots: since S3.4 an env-first body carries NO injected
    // argc slot, but its S1 hidden argc still needs the same
    // receiver correction (test262 bind/15.3.4.5.1-4-6 answered
    // arguments.length === 1 when the slot-keyed predicate went
    // false). A `__cm_` this-first head never reaches ast_skip
    // (env_count consumes it) and twins are not env-first.
    let recv =
        twin_lane || first_is_env && params.get(ast_skip).is_some_and(|p| p.name == "__this");
    // A function expression's construct face carries its new.target
    // right after the receiver (`check_monomorph_ctor_face`), and it is
    // no argument either: `arguments.length` counted it.
    let nt_face = recv
        && params
            .get(ast_skip + 1)
            .is_some_and(|p| p.name == crate::ast::new_target_param::NEW_TARGET);
    let recv_slots = usize::from(recv) + usize::from(nt_face);
    // The relay redirect (module doc in `relay`): the adapter of
    // a `__forward_<t>` / `__fwdrecv_<t>` enters `t` with its own
    // argc / argv. A receiver-first relay composes like a twin on
    // the this-first argv lane: argv[0] feeds `__this`, the rest
    // comes off the shifted window (`recv_leads`).
    let relay = (first_is_env && !rest)
        .then(|| {
            relay::headless_target(
                ast,
                call_retargets,
                name,
                fn_table,
                fn_sigs,
                fn_sig_ids,
                &user_tys,
                &ret_ty,
            )
        })
        .flatten();
    Some(BoxedEntryTarget {
        name: name.to_string(),
        fid,
        body_fid: relay.map_or(fid, |r| r.fid),
        user_tys,
        ret_ty: relay.filter(|r| r.ret_void).map_or(ret_ty, |_| Type::Void),
        has_argv: has_argv || relay.is_some(),
        recv_slots,
        lead: match relay {
            Some(r) if r.new_target => 2,
            Some(r) if r.recv_first => 1,
            _ => usize::from(twin_lane),
        },
        lead_undef: relay.is_some_and(|r| r.this_first && !r.recv_first),
        feeds_env: feeds_env && relay.is_none(),
        hidden_argc: hidden > 0,
        dflt_lits,
        type_anns: dflt_params.iter().map(|p| p.type_ann.clone()).collect(),
        rest,
        rest_kind,
        ret_arr_kind,
    })
}

/// One adapter-synthesis target — the per-FnDecl facts the targets
/// loop collects before any adapter is built.
pub(super) struct BoxedEntryTarget {
    pub(super) name: String,
    /// The closure body this adapter is registered for (the
    /// construction site's lookup key).
    pub(super) fid: FuncId,
    /// The fn the adapter CALLS — `fid` itself, except for a relay
    /// redirected to its head-less argv target (see `relay`).
    pub(super) body_fid: FuncId,
    pub(super) user_tys: Vec<Type>,
    pub(super) ret_ty: Type,
    pub(super) has_argv: bool,
    /// The receiver slots ahead of the user arguments in argv: the
    /// recv-first `__this`, and a construct face's new.target.
    pub(super) recv_slots: usize,
    /// The receiver in argv[0] is the body's user param 0 and
    /// precedes the hidden argc / argv slots in its sig (a twin on
    /// the this-first argv lane): unboxed first, the window then
    /// shifted for the rest.
    pub(super) lead: usize,
    /// A relay redirected to a one-slot this-first target: the body's
    /// leading `__this` takes the `undefined` receiver the relay's own
    /// forwarding call would have seeded.
    pub(super) lead_undef: bool,
    pub(super) feeds_env: bool,
    /// RFC 20260810-indirect-argc-abi S1 — the body's SSA sig
    /// carries the hidden I64 `__torajs_argc` at position 1 (every
    /// `__env`-first body; since S1-T1 also the this-first
    /// method_argv family); the adapter forwards its own argc there.
    pub(super) hidden_argc: bool,
    pub(super) dflt_lits: Vec<Option<DfltLit>>,
    /// The user params' type annotations, aligned with `user_tys`.
    pub(super) type_anns: Vec<Option<String>>,
    /// Last user param is an `any[]` rest — the adapter collects
    /// argv[fixed..argc] into a fresh Arr<Any> for that slot.
    pub(super) rest: bool,
    /// 刀 2 (RFC 20260815-fn-value-rest-spread) — a TYPED rest
    /// (`...args: number[]`): the collected Arr<Any> converts
    /// through `__torajs_arr_any_to_typed` with this ARR_KIND before
    /// feeding the slot (a mismatched element throws the same
    /// catchable TypeError the assign-boundary conversion does).
    /// `None` = the `any[]` spelling, no conversion.
    pub(super) rest_kind: Option<i64>,
    /// See [`BoxedEntryIntrinsics::arr_mark_kind`].
    pub(super) ret_arr_kind: Option<i64>,
}

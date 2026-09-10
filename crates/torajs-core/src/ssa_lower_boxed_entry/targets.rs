//! The adapter-synthesis TARGETS of
//! [`super::synthesize_boxed_entries`] — which FnDecls get a boxed
//! dual-entry adapter, and the per-fn facts the build reads. Split
//! from the parent (rotation 626) when the relay redirect's
//! retarget table pushed both the file and the collect loop past
//! their limits; the seam is the parent's own: admission (which
//! head shapes and factories qualify) in the loop, the facts of one
//! admitted fn in [`target_facts`], the build in `build`.

use std::collections::HashMap;

use super::admit::{program_constructs_from_value, promise_heir_classes};
use super::{DfltLit, MAX_BOXED_PARAMS, relay};
use crate::ast::{Ast, Stmt};
use crate::ssa::{self, FuncId, Type};

/// The module tables the targets loop reads, bundled once per
/// synthesis so the loop and [`target_facts`] share one carrier.
pub(super) struct TargetTables<'a> {
    pub(super) ast: &'a Ast,
    pub(super) call_retargets: &'a crate::ssa_lower::CallRetargets,
    pub(super) fn_table: &'a HashMap<String, FuncId>,
    pub(super) fn_sigs: &'a [(Vec<Type>, Type)],
    pub(super) fn_sig_ids: &'a HashMap<FuncId, ssa::SigId>,
    pub(super) arr_layouts: &'a [Type],
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

/// Head-param classification of one FnDecl: `(first_is_env,
/// first_is_this, is_twin)`. Env-first (lifted closure) and
/// this-first (`__cm_` mono method) bodies feed the adapter's env
/// argument into their pointer-shaped head param. A `__cmany_`
/// twin's `__this` is an ANY param, not a pointer (blade 3, RFC
/// 20260804-method-rebind-generic-body): it must NOT ride the env
/// channel (the env ptr's raw bits are not a nanbox) — it maps as an
/// ordinary user param instead, and the guard-fail path calls the
/// adapter recv-first (receiver prepended in argv[0], any→any
/// pass-through). An argv-face twin (synthetic `__torajs_argv`
/// head) would mis-map that slot, so it synthesizes nothing (twin
/// stays 0 in the face; recorded RFC residue).
fn head_shape(name: &str, params: &[crate::ast::Param]) -> (bool, bool, bool) {
    let head_is_this = params.first().is_some_and(|p| p.name == "__this");
    (
        params.first().is_some_and(|p| p.name == "__env"),
        head_is_this && name.starts_with("__cm_") && !name.starts_with("__cmany_"),
        head_is_this
            // Knife 3b (RFC 20260804-fn-this-channel) — the static
            // twin `__smany_` has the same any-typed `__this` head
            // and the same recv-first adapter shape. The ctor twin
            // `__ctorany_` (RFC 20260815 knife 2b) is the same shape
            // again: `(__this, __new_target, …user)`, every param
            // any — the value-parent super registry calls it through
            // this adapter.
            && (name.starts_with("__cmany_")
                || name.starts_with("__smany_")
                || name.starts_with("__ctorany_"))
            && !params.iter().any(|p| p.name == "__torajs_argv"),
    )
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
        let is_factory = name.starts_with("__new_") && {
            let cname = &name["__new_".len()..];
            constructs_from_value
                || promise_heirs.contains(cname)
                || crate::ast::desugar_classes_builtin_heritage::exotic_root_parent(ast, cname)
                    .is_some_and(|r| {
                        r == "Array"
                            || crate::ast::desugar_classes_builtin_heritage::is_typedarray_ctor(r)
                    })
        };
        if !first_is_env && !first_is_this && !is_static && !is_factory && !is_twin {
            continue;
        }
        if let Some(t) = target_facts(tables, name, params, (first_is_env, first_is_this)) {
            targets.push(t);
        }
    }
    targets
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
    let Some(&fid) = fn_table.get(name) else {
        return None;
    };
    let Some(&sig_id) = fn_sig_ids.get(&fid) else {
        return None;
    };
    let (param_tys, ret_ty) = fn_sigs[sig_id.0 as usize].clone();
    let env_count = usize::from(feeds_env);
    // RFC 20260708-closure-argv-face — a full-arguments body
    // carries the raw argv pointer right after its head param
    // (the argc channel is the S1 hidden sig slot on every tier
    // since the T/H blades — no injected argc param remains).
    let has_argv = params
        .get(env_count)
        .is_some_and(|p| p.name == "__torajs_argv");
    // param 0 is the env Ptr (when the body carries one); the
    // boxed surface covers the rest (minus the argv slot, which
    // never consumes argv positions). Two skip counts since
    // RFC 20260810-indirect-argc-abi S1: the AST param list has
    // no hidden slot, the SSA sig of an `__env`-first body has
    // the injected I64 `__torajs_argc` at position 1 — cutting
    // both sides with one count was the exact AST↔sig mis-align
    // this split exists to prevent. S1-T1 — the this-first
    // method_argv family carries the same sig-only slot (shared
    // predicate: pass 1 / setup_fn_params / here).
    let this_hidden = crate::ssa_lower_pass_1::this_first_hidden_argc(params);
    let ast_skip = env_count + usize::from(has_argv);
    let sig_skip = ast_skip + usize::from(first_is_env) + usize::from(this_hidden);
    let user_tys = param_tys[sig_skip..].to_vec();
    if user_tys.len() > MAX_BOXED_PARAMS {
        return None;
    }
    // Rest-param body: direct call sites pack trailing args into
    // an array literal (apply_rest_args), so the LAST param is an
    // ordinary Arr the static world always feeds. A runtime call
    // has no packing site — pre-fix the adapter unboxed argv
    // positionally and the rest param read one lone argument as
    // an array (garbage). The adapter collects argv[fixed..argc]
    // into a fresh Arr<Any> itself; a TYPED rest (刀 2, RFC
    // 20260815-fn-value-rest-spread) converts that collection
    // through `__torajs_arr_any_to_typed` with the element's
    // ARR_KIND. A REFCOUNTED element is one cell per slot, which
    // the kernel's HEAP path stakes — materializing a ShortStr
    // immediate rather than refusing one (the rotation-546 form).
    // Keeping this narrower than `arr_kind_chain_of`'s mapping is
    // what left `string[]` with no adapter at all, so
    // `const g = (...xs: string[]) => …; g('a')` died at "value
    // is not a function" (rotation 600).
    //
    // Nested typed arrays stay out for a reason still true: their
    // kind is a CHAIN and the kernel takes one level, so the inner
    // stamp would be lost. No adapter keeps such a call a loud
    // not-callable instead of silent garbage.
    let rest = params.last().is_some_and(|p| p.is_rest);
    let mut rest_kind: Option<i64> = None;
    if rest && params.last().and_then(|p| p.type_ann.as_deref()) != Some("any[]") {
        let elem = match user_tys.last() {
            Some(Type::Arr(id)) => arr_layouts.get(id.0 as usize),
            _ => None,
        };
        rest_kind = match elem {
            Some(Type::I64) | Some(Type::I32) => Some(torajs_rc::ARR_KIND_I64 as i64),
            Some(Type::F64) => Some(torajs_rc::ARR_KIND_F64 as i64),
            Some(Type::Bool) => Some(torajs_rc::ARR_KIND_BOOL as i64),
            Some(Type::Arr(_)) => None,
            Some(t) if t.is_refcounted() => Some(torajs_rc::ARR_KIND_HEAP as i64),
            _ => None,
        };
        if rest_kind.is_none() {
            return None;
        }
    }
    if !user_tys.iter().all(boxable) || !(ret_ty == Type::Void || boxable(&ret_ty)) {
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
            let elem = arr_layouts[id.0 as usize].clone();
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
    let dflt_lits: Vec<Option<DfltLit>> = params[ast_skip..]
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
    let recv_slot = first_is_env && params.get(ast_skip).is_some_and(|p| p.name == "__this");
    // The relay redirect (module doc in `relay`): the adapter of
    // a `__forward_<t>` enters `t` with its own argc / argv.
    let relay_fid = (first_is_env && !rest)
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
        body_fid: relay_fid.unwrap_or(fid),
        user_tys,
        ret_ty,
        has_argv: has_argv || relay_fid.is_some(),
        recv_slot,
        feeds_env: feeds_env && relay_fid.is_none(),
        hidden_argc: first_is_env || this_hidden,
        dflt_lits,
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
    pub(super) recv_slot: bool,
    pub(super) feeds_env: bool,
    /// RFC 20260810-indirect-argc-abi S1 — the body's SSA sig
    /// carries the hidden I64 `__torajs_argc` at position 1 (every
    /// `__env`-first body; since S1-T1 also the this-first
    /// method_argv family); the adapter forwards its own argc there.
    pub(super) hidden_argc: bool,
    pub(super) dflt_lits: Vec<Option<DfltLit>>,
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

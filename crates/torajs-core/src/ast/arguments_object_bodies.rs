//! The per-FnDecl body rewrite of
//! [`super::arguments_object::desugar_arguments_object`] — split to a
//! sibling (rotation 416) when the head-less argv tier pushed that
//! pass past the 200-line fn limit. Verbatim move.
//!
//! The split is the pass's own seam: everything left behind answers
//! "which tier is this fn in", and everything here answers "what does
//! a body on that tier turn into" — mode classification, the
//! materialize / FLAG_ARR_ARGUMENTS-mark / sloppy-callee prologue,
//! and the `__forward_` shims the sloppy goal mints along the way.

use super::arguments_object::ArgcMode;
use super::arguments_object_rewrite::SloppyCallee;
use super::arguments_object_sloppy::{CalleeSpelling, PendingFwds, callee_value_fn};
use super::arguments_object_synth::{synth_arguments_local_argv, synth_materialized_arguments};
use super::arguments_object_walkers::{
    body_has_callee_touch, body_has_callee_write, stmt_uses_dynamic_arguments,
};
use super::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

/// The tier memberships the body rewrite consults, exactly as the
/// main pass computed them.
pub(super) struct BodyTiers<'a> {
    pub(super) shadowed: &'a HashSet<String>,
    pub(super) iife_static_argv: &'a HashMap<String, usize>,
    pub(super) uses_real_argc: &'a HashSet<String>,
    pub(super) method_argv_fns: &'a HashSet<String>,
    pub(super) iife_real_argc: &'a HashSet<String>,
    pub(super) value_real_argc: &'a HashSet<String>,
    pub(super) value_argv_fns: &'a HashSet<String>,
    pub(super) env_fns: &'a HashSet<String>,
    pub(super) headless_argv_fns: &'a HashSet<String>,
}

pub(super) fn rewrite_fn_bodies(
    ast: &mut Ast,
    fn_params: &HashMap<String, Vec<String>>,
    t: BodyTiers<'_>,
) {
    // S2 — the sloppy goal's callee-value shims, appended to the
    // module after the loop (appending mid-loop is safe for the
    // idx-addressed writebacks, but batching keeps the shim decls
    // off this pass's own iteration).
    let (fwd_sigs, existing_fwds, _fwd_type_params) =
        super::forwarders_object::snapshot_fn_sigs(ast);
    let mut pending_fwds = PendingFwds {
        decls: Vec::new(),
        known: existing_fwds,
        sigs: fwd_sigs,
        zero_env: super::arguments_object_sloppy::collect_zero_env_closures(ast),
    };
    let stmts_clone: Vec<Stmt> = ast.stmts.clone();
    for (idx, stmt) in stmts_clone.iter().enumerate() {
        if let Stmt::FnDecl {
            name,
            params: decl_params,
            body,
            ..
        } = stmt
        {
            rewrite_one_body(
                ast,
                idx,
                name,
                decl_params,
                body,
                fn_params,
                &t,
                &mut pending_fwds,
            );
        }
    }

    // S2 — land the sloppy callee-value shims synthesized above.
    ast.stmts.extend(pending_fwds.decls);
}

/// One FnDecl's rewrite — mode classification, the materialize /
/// mark / sloppy-callee prologue, and the body walk. Carved out of
/// [`rewrite_fn_bodies`] to keep that fn inside the 200-line limit
/// (verbatim move; the tier lookups read through the carrier).
#[allow(clippy::too_many_arguments)]
fn rewrite_one_body(
    ast: &mut Ast,
    idx: usize,
    name: &str,
    decl_params: &[super::Param],
    body: &[Stmt],
    fn_params: &HashMap<String, Vec<String>>,
    t: &BodyTiers<'_>,
    pending_fwds: &mut PendingFwds,
) {
    let &BodyTiers {
        shadowed,
        iife_static_argv,
        uses_real_argc,
        method_argv_fns,
        iife_real_argc,
        value_real_argc,
        value_argv_fns,
        env_fns,
        headless_argv_fns,
    } = t;
    // A shadowed fn's `arguments` idents refer to its OWN
    // `var arguments` local — leave the body untouched (the
    // hoisted decl resolves them as an ordinary binding).
    if shadowed.contains(name) {
        return;
    }
    let Some(params) = fn_params.get(name) else {
        return;
    };
    let params = params.clone();
    let is_argv_fn = value_argv_fns.contains(name)
        || method_argv_fns.contains(name)
        || headless_argv_fns.contains(name);
    // RFC 20260708-closure-argv-face chunk 2 — an env
    // closure that did NOT qualify for the argv face (killed
    // binding chain / escaping touch / unsafe return) stays
    // loud on EVERY arguments touch, index reads included:
    // the historical FoldArity path rewrote `arguments[i]`
    // into a declared-params-only array, silently answering
    // undefined for beyond-declared values reached through
    // any-call / container dispatch (probe ac6/ac7).
    let argc_mode = super::arguments_object_stages::classify_argc_mode(
        ast,
        body,
        name,
        decl_params,
        &params,
        super::arguments_object_stages::ArgcTiers {
            iife_static_argv: iife_static_argv,
            uses_real_argc: uses_real_argc,
            method_argv_fns: method_argv_fns,
            iife_real_argc: iife_real_argc,
            value_real_argc: value_real_argc,
            value_argv_fns: value_argv_fns,
            env_fns: env_fns,
        },
    );
    // T-11 — pre-pass: detect any dynamic `arguments[<non-
    // literal>]` use. If found, prepend a synthesized
    // `let __torajs_arguments: any[] = [p0, p1, ...]` before
    // the body and rewrite the dynamic indices to read from it.
    // Literal-index rewrites (the existing path) take priority
    // and don't materialize the array — they stay zero-cost.
    // argv-face bodies always materialize (from the
    // adapter's argv — beyond-declared values included);
    // named-fn bodies keep the declared-params builder,
    // gated on a dynamic use.
    let mut needs_materialize =
        is_argv_fn || matches!(argc_mode, ArgcMode::LiveLength(_) | ArgcMode::Unmapped(_));
    if !needs_materialize {
        for s in body {
            if stmt_uses_dynamic_arguments(ast, s) {
                needs_materialize = true;
                break;
            }
        }
    }
    // RFC 20260810-sloppy-goal-arguments S2 — a sloppy body
    // that writes or deletes `arguments.callee` must
    // materialize: the keyed rewrite lands on the bag entry
    // the mint's defineProperty seeded (S10.6_A3_T3/T4).
    if !needs_materialize && ast.sloppy_script_goal && body_has_callee_write(ast, body) {
        needs_materialize = true;
    }
    // S2 — a sloppy body that touches callee (any position)
    // or materializes needs the fn's closure-shaped value:
    // ensure the `__forward_<name>` shim exists (reusing the
    // fn-to-closure pass's synthesizer — a bare fn Ident is
    // not a closure-shaped value this late in the pipeline).
    // KeepLoud keeps the strict thrower even under the
    // sloppy goal: an env closure has no plain-call shim to
    // forward to (recorded sloppy-goal face, plan-state L3b).
    // Class methods (the flattened `__cm*` family — the
    // generator state machine's included) are excluded
    // wholesale: the plain-call shim would feed their
    // receiver slot `undefined` (the forbidden-ext async-gen
    // family's "expected ClassRef, got Undefined"), and a
    // method's callee value is a recorded sloppy window.
    let wants_callee_value = ast.sloppy_script_goal
        && argc_mode != ArgcMode::KeepLoud
        && !name.starts_with("__cm")
        && (needs_materialize || body_has_callee_touch(ast, body));
    let spelling_opt = if wants_callee_value {
        callee_value_fn(ast, name, pending_fwds)
    } else {
        None
    };
    // How `arguments.callee` spells in this body (see
    // SloppyCallee).
    let sloppy_callee = if !ast.sloppy_script_goal || argc_mode == ArgcMode::KeepLoud {
        SloppyCallee::Strict
    } else if needs_materialize {
        SloppyCallee::Keyed
    } else {
        match &spelling_opt {
            Some(CalleeSpelling::Shim(f)) => SloppyCallee::Closure(f),
            Some(CalleeSpelling::EvalCall(w)) => SloppyCallee::EvalCall(w),
            None => SloppyCallee::Strict,
        }
    };
    let new_body: Vec<Stmt> = body
        .iter()
        .map(|s| {
            crate::ast::arguments_object_rewrite::rewrite_arguments_in_stmt(
                ast,
                s,
                &params,
                argc_mode,
                is_argv_fn,
                sloppy_callee,
            )
        })
        .collect();
    // Synthesize the local OUTSIDE the &mut ast.stmts borrow
    // (synth_arguments_local also takes &mut ast for add_expr).
    let argc_len_opt = (argc_mode == ArgcMode::RealLocal)
        .then(|| super::arguments_object_synth::synth_argc_len_local(ast));
    let synth_opt = if is_argv_fn {
        // S3.3 — the materialize take-count is the S1 hidden
        // argc; since S1-T2 both argv faces own the slot.
        Some(synth_arguments_local_argv(ast))
    } else if needs_materialize && argc_mode != ArgcMode::KeepLoud {
        Some(synth_materialized_arguments(
            ast,
            decl_params,
            &params,
            argc_mode,
        ))
    } else {
        None
    };
    // The FLAG_ARR_ARGUMENTS stamp rides a synthetic call
    // statement right after the mint — one mechanism for
    // both the literal and argv lanes (checker ident
    // special-case + class-synth lowering arm).
    let mark_opt = synth_opt
        .is_some()
        .then(|| super::arguments_object_synth::synth_arguments_mark(ast));
    // S2 — the sloppy goal seeds `callee` into the bag right
    // after the mark, through the ordinary defineProperty
    // pipeline (keyed reads / writes / deletes / gOPD all
    // ride the existing expando machinery). The value is the
    // `__forward_` closure shim ensured above.
    let sloppy_define_opt = match (&synth_opt, &spelling_opt) {
        (Some(_), Some(spelling)) => Some(
            super::arguments_object_synth::synth_sloppy_callee_define(ast, spelling),
        ),
        _ => None,
    };
    if let Stmt::FnDecl { body: b, .. } = &mut ast.stmts[idx] {
        if argc_len_opt.is_some() || synth_opt.is_some() {
            let mut full = Vec::with_capacity(new_body.len() + 4);
            full.extend(argc_len_opt);
            full.extend(synth_opt);
            if let Some(mark) = mark_opt {
                full.push(mark);
            }
            if let Some(dp) = sloppy_define_opt {
                full.push(dp);
            }
            full.extend(new_body);
            *b = full;
        } else {
            *b = new_body;
        }
    }
}

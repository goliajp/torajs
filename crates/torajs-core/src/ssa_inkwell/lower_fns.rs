//! Pass D — body lowering + per-fn LLVM attribute passes
//! (DISubprogram, memory(none), Internal linkage, alwaysinline).
//!
//! Extracted from `compile_for_kind_impl` in `ssa_inkwell.rs`
//! (2026-05-25, god-file decomp batch 22b).

use std::collections::HashMap;

use inkwell::context::Context;
use inkwell::debug_info::{AsDIScope, DIFlagsConstants};
use inkwell::values::FunctionValue;

use super::attrs::{mark_alwaysinline, mark_memory_none, ssa_fn_is_pure};
use super::lower::FnLower;
use super::{DebugCtx, OutputKind};
use crate::ssa::{InstKind, Module};

/// Pass D entry. Mutates `fn_map`'s LLVM-side metadata (linkage,
/// alwaysinline, memory(none)) and lowers user fn bodies via
/// `FnLower::run`. No return; side-effects on the LLVM module.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_user_fns<'ctx>(
    ssa_module: &Module,
    ctx: &'ctx Context,
    builder: &inkwell::builder::Builder<'ctx>,
    fn_map: &[FunctionValue<'ctx>],
    string_globals: &[inkwell::values::GlobalValue<'ctx>],
    static_str_globals: &[inkwell::values::GlobalValue<'ctx>],
    data_globals: &HashMap<String, inkwell::values::GlobalValue<'ctx>>,
    vtable_globals: &HashMap<String, inkwell::values::GlobalValue<'ctx>>,
    ast: Option<&crate::ast::Ast>,
    debug_ctx: Option<&DebugCtx<'ctx>>,
    opt: &str,
    kind: OutputKind,
) {
    // Pass D: lower bodies for every SSA function that has blocks AND isn't
    // a backend-owned intrinsic.
    let intrinsics = [
        "print_i64",
        "print_f64",
        "print_bool",
        "__torajs_obj_alloc",
        "__torajs_obj_drop",
        "__torajs_arr_alloc",
        "__torajs_arr_push_unchecked",
        "__torajs_arr_drop",
        "__torajs_throw_set",
        "__torajs_throw_check",
        "__torajs_throw_take",
        "__torajs_throw_take_tag",
    ];

    // v0.3 #4 D-2 — attach DISubprogram to every fn that's about to
    // lower its body via FnLower (i.e. user fns + runtime fns
    // synthesized at SSA layer). Done BEFORE the lowering loop so
    // each FnLower::run can pick up the subprogram and emit
    // !dbg-equipped instructions. Backend-owned intrinsics
    // (str_alloc, str_drop, ...) skip this — they're emitted by
    // their `define_*` fns which don't take a debug ctx; LLVM is
    // happy to leave them DI-less since they have no DISubprogram.
    let sub_ty_opt = debug_ctx.as_ref().map(|dctx| {
        dctx.dibuilder.create_subroutine_type(
            dctx.compile_unit.get_file(),
            None,
            &[],
            inkwell::debug_info::DIFlags::PUBLIC,
        )
    });
    if let (Some(dctx), Some(sub_ty)) = (debug_ctx.as_ref(), sub_ty_opt.as_ref()) {
        for (i, f) in ssa_module.funcs.iter().enumerate() {
            if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
                continue;
            }
            let llvm_fn = fn_map[i];
            // line_no = 0 placeholder until D-3 carries fn-decl line.
            // 0 is DWARF's "unknown"; tools fall back to scope-only.
            let sp = dctx.dibuilder.create_function(
                dctx.compile_unit.as_debug_info_scope(),
                &f.name,
                None,
                dctx.compile_unit.get_file(),
                0,
                *sub_ty,
                /* is_local_to_unit */ false,
                /* is_definition */ true,
                /* scope_line */ 0,
                inkwell::debug_info::DIFlags::PUBLIC,
                /* is_optimized */ opt != "O0",
            );
            llvm_fn.set_subprogram(sp);
        }
    }

    for (i, f) in ssa_module.funcs.iter().enumerate() {
        if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
            continue;
        }
        // v0.3 #4 D-2 — set a default DILocation for this fn so
        // LLVM verify's "inlinable call needs !dbg" rule is
        // satisfied. line=0 / col=0 is a placeholder; D-3 will
        // override per-instruction with actual span lookup.
        if let Some(dctx) = debug_ctx.as_ref()
            && let Some(sp) = fn_map[i].get_subprogram()
        {
            let loc =
                dctx.dibuilder
                    .create_debug_location(ctx, 0, 0, sp.as_debug_info_scope(), None);
            builder.set_current_debug_location(loc);
        }
        let lower = FnLower {
            ctx,
            builder,
            ssa_fn: f,
            llvm_fn: fn_map[i],
            fn_map,
            string_globals,
            static_str_globals,
            data_globals,
            vtable_globals,
            ssa_module,
            ast,
            debug_ctx,
            block_map: HashMap::new(),
            value_map: HashMap::new(),
        };
        lower.run();
    }

    // v0.3 #4 D-2 — finalize DI metadata before LLVM verify (which
    // rejects incomplete DICompileUnits).
    if let Some(dctx) = debug_ctx {
        dctx.dibuilder.finalize();
    }

    /* v0.6+1 perf checkpoint — fn-purity attribute pass.
     *
     * Walks every user FnDecl's lowered SSA body; if it has zero
     * memory access (no Load / Store / Call / Alloca etc — see
     * ssa_fn_is_pure for the exact predicate), tag the LLVM fn
     * with `memory(none)`. LLVM's LICM / GVN then know calls to
     * the fn have zero side effects → invariant loads in the
     * caller's loops can be hoisted past the call.
     *
     * Concrete win: `function id<T>(x: T): T { return x }` in a
     * tight `for (let i = 0; i < xs.length; i++) sum += id(xs[i])`
     * loop. Pre-tag, LLVM reloads `xs.length` (and the array data
     * pointer) on every iteration because the call to `id` could,
     * in principle, modify them. With memory(none) the loads
     * hoist out and the loop becomes the same shape as rust's
     * inlined version. */
    for (i, f) in ssa_module.funcs.iter().enumerate() {
        if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
            continue;
        }
        /* `main` always touches memory (top-level user code) and
         * doesn't benefit from the attr — skip it explicitly. */
        if f.name == "main" {
            continue;
        }
        if ssa_fn_is_pure(f) {
            mark_memory_none(ctx, fn_map[i]);
        }
    }

    /* P-PERF.A1 (2026-05-22) — Internal-linkage pass for user
     * FnDecls (plus all tora-synthesized internal helpers like
     * `__closure_N` / `__env_drop_N` / `__cm_<C>__<m>` /
     * `__forward_<name>`). Pre-P-PERF.A1 those fns were emitted
     * with default LLVM linkage (External) — which forces LLVM
     * to assume the symbol may be called from outside the module.
     * Consequence: no IPSCCP, no per-callsite specialization, no
     * inlining of single-call helpers. Concrete leakage on the
     * `reduce(xs, add1)` shape — closure-pipeline-1m's hot loop
     * is `tail call i64 %f(i64 %arg)` through an opaque fn-ptr
     * because LLVM can't see `add1` flows in from main.
     *
     * With Internal linkage:
     *  - IPSCCP folds the fn-ptr param to a constant per call site
     *  - Inliner inlines the reducer body into main
     *  - The (now-direct) inner call to add1 inlines too
     *  - LLVM unrolls + vectorizes the loop
     *
     * Scope: Executable output only. SharedLib output (torajs-embed
     * crate's `compile_to_dylib` path) MUST keep user fns External
     * — the embedding host (C / Rust runtime) dlopens the .dylib
     * and looks up user-fn symbols by name; Internal linkage hides
     * them from the dynamic symbol table.
     *
     * Skip set within Executable:
     *  - declarations (extern C runtime helpers — MUST stay External)
     *  - intrinsics (tora-defined fns called cross-TU by the C runtime
     *    side; their symbols must be visible at link time)
     *  - `main` (renamed to platform-specific entry by declare_ssa_fn;
     *    the OS / wasi-libc resolves it as External by ABI)
     *  - `__main_argc_argv` (wasi-libc lookup target; same reason).
     */
    if matches!(kind, OutputKind::Executable) {
        for (i, f) in ssa_module.funcs.iter().enumerate() {
            if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
                continue;
            }
            if f.name == "main" || f.name == "__main_argc_argv" {
                continue;
            }
            fn_map[i].set_linkage(inkwell::module::Linkage::Internal);
        }

        /* P-PERF.A3 (2026-05-22) — alwaysinline for small,
         * non-recursive user fns. LLVM's cost-model inliner is
         * conservative on fns with loops; small reducer-shaped
         * helpers (add1, gcd inner, is_prime, etc.) that get
         * called from a tight outer loop benefit from
         * unconditional inline → enables downstream vectorize /
         * unroll on the (now flattened) inner loop.
         *
         * Heuristic:
         *   - body SSA inst count below threshold (60), AND
         *   - no Call back to self anywhere (recursive fns are
         *     skipped — alwaysinline would infinite-expand).
         *
         * Threshold tuning history:
         *   - 60 (P-PERF.A3, 2026-05-22 ship): real wins on
         *     numeric/array hot paths; only promise-all-1k +4%
         *     borderline regression.
         *   - 30 (P-PERF.A4 attempt, reverted): net worse than
         *     60 across the suite (csv-rebuild +8.6%, async-fn-call
         *     +8.5%, rpn-eval +11.2% vs A1). Missed the 30-60-inst
         *     mid-size helper range where A3 was actually winning.
         *     Geomean 4.16 → 4.15 (-0.3%). The "i-cache pressure"
         *     hypothesis for the A3 promise-all regression turned
         *     out wrong — promise-all's cost is elsewhere; the
         *     mid-size helpers are NET positive.
         *   - 60 (current, A4 reverted): sweet spot per empirical
         *     measurement. Don't drop without rebenchmarking on
         *     the full 26-case suite.
         *
         * Recursive case (fib40 / ackermann) explicitly excluded.
         * `main` already excluded above. */
        for (i, f) in ssa_module.funcs.iter().enumerate() {
            if f.is_declaration() || intrinsics.contains(&f.name.as_str()) {
                continue;
            }
            if f.name == "main" || f.name == "__main_argc_argv" {
                continue;
            }
            let inst_count: usize = f.blocks.iter().map(|b| b.insts.len()).sum();
            if inst_count >= 60 {
                continue;
            }
            let self_recursive = f.blocks.iter().any(|b| {
                b.insts.iter().any(|inst| {
                    matches!(
                        inst.kind,
                        InstKind::Call(fid, _) if ssa_module.func_name(fid) == f.name.as_str()
                    )
                })
            });
            if self_recursive {
                continue;
            }
            mark_alwaysinline(ctx, fn_map[i]);
        }
    }
}

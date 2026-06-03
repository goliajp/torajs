//! LLVM attribute helpers + SSA-side pure-fn / fetch-use detectors.
//!
//! These are small, side-effect-free predicates + attribute setters
//! used by `compile_for_kind_impl` to annotate the IR for LLVM's
//! optimizer. Extracted from `ssa_inkwell.rs` god-file decomposition
//! (2026-05-25, batch 3).

use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::context::Context;
use inkwell::values::FunctionValue;

use crate::ssa::{self as s, InstKind, Module};

/// Mark a function as `alwaysinline` — LLVM forces inlining at every
/// call site regardless of cost model. Used for hot, small intrinsics
/// (e.g. `__torajs_str_char_code_at`) where the per-call C-function-
/// boundary cost dwarfs the body. Must be called AFTER `add_function`
/// and BEFORE the body lowers; doesn't change function semantics.
pub(super) fn mark_alwaysinline<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    let kind = Attribute::get_named_enum_kind_id("alwaysinline");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Function, attr);
}

/// Tag a function with LLVM's `memory(...)` attribute carrying an
/// explicit encoded bitmask. LLVM 16+ packs three location/ModRef
/// pairs into a single u64:
///
///   bits 0-1   ArgMem          (0 = none, 1 = read, 2 = write, 3 = readwrite)
///   bits 2-3   InaccessibleMem (same layout)
///   bits 4-5   Other           (same layout)
///
/// So e.g. `memory(argmem: readwrite)` = 3, `memory(argmem: read)` = 1,
/// `memory(inaccessiblemem: readwrite)` = 12, `memory(argmem: readwrite,
/// inaccessiblemem: readwrite)` = 15, `memory(none)` = 0. The encoding
/// is locked in by `cfg(test)` tests below — bumping LLVM versions
/// must re-validate against them.
pub(super) fn mark_memory_effect<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>, encoded: u32) {
    let kind = Attribute::get_named_enum_kind_id("memory");
    let attr = ctx.create_enum_attribute(kind, encoded as u64);
    f.add_attribute(AttributeLoc::Function, attr);
}

/// T-24-prep (v0.6+1) — mark a function as `memory(none)` so LLVM's
/// LICM / GVN can hoist invariant loads through call sites. Applied
/// to user FnDecls whose SSA body is provably pure: no Store /
/// StoreDyn / Call / CallIndirect anywhere. The dominant win is
/// `id<T>(x: T): T { return x }`-shape generic helpers in tight
/// loops (generic-id-1m: `xs.length` reload through the call site
/// disappears once LLVM knows the call has zero memory effect).
///
/// Conservative on the false-negative side — Load/LoadDyn alone
/// would qualify for `memory(read)`, but that's harder to apply
/// safely (caller's stack alloca writes vs callee's heap reads
/// need explicit alias info LLVM can't infer cheaply); ship the
/// strict-none variant first, expand to read-only later if a
/// bench case proves the gap.
pub(super) fn mark_memory_none<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    mark_memory_effect(ctx, f, 0);
}

/// `nounwind` — function never raises an exception (in our case
/// never `longjmp`s out via `__torajs_throw_*`). All libc fns we
/// call (malloc/free/memcpy/...) are nounwind by spec; same for
/// the torajs pool-aware free / alloc family. Lets LLVM elide
/// invoke-vs-call landing-pad bookkeeping and treat calls as
/// pure control-flow edges.
pub(super) fn mark_nounwind<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    let kind = Attribute::get_named_enum_kind_id("nounwind");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Function, attr);
}

/// `willreturn` — function is guaranteed to return to its caller
/// (no infinite loops, no `abort`, no `longjmp`). Combined with
/// `mustprogress`, this lets LLVM remove provably dead calls and
/// hoist invariant loads through call sites in loops whose only
/// "interesting" exit is the call returning.
pub(super) fn mark_willreturn<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    let kind = Attribute::get_named_enum_kind_id("willreturn");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Function, attr);
}

/// `mustprogress` — function (or one of its descendants) is
/// required to make observable forward progress in finite time.
/// Required for LICM hoisting in loops where dropping the call
/// would otherwise change termination behavior; pairs with
/// `willreturn` for the strongest guarantee.
pub(super) fn mark_mustprogress<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    let kind = Attribute::get_named_enum_kind_id("mustprogress");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Function, attr);
}

/// `nofree` — function never frees memory reachable via pointer
/// arguments or globals. Applies cleanly to `memcmp` (pure read)
/// but NOT to `free` / `realloc` / pool-aware allocators (which
/// by design release blocks back to libc / the per-cap pool).
pub(super) fn mark_nofree<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    let kind = Attribute::get_named_enum_kind_id("nofree");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Function, attr);
}

// `allocsize(N)` single-arg form is deferred to a Step 12-a-1
// follow-up. Inkwell 0.9 + llvm-sys-22.1 only expose the raw
// `LLVMCreateEnumAttribute(kind, value: u64)` ABI, and every
// encoding via that path round-trips as the two-arg form
// `allocsize(X, Y)` — which is semantically wrong for malloc-
// shape allocators (LLVM would interpret it as size*count from
// two args). Doing this correctly needs the dedicated
// `LLVMCreateAllocSizeAttribute(ctx, ElemSize, NumElems)` C
// API, which llvm-sys-22.1 doesn't bind yet — adding it is a
// narrow llvm-sys patch / FFI extern, separate from this batch.
// The other LICM-blocking attributes (memory / nounwind /
// willreturn / mustprogress / nofree) cover the bulk of
// array-sum-1m's hoist gap on their own.

/// One-shot helper that applies the canonical "well-behaved extern
/// call" attribute bundle: `nounwind` + `willreturn` +
/// `mustprogress` + `memory(<encoded>)`. Every libc /
/// pool-aware-alloc / pool-aware-free declaration in `declares.rs`
/// is well-behaved in this sense — they always return, never
/// unwind across the FFI boundary, and (with the right `memory(...)`
/// mask) only touch the memory locations LLVM is told about. The
/// `nofree` attribute is applied separately by the few sites that
/// genuinely never call `free` on their args (notably `memcmp`).
pub(super) fn mark_extern_canonical<'ctx>(
    ctx: &'ctx Context,
    f: FunctionValue<'ctx>,
    memory_encoded: u32,
) {
    mark_nounwind(ctx, f);
    mark_willreturn(ctx, f);
    mark_mustprogress(ctx, f);
    mark_memory_effect(ctx, f, memory_encoded);
}

/// T-21 link-time gate. Walk every fn's instructions; return true
/// iff any Call targets a function named `__torajs_fetch_sync`. The
/// intrinsic is only declared (and only ever called) when ssa_lower
/// has lowered a `fetch(url)` site, so this doubles as "does the
/// program use fetch".
pub(super) fn module_uses_fetch(module: &Module) -> bool {
    for f in &module.funcs {
        for blk in &f.blocks {
            for inst in &blk.insts {
                if let InstKind::Call(fid, _) = &inst.kind
                    && module.func_name(*fid) == "__torajs_fetch_sync"
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Walk a SSA Function's blocks + insts and return true iff the body
/// performs zero memory mutation AND zero unknown-effect calls.
/// Pure as defined here:
///   - no Store / StoreDyn (never writes memory observable to caller)
///   - no Call (we conservatively treat all callees as having effects;
///     refining this to "transitive purity" is a follow-up)
///   - no CallIndirect (function-pointer call → can be anything)
///   - no Alloca / AllocaBytes (these allocate stack but the caller
///     doesn't observe; technically pure but LLVM may still see the
///     `mem(none)` lie — safer to treat as "has memory effect" in
///     this conservative sweep).
///
/// Loads are fine — readonly memory access doesn't break memory(none)
/// in the strict sense for return values (LLVM treats memory(none) as
/// "no read AND no write"; a fn with Load wouldn't qualify here).
/// We err on the strict side: only fns with literally zero memory
/// inst kinds get tagged.
pub(super) fn ssa_fn_is_pure(f: &s::Function) -> bool {
    for blk in &f.blocks {
        for inst in &blk.insts {
            match &inst.kind {
                InstKind::Store(..)
                | InstKind::StoreDyn(..)
                | InstKind::Load(..)
                | InstKind::LoadDyn(..)
                | InstKind::Call(..)
                | InstKind::CallIndirect(..)
                | InstKind::Alloca(_)
                | InstKind::AllocaBytes(_) => return false,
                _ => {}
            }
        }
    }
    true
}

/// Tag a function as returning a fresh, non-aliasing pointer (libc
/// `malloc` semantics). Lets LLVM hoist invariant loads through
/// foreign writes — e.g. in rpn-eval-100k, `parts.length` (parts
/// from str_split) gets hoisted out of the inner loop because the
/// stack writes (stack from arr_alloc) provably can't alias it.
///
/// Apply only to allocators that genuinely return a fresh ptr each
/// call (str_alloc, arr_alloc, str_split, substr_create, ...).
/// `arr_push` / `arr_reserve` return the same ptr they got OR a
/// reallocated one — those are NOT noalias.
pub(super) fn mark_noalias_ret<'ctx>(ctx: &'ctx Context, f: FunctionValue<'ctx>) {
    let kind = Attribute::get_named_enum_kind_id("noalias");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Return, attr);
}

/// Whitelist of intrinsics whose return is a fresh-from-alloc pointer
/// suitable for `noalias` tagging. The list is conservative — anything
/// that *might* return an existing pointer (arr_push / arr_reserve /
/// arr_unshift / arr_extend_unchecked) is excluded. Misuse here is
/// undefined behavior at the LLVM level (silent miscompile under
/// alias analysis), so additions need clear "always fresh" semantics.
pub(super) fn is_alloc_intrinsic(name: &str) -> bool {
    matches!(
        name,
        // Str constructors
        "__torajs_str_alloc"
        | "__torajs_str_alloc_pooled"
        | "__torajs_str_concat"
        | "__torajs_str_slice"
        | "__torajs_str_substring"
        | "__torajs_str_repeat"
        | "__torajs_str_to_upper"
        | "__torajs_str_to_lower"
        | "__torajs_str_trim"
        | "__torajs_str_trim_start"
        | "__torajs_str_trim_end"
        | "__torajs_str_pad_start"
        | "__torajs_str_pad_end"
        | "__torajs_str_at"
        | "__torajs_str_from_char_code"
        | "__torajs_str_from_code_point"
        | "__torajs_str_normalize"
        | "__torajs_str_replace"
        | "__torajs_str_replace_all"
        | "__torajs_substr_to_owned"
        // Substr constructors
        | "__torajs_substr_create"
        | "__torajs_substr_slice"
        | "__torajs_substr_substring"
        | "__torajs_substr_trim"
        | "__torajs_substr_trim_start"
        | "__torajs_substr_trim_end"
        | "__torajs_substr_concat_substr_str"
        | "__torajs_substr_concat_str_substr"
        | "__torajs_substr_concat_substr_substr"
        // Array constructors that always return a fresh block
        | "__torajs_arr_alloc"
        | "__torajs_arr_alloc_pooled"
        | "__torajs_arr_slice"
        // Object / closure / regex / date constructors
        | "__torajs_obj_alloc"
        // String split returns a single fresh block (header + slots
        // + inline substr structs); does not alias its inputs.
        | "__torajs_str_split"
        | "__torajs_str_match_regex"
        | "__torajs_str_replace_regex"
        | "__torajs_str_replace_all_regex"
        | "__torajs_str_split_regex"
        | "__torajs_str_match_all_regex"
        | "__torajs_regex_compile"
        | "__torajs_regex_exec"
        | "__torajs_date_alloc_now"
        | "__torajs_date_alloc_ms"
        | "__torajs_date_alloc_iso"
        | "__torajs_date_alloc_components"
        | "__torajs_date_to_iso_string"
        | "__torajs_process_argv"
        | "__torajs_process_cwd"
        | "__torajs_process_platform"
        | "__torajs_process_getenv"
        | "__torajs_fs_read_file_sync"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::AddressSpace;
    use inkwell::context::Context;

    /// Build a tiny module with one extern declaration, run the
    /// given closure to apply attributes to that fn, then return
    /// the IR text. Used to validate that each `mark_*` helper
    /// emits the expected LLVM IR syntax under LLVM 22.
    fn ir_with_apply(apply: impl FnOnce(&Context, FunctionValue<'_>)) -> String {
        let ctx = Context::create();
        let m = ctx.create_module("attr_emit_test");
        let void_t = ctx.void_type();
        let ptr_t = ctx.ptr_type(AddressSpace::default());
        let i64_t = ctx.i64_type();
        let fn_t = void_t.fn_type(&[ptr_t.into(), ptr_t.into(), i64_t.into()], false);
        let f = m.add_function("dummy", fn_t, None);
        apply(&ctx, f);
        m.print_to_string().to_string()
    }

    /// `memory(none)` = all locations NoModRef, encoded value 0.
    /// Same call path as `mark_memory_none` — locks in the invariant
    /// that value 0 prints `memory(none)`.
    #[test]
    fn memory_encoding_none_zero() {
        let ir = ir_with_apply(|c, f| mark_memory_effect(c, f, 0));
        assert!(
            ir.contains("memory(none)"),
            "expected `memory(none)` in IR for encoded=0, got:\n{ir}"
        );
    }

    /// ArgMem bits 0-1 = ModRef (3); others NoModRef. Expect
    /// `memory(argmem: readwrite)` — used by memcpy/memmove
    /// declarations in Step 12-a-1.
    #[test]
    fn memory_encoding_argmem_readwrite() {
        let ir = ir_with_apply(|c, f| mark_memory_effect(c, f, 3));
        assert!(
            ir.contains("memory(argmem: readwrite)"),
            "expected `memory(argmem: readwrite)` in IR for encoded=3, got:\n{ir}"
        );
    }

    /// ArgMem bits 0-1 = Ref (1); others NoModRef. Expect
    /// `memory(argmem: read)` — used by memcmp declaration.
    #[test]
    fn memory_encoding_argmem_read() {
        let ir = ir_with_apply(|c, f| mark_memory_effect(c, f, 1));
        assert!(
            ir.contains("memory(argmem: read)"),
            "expected `memory(argmem: read)` in IR for encoded=1, got:\n{ir}"
        );
    }

    /// InaccessibleMem bits 2-3 = ModRef (3 << 2 = 12); others
    /// NoModRef. Expect `memory(inaccessiblemem: readwrite)` —
    /// used by malloc declaration (touches allocator state but no
    /// caller-visible memory).
    #[test]
    fn memory_encoding_inaccessible_readwrite() {
        let ir = ir_with_apply(|c, f| mark_memory_effect(c, f, 12));
        assert!(
            ir.contains("memory(inaccessiblemem: readwrite)"),
            "expected `memory(inaccessiblemem: readwrite)` in IR for encoded=12, got:\n{ir}"
        );
    }

    /// ArgMem (3) | InaccessibleMem (3 << 2 = 12) = 15; Other still
    /// NoModRef. Expect `memory(argmem: readwrite, inaccessiblemem:
    /// readwrite)`. Used by realloc/free/arr_alloc declarations —
    /// the bulk of Step 12-a-1's attribute set.
    #[test]
    fn memory_encoding_argmem_and_inaccessible() {
        let ir = ir_with_apply(|c, f| mark_memory_effect(c, f, 15));
        // LLVM may print the locations in either order; accept both.
        let canonical = ir.contains("memory(argmem: readwrite, inaccessiblemem: readwrite)");
        let reversed = ir.contains("memory(inaccessiblemem: readwrite, argmem: readwrite)");
        assert!(
            canonical || reversed,
            "expected `memory(argmem: readwrite, inaccessiblemem: readwrite)` (either order) for encoded=15, got:\n{ir}"
        );
    }

    #[test]
    fn nounwind_helper_emits_attribute() {
        let ir = ir_with_apply(|c, f| mark_nounwind(c, f));
        assert!(
            ir.contains("nounwind"),
            "expected `nounwind` in IR, got:\n{ir}"
        );
    }

    #[test]
    fn willreturn_helper_emits_attribute() {
        let ir = ir_with_apply(|c, f| mark_willreturn(c, f));
        assert!(
            ir.contains("willreturn"),
            "expected `willreturn` in IR, got:\n{ir}"
        );
    }

    #[test]
    fn mustprogress_helper_emits_attribute() {
        let ir = ir_with_apply(|c, f| mark_mustprogress(c, f));
        assert!(
            ir.contains("mustprogress"),
            "expected `mustprogress` in IR, got:\n{ir}"
        );
    }

    #[test]
    fn nofree_helper_emits_attribute() {
        let ir = ir_with_apply(|c, f| mark_nofree(c, f));
        assert!(ir.contains("nofree"), "expected `nofree` in IR, got:\n{ir}");
    }

    /// `mark_extern_canonical` is the bundle every libc / pooled-
    /// alloc / pooled-free declaration in `declares.rs` calls.
    /// Verify all four pieces (nounwind, willreturn, mustprogress,
    /// memory(<encoded>)) land on the same fn under LLVM 22.
    #[test]
    fn extern_canonical_bundle_argmem_readwrite() {
        let ir = ir_with_apply(|c, f| mark_extern_canonical(c, f, 3));
        for needle in [
            "nounwind",
            "willreturn",
            "mustprogress",
            "memory(argmem: readwrite)",
        ] {
            assert!(
                ir.contains(needle),
                "expected `{needle}` in IR after mark_extern_canonical(encoded=3), got:\n{ir}"
            );
        }
    }
}

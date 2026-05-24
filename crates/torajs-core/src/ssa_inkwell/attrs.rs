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
    /* LLVM 22's memory effect attribute encodes (location, mod-ref)
     * pairs into a u64. memory(none) is the all-zero bitmask. */
    let kind = Attribute::get_named_enum_kind_id("memory");
    let attr = ctx.create_enum_attribute(kind, 0);
    f.add_attribute(AttributeLoc::Function, attr);
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

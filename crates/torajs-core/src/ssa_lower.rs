#![allow(dead_code)] // step 2: minimum-scope lowerer; some helpers used by step 2.x onward

// AST → SSA lowerer (P3.5.a step 2).
//
// Scope of this step: just enough to lower fib40.tora.ts. That means:
//   - Top-level `Stmt::FnDecl` → `ssa::Function`
//   - `Stmt::If { else? }` → CondBr with no-else fall-through to merge block
//   - `Stmt::Return(expr?)` → Terminator::Ret
//   - `Stmt::Block`, `Stmt::Expr` (for chained calls)
//   - `Expr::Number` (i64 only — no f64 narrowing yet), `Bool`, `Ident`
//   - `Expr::BinOp` for the arith / compare / bitwise ops in the AST
//   - `Expr::Call { callee: Ident("...") }` resolving to a same-module FnDecl
//
// Deferred to step 2.x:
//   - `Stmt::LetDecl` + `Stmt::While` + `Expr::Assign` (need phi nodes)
//   - f64 numeric narrowing (number → f64 vs i64)
//   - `console.log(...)` at top level + a synthesized `main()` (step 3 wires
//     this when the Inkwell backend lands; right now `tr ssa` ignores
//     non-FnDecl top-level statements)
//   - Member-call resolution (only `Ident("...")` callees handled here)
//
// On unsupported shapes we panic with a clear message — labs material, not a
// user-facing tool yet. Will switch to a Result<_, LowerError> path when this
// is wired into a full `tr build` driver.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId};
use crate::ssa::{Module, Type, ValueId};
pub(crate) use crate::ssa_lower_ctx_struct::LowerCtx;
pub(crate) use crate::ssa_lower_deep_clone::deep_clone_stmt;
pub(crate) use crate::ssa_lower_env_drop_and_ret_ty::{effective_ret_ty, synthesize_env_drop};
pub(crate) use crate::ssa_lower_generics_monomorph::substitute_in_ann;
use crate::ssa_lower_inner::lower_inner;
pub(crate) use crate::ssa_lower_intrinsics::Intrinsics;
pub(crate) use crate::ssa_lower_synthesize_main::{declare_intrinsic, synthesize_main};

/// Phase 2B refcount: every heap-allocated Obj reserves a 32-byte
/// header:
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — class tag (u64-slot; low 32 bits = per-class id, high
///               32 reserved)
///   offset 16 — vtable pointer (per-class const global; null for plain
///               `type` aliases)
///   offset 24 — props dynobj ptr (RFC 20260714-struct-dynamic-props
///               blade 1 — lazily-allocated expando/tombstone shadow;
///               NULL = no dynamic property was ever written. Same
///               +24 slot Arr (`ARR_PROPS_OFF`) and Closure
///               (`CLOSURE_PROPS_OFF`) carry, so the three growable
///               cells share one convention. Fixed offset — an
///               anonymous struct (class_tag 0, no layout row) still
///               reaches its props without a table lookup.)
/// Field 0 lives at `OBJ_HEADER_SIZE`, field i at
/// `OBJ_HEADER_SIZE + i*8`. Closure env layout is unaffected — it has
/// its own fn-ptr header at offset 0 and lives in a separate alloc path.
///
/// Runtime mirrors that must move in lockstep (each self-documents):
/// `torajs-anyvalue/src/arg_struct_coerce.rs` OBJ_HEADER_SIZE,
/// `torajs-promise/src/layout.rs` ALLSETTLED_OBJ_HEADER_SIZE,
/// `torajs-throw/src/uncaught.rs` + `torajs-meta/src/error_to_string.rs`
/// OBJ_MESSAGE_OFF / OBJ_NAME_OFF.
pub(crate) const OBJ_HEADER_SIZE: u64 = 32;
pub(crate) const OBJ_CLASS_TAG_OFF: u64 = 8;
pub(crate) const OBJ_VTABLE_OFF: u64 = 16;
/// Inline props-dynobj slot (blade 1). NULL until the first dynamic
/// write through the `any` lane; blade 2 wires the write side.
pub(crate) const OBJ_PROPS_OFF: u64 = 24;

/// Phase 2A refcount + T-13.5 deque layout (mirrors the `ARR_HDR_*`
/// constants in torajs-arr):
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — len (u64)
///   offset 16 — cap (u32)
///   offset 20 — head (u32) — physical-slot offset of logical[0]; O(1) shift
///   offset 24 — props_dynobj (Round 4 chunk 5a)
///   offset 32 — slot data (N * 8 bytes physical capacity)
///
/// Logical index i lives at physical offset `32 + (head + i) * 8`.
/// Sites that access elements on a possibly-shifted array (Index, drop
/// walk, inc walk, pop) must add `head*8` to the byte offset; sites that
/// operate on freshly-allocated arrays (literal init, freshly-built dst
/// in concat/slice/spread) can skip the head load since head=0 there.
pub(crate) const ARR_LEN_OFF: u64 = 8;
const ARR_HEAD_OFF: u64 = 20;
/// Inline `props_dynobj` slot (Round 4 chunk 5a). Mirrors
/// `torajs_arr::layout::ARR_PROPS_OFF`. NULL when no `arr.x = v`
/// was ever written; chunk 5b+ flips arrprops_set to inline it.
#[allow(dead_code)]
pub(crate) const ARR_PROPS_OFF: u64 = 24;
/// Data-pointer field offset (RFC 20260706-arr-grow-alias-stability
/// B1) — slots live behind the pointer stored here; the cell is
/// fixed across grow. Cross-crate sync:
/// `torajs_arr::layout::ARR_DATA_PTR_OFF` +
/// `torajs_cycle::arr::ARR_DATA_PTR_OFF` +
/// `torajs_promise::layout::ARR_DATA_PTR_OFF` must equal this.
pub(crate) const ARR_DATA_PTR_OFF: u64 = 32;
/// Fixed cell size — the inline slots region a fresh alloc's data
/// pointer targets starts here (`torajs_arr::layout::ARR_CELL_SIZE`).
pub(crate) const ARR_CELL_SIZE: u64 = 40;

/// Phase 2C refcount: Closure env layout:
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — fn_addr (entry point, native env-first signature)
///   offset 16 — drop_fn  (per-closure cleanup, populated in Pass 2.5)
///   offset 24 — props_dynobj  (T-27 — Function as Object property bag,
///                              NULL until first `f.x = v` write; lazy-
///                              alloc'd dynobj per ECMAScript §10.2)
///   offset 32 — boxed_entry  (Any-method-call RFC 20260704 C3 — the
///                             synthesized `(env, argv, argc) -> AnyValue`
///                             dual entry for dynamic any-world calls;
///                             V8 JSFunction code-entry analogue. 0 =
///                             not dynamically callable)
///   offset 40 — trace_fn  (RFC 20260717 closure-env-cycle — the
///                          per-closure `__env_trace_<fn>` twin of
///                          drop_fn the cycle collector walks capture
///                          slots through; CPython tp_traverse shape.
///                          0 = stub until Pass 2.5 synthesis lands)
///   offset 48 — cap0
///   offset 56 — cap1
///   ...
///
/// `__torajs_obj_alloc` stays the underlying allocator (plain malloc);
/// the lowerer writes the universal header at the closure construction
/// site via `emit_obj_header_init` adapted for type_tag=CLOSURE.
pub const CLOSURE_FN_ADDR_OFF: u64 = 8;
pub const CLOSURE_DROP_FN_OFF: u64 = 16;
pub(crate) const CLOSURE_PROPS_OFF: u64 = 24;
pub(crate) const CLOSURE_BOXED_ENTRY_OFF: u64 = 32;
pub(crate) const CLOSURE_TRACE_FN_OFF: u64 = 40;
pub(crate) const CLOSURE_CAP_BASE_OFF: u64 = 48;

/// M3 — generic call-site retargeting. For each `Expr::Call` whose ExprId
/// is a generic call site, the typechecker has already inferred the
/// concrete type args; this map remembers the **specialized fn name** the
/// monomorphization pre-pass picked for that call site, so the lowerer's
/// `Expr::Call` arm rewrites the callee to point at the specialized fn.
pub(crate) type CallRetargets = HashMap<ExprId, String>;

/// V3-18 m2.b — per-namespace known-own-property table for the
/// hasOwnProperty / propertyIsEnumerable subset stub. Only literal-
/// string keys land in this lookup; runtime keys default to `false`.
/// T-28 — lower with the per-Call arity-pad map. Pads missing trailing
/// args with ANY_UNDEF Any-box operands at the call site for fns whose
/// trailing missing params are Type::Any. ssa_lower's Expr::Call arm
/// reads this and emits the padding before invoking the callee.
pub fn lower_with_arity(ast: &Ast, artifacts: &crate::check::CheckArtifacts) -> Module {
    let (
        _generic_call_sites,
        expr_types,
        arity_pad_count,
        demoted_cm_rewrites,
        contextual_any,
        mono,
    ) = artifacts;
    let _ = ast;
    lower_inner(
        expr_types,
        arity_pad_count,
        demoted_cm_rewrites,
        contextual_any,
        mono,
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalInfo {
    /// Pointer to the alloca slot — Type::Ptr.
    pub(crate) slot: ValueId,
    /// Type of the slot's *contents* (what Load returns).
    pub(crate) ty: Type,
    /// True after the binding's value has been consumed. Drop emission at
    /// fn-end skips moved locals.
    pub(crate) moved: bool,
    /// True if the binding never owned its heap value — it aliases a
    /// reference whose canonical owner lives elsewhere: non-Copy
    /// params (caller owns), closure captures (env owns), for-of
    /// bindings (the iterated container / iterator step owns),
    /// alias-init lets (`let v = o.f` / cross-scope `let n = s` —
    /// source owns). Distinct from `moved`: an owned local becomes
    /// `moved: true` when consumed, but `borrowed` is set at birth
    /// and never changes. `Stmt::Return` uses it to retain (+1) a
    /// returned borrow so the caller receives the owned reference
    /// the call-result convention promises.
    pub(crate) borrowed: bool,
    /// Lexical scope depth this binding was declared at. 0 = fn-root,
    /// each enclosing `Block` increments. Used by M1.3 to (a) drop
    /// inner-block locals at the closing `}` and (b) prevent cross-
    /// scope `let n = s` from transferring ownership (would dangle the
    /// outer-scope reference); see LetDecl in lower_stmt for the rule.
    pub(crate) scope_depth: usize,
}

pub(crate) use crate::ssa_lower_free_helpers::{count_capture_groups, decode_env_ann};
pub(crate) use crate::ssa_lower_interners::{intern_arr_layout, intern_fn_sig};
pub(crate) use crate::ssa_lower_parse_type::parse_type;

// P11.1-S2-a build-time encoding lives on `ssa::StringLiteral` as
// `StringLiteral::encode_from_str` — see crates/torajs-core/src/
// ssa/module_methods.rs.

/// v0.6+1 perf checkpoint — per-array hoisted state for the push-loop
/// pre-reserve fast-push. See `LowerCtx::push_unchecked_for`.
#[derive(Clone, Copy)]
pub(crate) struct PreReserveState {
    /// The array's cell pointer at loop entry. B1: the cell is fixed
    /// across grow, so this stays valid; used for the post-loop
    /// len-writeback.
    pub(crate) arr_ptr: ValueId,
    /// The data buffer pointer loaded once after `arr_reserve` — the
    /// StoreDyn base for the fast-path slot writes. Loop-invariant
    /// because the reserve guarantees no grow happens mid-loop (a
    /// grow would swap the buffer and stale this).
    pub(crate) data_ptr: ValueId,
    /// Pre-computed `head_x8` — the byte offset from `data_ptr` to
    /// slot[0]. Loop-invariant since the pattern detector excludes
    /// any body that could shift/unshift the array.
    pub(crate) head_off: ValueId,
    /// Local alloca'd i64 holding the running length. Initialized
    /// to the array's len at loop entry; bumped per push; written
    /// back to the array's len field at loop exit. mem2reg promotes
    /// this to a phi-register at -O1+.
    pub(crate) len_slot: ValueId,
}

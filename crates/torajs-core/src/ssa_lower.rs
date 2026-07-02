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

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa::{self, FuncId, InstKind, Module, Operand, Terminator, Type, ValueId};
pub(crate) use crate::ssa_lower_ctx_struct::LowerCtx;
pub(crate) use crate::ssa_lower_deep_clone::deep_clone_stmt;
pub(crate) use crate::ssa_lower_env_drop_and_ret_ty::{effective_ret_ty, synthesize_env_drop};
pub(crate) use crate::ssa_lower_generics_monomorph::{monomorphize_generics, substitute_in_ann};
use crate::ssa_lower_inner::lower_inner;
use crate::ssa_lower_push_loop_detect::let_counter_zero_name;
pub(crate) use crate::ssa_lower_rewrite_inner_generics::rewrite_inner_generic_calls;
pub(crate) use crate::ssa_lower_synthesize_main::{declare_intrinsic, synthesize_main};
use crate::ssa_lower_while_push_fast::lower_while_inner;

/// Phase 2B refcount: every heap-allocated Obj reserves a 24-byte
/// header:
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — class tag (u64-slot; low 32 bits = per-class id, high
///               32 reserved)
///   offset 16 — vtable pointer (per-class const global; null for plain
///               `type` aliases)
/// Field 0 lives at `OBJ_HEADER_SIZE`, field i at
/// `OBJ_HEADER_SIZE + i*8`. Closure env layout is unaffected — it has
/// its own fn-ptr header at offset 0 and lives in a separate alloc path.
pub(crate) const OBJ_HEADER_SIZE: u64 = 24;
pub(crate) const OBJ_CLASS_TAG_OFF: u64 = 8;
pub(crate) const OBJ_VTABLE_OFF: u64 = 16;

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
/// Slot data offset — bumped 24 → 32 in Round 4 chunk 5a so the
/// new `props_dynobj` u64 fits at offset 24. Cross-crate sync:
/// `torajs_arr::layout::ARR_SLOTS_OFF` + `torajs_str::split::pool::ARR_HDR_SIZE`
/// must equal this.
pub(crate) const ARR_DATA_OFF: u64 = 32;

/// Phase 2C refcount: Closure env layout:
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — fn_addr (entry point)
///   offset 16 — drop_fn  (per-closure cleanup, populated in Pass 2.5)
///   offset 24 — props_dynobj  (T-27 — Function as Object property bag,
///                              NULL until first `f.x = v` write; lazy-
///                              alloc'd dynobj per ECMAScript §10.2)
///   offset 32 — cap0
///   offset 40 — cap1
///   ...
///
/// `__torajs_obj_alloc` stays the underlying allocator (plain malloc);
/// the lowerer writes the universal header at the closure construction
/// site via `emit_obj_header_init` adapted for type_tag=CLOSURE.
pub const CLOSURE_FN_ADDR_OFF: u64 = 8;
pub const CLOSURE_DROP_FN_OFF: u64 = 16;
pub(crate) const CLOSURE_PROPS_OFF: u64 = 24;
pub(crate) const CLOSURE_CAP_BASE_OFF: u64 = 32;

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
    let (generic_call_sites, expr_types, arity_pad_count, demoted_cm_rewrites) = artifacts;
    lower_inner(
        ast,
        generic_call_sites,
        expr_types,
        arity_pad_count,
        demoted_cm_rewrites,
    )
}

/// FuncIds of every backend-provided runtime entry point. Threaded through
/// every lowering site that needs to emit a runtime call. Single struct so
/// adding a new intrinsic later (e.g. `__torajs_str_concat` for P2.2.c)
/// only touches one type signature.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Intrinsics {
    /// Per-call-site trivial closure-wrapper drop. (FuncId, SigId).
    /// Used by the Return arm when wrapping a top-level FnAddr into
    /// a Closure-typed value to satisfy a fn signature returning
    /// `(...) => R` from a non-capturing return path. The wrapper
    /// env has just `fn_addr@0 + drop_fn@8` and no captures; the
    /// drop body just frees the env block.
    pub(crate) env_drop_trivial: (FuncId, ssa::SigId),
    pub(crate) print_i64: FuncId,
    pub(crate) print_f64: FuncId,
    pub(crate) print_bool: FuncId,
    pub(crate) str_alloc: FuncId,
    pub(crate) str_print: FuncId,
    pub(crate) str_drop: FuncId,
    pub(crate) str_concat: FuncId,
    /// Phase B refcount — `__torajs_rc_inc(ptr)` increments the heap
    /// header's refcount (NULL passes through). Emitted at every
    /// slot-copy / shared-ownership site for non-Copy heap values.
    pub(crate) rc_inc: FuncId,
    pub(crate) obj_alloc: FuncId,
    pub(crate) capture_box_alloc: FuncId,
    pub(crate) capture_box_inc: FuncId,
    pub(crate) capture_box_drop: FuncId,
    pub(crate) obj_drop_sized: FuncId,
    pub(crate) value_drop_heap: FuncId,
    pub(crate) cycle_unbuffer: FuncId,
    pub(crate) arr_alloc: FuncId,
    pub(crate) arr_push: FuncId,
    pub(crate) arr_push_non_deque: FuncId,
    pub(crate) arr_shift: FuncId,
    pub(crate) arr_unshift: FuncId,
    pub(crate) arr_splice: FuncId,
    pub(crate) arr_drop: FuncId,
    pub(crate) arr_reserve: FuncId,
    pub(crate) arr_push_unchecked: FuncId,
    pub(crate) arr_extend_unchecked: FuncId,
    pub(crate) arr_slice: FuncId,
    pub(crate) str_repeat: FuncId,
    pub(crate) str_to_upper: FuncId,
    pub(crate) str_to_lower: FuncId,
    pub(crate) str_trim: FuncId,
    pub(crate) str_trim_start: FuncId,
    pub(crate) str_trim_end: FuncId,
    pub(crate) str_pad_start: FuncId,
    pub(crate) str_pad_end: FuncId,
    pub(crate) str_from_char_code: FuncId,
    pub(crate) str_from_code_point: FuncId,
    pub(crate) str_normalize: FuncId,
    pub(crate) str_at: FuncId,
    pub(crate) str_replace: FuncId,
    pub(crate) str_replace_all: FuncId,
    pub(crate) num_to_fixed_f: FuncId,
    pub(crate) num_to_fixed_i: FuncId,
    pub(crate) num_to_string_radix_i: FuncId,
    pub(crate) num_to_string_radix_f: FuncId,
    pub(crate) num_to_exp_f: FuncId,
    pub(crate) num_to_exp_i: FuncId,
    pub(crate) num_to_precision_f: FuncId,
    pub(crate) num_to_precision_i: FuncId,
    pub(crate) num_to_locale_f: FuncId,
    pub(crate) num_to_locale_i: FuncId,
    pub(crate) num_parse_int: FuncId,
    pub(crate) num_parse_float: FuncId,
    pub(crate) num_is_integer_f: FuncId,
    pub(crate) num_is_integer_i: FuncId,
    pub(crate) num_is_nan_f: FuncId,
    pub(crate) num_is_nan_i: FuncId,
    pub(crate) num_is_finite_f: FuncId,
    pub(crate) num_is_finite_i: FuncId,
    pub(crate) num_is_safe_integer_f: FuncId,
    pub(crate) num_is_safe_integer_i: FuncId,
    pub(crate) num_is_integer_any: FuncId,
    pub(crate) num_is_nan_any: FuncId,
    pub(crate) num_is_finite_any: FuncId,
    pub(crate) num_is_safe_integer_any: FuncId,
    pub(crate) str_slice: FuncId,
    pub(crate) str_char_code_at: FuncId,
    pub(crate) str_code_point_at: FuncId,
    pub(crate) str_starts_with: FuncId,
    pub(crate) str_ends_with: FuncId,
    pub(crate) str_index_of: FuncId,
    pub(crate) str_last_index_of: FuncId,
    pub(crate) str_locale_compare: FuncId,
    pub(crate) str_includes: FuncId,
    pub(crate) str_eq: FuncId,
    pub(crate) str_split: FuncId,
    pub(crate) str_split_no_sep: FuncId,
    /// Phase Substr.A — substring view runtime helpers.
    pub(crate) substr_create: FuncId,
    pub(crate) substr_drop: FuncId,
    pub(crate) substr_char_code_at: FuncId,
    pub(crate) substr_code_point_at: FuncId,
    pub(crate) substr_eq_str: FuncId,
    pub(crate) substr_to_owned: FuncId,
    pub(crate) substr_starts_with: FuncId,
    pub(crate) substr_ends_with: FuncId,
    pub(crate) substr_includes: FuncId,
    pub(crate) substr_index_of: FuncId,
    pub(crate) substr_slice: FuncId,
    pub(crate) substr_substring: FuncId,
    pub(crate) substr_trim: FuncId,
    pub(crate) substr_trim_into: FuncId,
    pub(crate) substr_trim_start: FuncId,
    pub(crate) substr_trim_end: FuncId,
    pub(crate) substr_concat_substr_str: FuncId,
    pub(crate) substr_concat_str_substr: FuncId,
    pub(crate) substr_concat_substr_substr: FuncId,
    /// v0.2 #1 — regex matching engine. `regex_compile` parses the
    /// pattern + flag string at runtime into an NFA + flag bitset
    /// (Thompson construction); `regex_test` runs the backtracking
    /// matcher against a string and returns 1/0. Subsequent surface
    /// methods (`s.match`, `s.replace`, `re.exec`, ...) land in
    /// follow-up sub-phases as more `__torajs_regex_*` helpers.
    pub(crate) regex_compile: FuncId,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-4 — AOT-baked DFA
    /// variant. See the declare site for the contract.
    pub(crate) regex_compile_from_static_dfa: FuncId,
    pub(crate) regex_test: FuncId,
    pub(crate) regex_get_source: FuncId,
    pub(crate) regex_get_flags: FuncId,
    pub(crate) regex_to_string: FuncId,
    pub(crate) regex_has_flag: FuncId,
    pub(crate) regex_drop: FuncId,
    pub(crate) regex_match: FuncId,
    pub(crate) regex_replace: FuncId,
    pub(crate) regex_replace_all: FuncId,
    /// P9.5-A1 — fn-callback variants of `s.replace(re, fn)` /
    /// `s.replaceAll(re, fn)`. 3rd arg is the closure env block (env+8
    /// = lifted body's fn_addr). Runtime invokes (env, match_str) ->
    /// ret_str per match.
    pub(crate) regex_replace_fn: FuncId,
    pub(crate) regex_replace_all_fn: FuncId,
    pub(crate) regex_split: FuncId,
    pub(crate) regex_exec: FuncId,
    pub(crate) regex_match_all: FuncId,
    /// P9.4 — `RegExp.prototype.lastIndex` accessors. Get returns the
    /// raw int64 field on the RegExp heap object; set stores it
    /// without coercion (typed-tier passes integer literals or
    /// arithmetic results that already lower to I64).
    pub(crate) regex_get_last_index: FuncId,
    pub(crate) regex_set_last_index: FuncId,
    pub(crate) date_now: FuncId,
    pub(crate) date_from_ms: FuncId,
    pub(crate) date_drop: FuncId,
    pub(crate) date_now_static: FuncId,
    pub(crate) date_get_time: FuncId,
    pub(crate) date_to_iso_string: FuncId,
    pub(crate) date_set_time: FuncId,
    pub(crate) date_get_year: FuncId,
    pub(crate) date_set_year: FuncId,
    pub(crate) date_to_gmt_string: FuncId,
    pub(crate) date_to_date_string: FuncId,
    pub(crate) date_to_locale_string: FuncId,
    pub(crate) date_to_locale_date_string: FuncId,
    pub(crate) date_to_locale_time_string: FuncId,
    pub(crate) date_set_full_year: FuncId,
    pub(crate) date_set_month: FuncId,
    pub(crate) date_set_date: FuncId,
    pub(crate) date_set_hours: FuncId,
    pub(crate) date_set_minutes: FuncId,
    pub(crate) date_set_seconds: FuncId,
    pub(crate) date_set_milliseconds: FuncId,
    pub(crate) date_get_full_year: FuncId,
    pub(crate) date_get_month: FuncId,
    pub(crate) date_get_date: FuncId,
    pub(crate) date_get_hours: FuncId,
    pub(crate) date_get_minutes: FuncId,
    pub(crate) date_get_seconds: FuncId,
    pub(crate) date_get_milliseconds: FuncId,
    pub(crate) date_get_day: FuncId,
    pub(crate) date_get_timezone_offset: FuncId,
    pub(crate) date_get_utc_full_year: FuncId,
    pub(crate) date_get_utc_month: FuncId,
    pub(crate) date_get_utc_date: FuncId,
    pub(crate) date_get_utc_hours: FuncId,
    pub(crate) date_get_utc_minutes: FuncId,
    pub(crate) date_get_utc_seconds: FuncId,
    pub(crate) date_get_utc_milliseconds: FuncId,
    pub(crate) date_get_utc_day: FuncId,
    pub(crate) date_from_components: FuncId,
    pub(crate) date_utc_components: FuncId,
    pub(crate) date_from_iso: FuncId,
    pub(crate) date_parse_iso: FuncId,
    pub(crate) fs_read_file_sync: FuncId,
    pub(crate) fs_write_file_sync: FuncId,
    pub(crate) fs_exists_sync: FuncId,
    pub(crate) fs_append_file_sync: FuncId,
    pub(crate) fs_unlink_sync: FuncId,
    pub(crate) fs_mkdir_sync: FuncId,
    pub(crate) fs_readdir_sync: FuncId,
    pub(crate) fs_size_sync: FuncId,
    pub(crate) process_exit: FuncId,
    pub(crate) process_cwd: FuncId,
    pub(crate) process_platform: FuncId,
    pub(crate) process_getenv: FuncId,
    pub(crate) argv_init: FuncId,
    pub(crate) process_argv: FuncId,
    pub(crate) process_stdout_write: FuncId,
    pub(crate) process_stderr_write: FuncId,
    pub(crate) arr_alloc_any: FuncId,
    pub(crate) arr_push_any: FuncId,
    pub(crate) arr_fill_any: FuncId,
    pub(crate) arr_extend_any: FuncId,
    pub(crate) arr_set_any: FuncId,
    pub(crate) arr_set_any_grow: FuncId,
    pub(crate) arr_oob_write_reject: FuncId,
    pub(crate) arr_get_any_tag: FuncId,
    pub(crate) arr_get_any_value: FuncId,
    pub(crate) dynobj_alloc: FuncId,
    pub(crate) get_builtin_prototype: FuncId,
    pub(crate) instanceof_class_any_tag: FuncId,
    pub(crate) instanceof_builtin_any_tag: FuncId,
    pub(crate) instanceof_object_any: FuncId,
    pub(crate) in_op_any_num: FuncId,
    pub(crate) in_op_any_str: FuncId,
    pub(crate) any_is_arr: FuncId,
    pub(crate) fnprops_set: FuncId,
    pub(crate) fnprops_get_tag: FuncId,
    pub(crate) fnprops_get_value: FuncId,
    pub(crate) arrprops_set: FuncId,
    pub(crate) arrprops_get_tag: FuncId,
    pub(crate) arrprops_get_value: FuncId,
    pub(crate) dynobj_get_tag: FuncId,
    pub(crate) dynobj_get_value: FuncId,
    pub(crate) dynobj_set: FuncId,
    pub(crate) dynobj_define: FuncId,
    pub(crate) dynobj_define_from_desc: FuncId,
    pub(crate) accessor_pair_new: FuncId,
    pub(crate) accessor_invoke_getter: FuncId,
    pub(crate) get_property_descriptor: FuncId,
    pub(crate) throw_typeerror_if_not_object: FuncId,
    pub(crate) arr_throw_reduce_empty: FuncId,
    pub(crate) arr_throw_reduce_right_empty: FuncId,
    pub(crate) arr_length_descriptor: FuncId,
    pub(crate) str_length_descriptor: FuncId,
    pub(crate) arr_index_strs: FuncId,
    pub(crate) str_index_strs: FuncId,
    pub(crate) arr_keys_only: FuncId,
    pub(crate) str_keys_only: FuncId,
    pub(crate) str_to_char_arr: FuncId,
    pub(crate) arr_entries_by_tag: FuncId,
    pub(crate) str_entries: FuncId,
    pub(crate) anyv_struct_keys: FuncId,
    pub(crate) anyv_struct_values: FuncId,
    pub(crate) anyv_struct_entries: FuncId,
    pub(crate) str_index_descriptor: FuncId,
    pub(crate) anyv_prevent_extensions: FuncId,
    pub(crate) anyv_is_extensible: FuncId,
    pub(crate) anyv_seal: FuncId,
    pub(crate) anyv_is_sealed: FuncId,
    pub(crate) dynobj_has: FuncId,
    pub(crate) dynobj_delete: FuncId,
    pub(crate) arr_drop_any: FuncId,
    pub(crate) any_box: FuncId,
    pub(crate) any_payload_rc_inc: FuncId,
    pub(crate) proto_register: FuncId,
    pub(crate) register_native_error: FuncId,
    pub(crate) proto_get: FuncId,
    pub(crate) class_register: FuncId,
    pub(crate) class_get: FuncId,
    pub(crate) get_proto_of_any: FuncId,
    pub(crate) any_typeof: FuncId,
    pub(crate) any_to_bool: FuncId,
    pub(crate) any_to_number: FuncId,
    pub(crate) any_add: FuncId,
    pub(crate) any_arith: FuncId,
    pub(crate) any_compare: FuncId,
    pub(crate) any_strict_eq: FuncId,
    pub(crate) any_any_strict_eq: FuncId,
    pub(crate) any_unbox_tag: FuncId,
    pub(crate) any_unbox_value: FuncId,
    pub(crate) any_box_drop: FuncId,
    pub(crate) print_any: FuncId,
    pub(crate) print_any_inline_top: FuncId,
    pub(crate) io_putc_stdout: FuncId,
    pub(crate) arr_print_i64_inline: FuncId,
    pub(crate) arr_print_f64_inline: FuncId,
    pub(crate) arr_print_bool_inline: FuncId,
    pub(crate) arr_print_str_inline: FuncId,
    pub(crate) arr_print_substr_inline: FuncId,
    pub(crate) map_print_outer: FuncId,
    pub(crate) set_print_outer: FuncId,
    pub(crate) fn_print_outer: FuncId,
    pub(crate) any_to_str: FuncId,
    pub(crate) obj_freeze: FuncId,
    pub(crate) obj_is_frozen: FuncId,
    pub(crate) obj_is_frozen_any: FuncId,
    pub(crate) obj_check_not_frozen: FuncId,
    /// v0.5 T-15.e — drains the microtask queue. Auto-called at
    /// main exit so chained Promise callbacks run before the
    /// process returns.
    pub(crate) microtask_drain: FuncId,
    /// P10.1-A1 — queueMicrotask(cb) closure-path enqueue. Takes
    /// the closure env pointer; runtime rc-inc's + dispatches at
    /// the next drain. cb's ABI is `(env*) -> void` (mirrors
    /// finally_closure).
    pub(crate) microtask_enqueue_closure: FuncId,
    /// P10.1-A1.1 — queueMicrotask(cb) simple-fn (no-env) enqueue.
    /// Takes a raw fn pointer; runtime dispatcher casts back to
    /// `void ()` and invokes. No rc (fn pointers are not heap
    /// objects). Selection happens at the queueMicrotask call
    /// site based on cb's static type (Type::Closure → _closure,
    /// Type::FnSig → this one), mirroring promise_then dispatch.
    pub(crate) microtask_enqueue_simple: FuncId,
    /// v0.5 T-15.g — Promise.resolve / Promise.reject runtime
    /// constructors + drop. The arg value is i64-packed (heap-ptr
    /// cast, bool widened, f64 bitcast).
    pub(crate) promise_alloc_fulfilled: FuncId,
    pub(crate) promise_resolve_thenable: FuncId,
    pub(crate) promise_alloc_rejected: FuncId,
    pub(crate) promise_alloc_fulfilled_heap: FuncId,
    pub(crate) promise_alloc_rejected_heap: FuncId,
    pub(crate) promise_drop: FuncId,
    pub(crate) promise_get_value: FuncId,
    pub(crate) promise_then_simple: FuncId,
    pub(crate) promise_then_closure: FuncId,
    pub(crate) promise_catch_simple: FuncId,
    pub(crate) promise_finally: FuncId,
    pub(crate) promise_catch_closure: FuncId,
    pub(crate) promise_finally_closure: FuncId,
    pub(crate) fetch_sync: FuncId,
    pub(crate) promise_all_sync: FuncId,
    pub(crate) promise_race_sync: FuncId,
    pub(crate) promise_any_sync: FuncId,
    pub(crate) promise_allsettled_sync: FuncId,
    pub(crate) bigint_from_decimal: FuncId,
    pub(crate) bigint_from_hex: FuncId,
    pub(crate) bigint_add: FuncId,
    pub(crate) bigint_sub: FuncId,
    pub(crate) bigint_mul: FuncId,
    pub(crate) bigint_div: FuncId,
    pub(crate) bigint_mod: FuncId,
    pub(crate) bigint_pow: FuncId,
    pub(crate) bigint_and: FuncId,
    pub(crate) bigint_or: FuncId,
    pub(crate) bigint_xor: FuncId,
    pub(crate) bigint_not: FuncId,
    pub(crate) bigint_shl: FuncId,
    pub(crate) bigint_shr: FuncId,
    pub(crate) bigint_from_str: FuncId,
    pub(crate) bigint_from_number: FuncId,
    pub(crate) bigint_clone: FuncId,
    pub(crate) bigint_neg: FuncId,
    pub(crate) bigint_cmp: FuncId,
    pub(crate) bigint_to_string: FuncId,
    pub(crate) bigint_to_string_radix: FuncId,
    pub(crate) bigint_as_int_n: FuncId,
    pub(crate) bigint_as_uint_n: FuncId,
    pub(crate) bigint_drop_rc: FuncId,
    pub(crate) weakref_create: FuncId,
    pub(crate) weakref_deref: FuncId,
    pub(crate) weakref_drop: FuncId,
    pub(crate) weakref_target_dying: FuncId,
    pub(crate) weakmap_create: FuncId,
    pub(crate) weakmap_set: FuncId,
    pub(crate) weakmap_get: FuncId,
    pub(crate) weakmap_has: FuncId,
    pub(crate) weakmap_delete: FuncId,
    pub(crate) weakmap_drop: FuncId,
    /* P6.1 — strong-ref Map<K,V> intrinsics. */
    pub(crate) map_create: FuncId,
    /* `__torajs_set_create()` — fresh Set heap (Map layout + TAG_SET).
     * Tag::Set discriminates Set from Map for the AnyValue tag-walker
     * (inspect.rs) so `const s: any = new Set()` console.log can route
     * to the bun `Set(N) {…}` printer instead of the Map walker. */
    pub(crate) set_create: FuncId,
    pub(crate) set_is_subset_of: FuncId,
    pub(crate) set_is_superset_of: FuncId,
    pub(crate) set_is_disjoint_from: FuncId,
    pub(crate) set_union: FuncId,
    pub(crate) set_intersection: FuncId,
    pub(crate) set_difference: FuncId,
    pub(crate) set_symmetric_difference: FuncId,
    pub(crate) map_clone: FuncId,
    pub(crate) map_set: FuncId,
    pub(crate) map_get: FuncId,
    pub(crate) map_has: FuncId,
    pub(crate) map_delete: FuncId,
    pub(crate) map_clear: FuncId,
    pub(crate) map_size: FuncId,
    pub(crate) map_drop: FuncId,
    pub(crate) map_iter_next: FuncId,
    pub(crate) map_iter_create_keys: FuncId,
    pub(crate) map_iter_create_values: FuncId,
    pub(crate) map_iter_create_entries: FuncId,
    pub(crate) map_iter_create_set_entries: FuncId,
    pub(crate) map_iter_step: FuncId,
    pub(crate) map_iter_drop: FuncId,
    pub(crate) arr_iter_create_keys: FuncId,
    pub(crate) arr_iter_create_values: FuncId,
    pub(crate) arr_iter_create_entries: FuncId,
    pub(crate) arr_iter_step: FuncId,
    pub(crate) arr_iter_drop: FuncId,
    pub(crate) weakset_create: FuncId,
    pub(crate) weakset_add: FuncId,
    pub(crate) weakset_has: FuncId,
    pub(crate) weakset_delete: FuncId,
    pub(crate) weakset_drop: FuncId,
    pub(crate) cycle_buffer: FuncId,
    pub(crate) cycle_at_exit_drain: FuncId,
    pub(crate) cycle_collect: FuncId,
    pub(crate) symbol_alloc: FuncId,
    pub(crate) symbol_drop: FuncId,
    pub(crate) symbol_print: FuncId,
    pub(crate) symbol_for: FuncId,
    pub(crate) symbol_key_for: FuncId,
    pub(crate) symbol_iterator: FuncId,
    pub(crate) symbol_async_iterator: FuncId,
    pub(crate) symbol_to_primitive: FuncId,
    pub(crate) object_is_f64: FuncId,
    pub(crate) split_iter_init: FuncId,
    pub(crate) split_iter_next: FuncId,
    pub(crate) split_iter_drop: FuncId,
    pub(crate) arr_from_string: FuncId,
    pub(crate) str_substring: FuncId,
    pub(crate) str_substr: FuncId,
    pub(crate) arr_set_length_validate: FuncId,
    pub(crate) arr_set_length_truncate_scalar: FuncId,
    pub(crate) arr_to_reversed: FuncId,
    pub(crate) arr_with: FuncId,
    pub(crate) arr_join: FuncId,
    pub(crate) arr_join_substr: FuncId,
    pub(crate) i64_to_str: FuncId,
    pub(crate) bool_to_str: FuncId,
    pub(crate) null_to_str: FuncId,
    pub(crate) undefined_to_str: FuncId,
    pub(crate) str_to_number: FuncId,
    pub(crate) arr_print_i64: FuncId,
    pub(crate) arr_print_f64: FuncId,
    pub(crate) arr_print_bool: FuncId,
    pub(crate) arr_print_str: FuncId,
    pub(crate) arr_print_substr: FuncId,
    pub(crate) substr_print: FuncId,
    pub(crate) str_char_at: FuncId,
    pub(crate) arr_join_i64: FuncId,
    pub(crate) arr_join_f64: FuncId,
    pub(crate) arr_join_i64_locale: FuncId,
    pub(crate) arr_join_f64_locale: FuncId,
    pub(crate) arr_join_bool: FuncId,
    pub(crate) arr_join_any: FuncId,
    pub(crate) symbol_to_str: FuncId,
    pub(crate) str_index_of_from: FuncId,
    pub(crate) str_last_index_of_from: FuncId,
    pub(crate) str_starts_with_from: FuncId,
    pub(crate) str_ends_with_from: FuncId,
    pub(crate) str_includes_from: FuncId,
    pub(crate) symbol_description: FuncId,
    pub(crate) f64_to_str: FuncId,
    pub(crate) math_sqrt: FuncId,
    pub(crate) math_abs: FuncId,
    pub(crate) math_floor: FuncId,
    pub(crate) math_ceil: FuncId,
    pub(crate) math_log: FuncId,
    pub(crate) math_exp: FuncId,
    pub(crate) math_sign: FuncId,
    pub(crate) math_round: FuncId,
    pub(crate) math_trunc: FuncId,
    pub(crate) math_pow: FuncId,
    pub(crate) math_min: FuncId,
    pub(crate) math_max: FuncId,
    pub(crate) math_sin: FuncId,
    pub(crate) math_cos: FuncId,
    pub(crate) math_tan: FuncId,
    pub(crate) math_asin: FuncId,
    pub(crate) math_acos: FuncId,
    pub(crate) math_atan: FuncId,
    pub(crate) math_atan2: FuncId,
    pub(crate) math_log2: FuncId,
    pub(crate) math_log10: FuncId,
    pub(crate) math_cbrt: FuncId,
    pub(crate) math_sinh: FuncId,
    pub(crate) math_cosh: FuncId,
    pub(crate) math_tanh: FuncId,
    pub(crate) math_asinh: FuncId,
    pub(crate) math_acosh: FuncId,
    pub(crate) math_atanh: FuncId,
    pub(crate) math_expm1: FuncId,
    pub(crate) math_log1p: FuncId,
    pub(crate) math_imul: FuncId,
    pub(crate) math_clz32: FuncId,
    pub(crate) math_fround: FuncId,
    pub(crate) math_f16round: FuncId,
    pub(crate) math_sum_precise: FuncId,
    pub(crate) math_sum_precise_i64: FuncId,
    pub(crate) math_random: FuncId,
    pub(crate) json_quote_str: FuncId,
    /// V0.2 P14-S5 — JSON builder fast path intrinsics for
    /// `JSON.stringify(struct)`. See `crates/torajs-str/src/
    /// json_builder.rs` and the `Type::Obj` arm of
    /// `lower_json_stringify`.
    pub(crate) jsb_new: FuncId,
    pub(crate) jsb_push_byte: FuncId,
    pub(crate) jsb_push_str_raw: FuncId,
    pub(crate) jsb_push_str_quoted: FuncId,
    pub(crate) jsb_push_i64: FuncId,
    pub(crate) jsb_push_bool: FuncId,
    pub(crate) jsb_finalize: FuncId,
    /// M6.3 — JSON.parse runtime helpers. See `runtime_str.c` for the
    /// per-helper contract. Cursor is `int64_t *`, threaded by the
    /// caller via an alloca slot; helpers advance it past the
    /// consumed token. Throws via `__torajs_throw_set` on mismatch.
    pub(crate) json_eat_char: FuncId,
    pub(crate) json_parse_int: FuncId,
    pub(crate) json_parse_float: FuncId,
    pub(crate) json_parse_bool: FuncId,
    pub(crate) json_parse_string: FuncId,
    pub(crate) json_arr_step: FuncId,
    pub(crate) json_arr_first: FuncId,
    pub(crate) str_eq_cstr: FuncId,
    pub(crate) print_i64_err: FuncId,
    pub(crate) print_f64_err: FuncId,
    pub(crate) print_bool_err: FuncId,
    pub(crate) str_print_err: FuncId,
    pub(crate) arr_flat: FuncId,
    pub(crate) arr_flat_any: FuncId,
    pub(crate) arr_extend_typed_into_any: FuncId,
    pub(crate) arr_concat: FuncId,
    pub(crate) arr_reverse: FuncId,
    pub(crate) arr_fill: FuncId,
    pub(crate) arr_copy_within: FuncId,
    /// M4 — exception state. `throw_set(value)` writes to module-level
    /// throw_active=1 + throw_value; `throw_check()` returns active flag;
    /// `throw_take()` reads value + clears flag. The backend defines the
    /// underlying globals.
    pub(crate) throw_set: FuncId,
    pub(crate) throw_check: FuncId,
    pub(crate) throw_take: FuncId,
    pub(crate) throw_take_tag: FuncId,
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
    /// The array's heap pointer at loop entry (= `arr_reserve`'s
    /// return). Used as the StoreDyn base + post-loop len-writeback
    /// target.
    pub(crate) arr_ptr: ValueId,
    /// Pre-computed `head_x8 + 24` — the byte offset from `arr_ptr`
    /// to slot[0]. Loop-invariant since the pattern detector
    /// excludes any body that could shift/unshift the array.
    pub(crate) head_off: ValueId,
    /// Local alloca'd i64 holding the running length. Initialized
    /// to the array's len at loop entry; bumped per push; written
    /// back to the array's len field at loop exit. mem2reg promotes
    /// this to a phi-register at -O1+.
    pub(crate) len_slot: ValueId,
}

impl<'a> LowerCtx<'a> {
    /// W1 — the num_width SlotKey for a let binding in the current fn.
    /// Top-level bindings key as Global regardless of whether Pass 1.5
    /// promoted them (the analysis keys every top-level let that way).
    pub(crate) fn num_width_local_key(&self, name: &str) -> crate::num_width::SlotKey {
        if self.is_main_fn {
            crate::num_width::SlotKey::Global(name.to_string())
        } else {
            crate::num_width::SlotKey::Local(self.f.name.clone(), name.to_string())
        }
    }

    /// True iff the current block hasn't been terminated yet (still has the
    /// default `Unreachable` placeholder). Used after lowering a sub-statement
    /// to decide whether we still need to emit a fall-through Br.
    pub(crate) fn cur_open(&self) -> bool {
        matches!(
            self.f.blocks[self.cur_block.0 as usize].term,
            Terminator::Unreachable
        )
    }

    /// 12-c-1 — route `while` through [`lower_while_inner`] with the
    /// let-zero counter derived from `prev`. Returns `true` iff `s`
    /// was a While; caller lowers non-Whiles normally. See the module
    /// doc on [`crate::ssa_lower_while_push_fast`].
    pub(crate) fn try_lower_while_fast(&mut self, prev: Option<&Stmt>, s: &Stmt) -> bool {
        let Stmt::While { cond, body } = s else {
            return false;
        };
        let counter = let_counter_zero_name(self.ast, prev);
        lower_while_inner(self, *cond, body, counter.as_deref());
        true
    }

    /// Coerce a value of any type to Type::Str. Used by multi-arg
    /// console.X to build a space-joined output line.
    /// `console.log` recognized as an Ident("console") + Member.name == "log".
    fn is_console_log_member(&self, eid: ExprId) -> bool {
        match self.ast.get_expr(eid) {
            Expr::Member { obj, name } if name == "log" => {
                matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "console")
            }
            _ => false,
        }
    }

    /// `JSON.stringify(value)` — type-aware serializer. Emits SSA for
    /// the static type of `val_op` and returns a fresh Type::Str
    /// operand containing the JSON encoding. Recursive: arrays loop +
    /// dispatch on element type; structs unfold field-by-field at
    /// compile time. Always single-pass — no second walk for length
    /// pre-computation; fragments accumulate via str_concat.
    pub(crate) fn lower_json_stringify(&mut self, val_op: Operand, ty: Type) -> Operand {
        crate::ssa_lower_json_stringify::lower(self, val_op, ty)
    }

    /// Emit drop sequences for every owned non-Copy local in the current
    /// block. Called immediately before terminators that exit the function
    /// (Ret, fall-through). Skips `moved` bindings — those have transferred
    /// ownership elsewhere and the receiver is responsible for the drop.
    pub(crate) fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Multi(stmts) => {
                // Compiler-generated sequence — share surrounding scope.
                // No scope push, no drop emission of its own. Each child
                // lowers as if it appeared at the parent site. Used by
                // parse-time desugars (destructuring, possibly others)
                // that need to emit multiple lets without burying them
                // in a child block.
                let mut prev: Option<&Stmt> = None;
                for s in stmts {
                    if !self.try_lower_while_fast(prev, s) {
                        self.lower_stmt(s);
                    }
                    if !self.cur_open() {
                        break;
                    }
                    prev = Some(s);
                }
            }
            Stmt::Block(stmts) => {
                crate::ssa_lower_stmt_block::lower(self, stmts);
            }
            Stmt::LetDecl {
                mutable: _,
                name,
                type_ann,
                init,
                is_var: false,
            } => {
                crate::ssa_lower_stmt_let_decl::lower(self, name, type_ann.as_ref(), *init);
            }
            Stmt::While { cond, body } => {
                lower_while_inner(self, *cond, body, None);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                crate::ssa_lower_stmt_for_of_split_iter::lower(self, var_name, *parent, *sep, body);
            }
            Stmt::ForOf {
                var_name,
                i_ident,
                elem_expr,
                body,
                ..
            } => {
                crate::ssa_lower_stmt_for_of::lower(self, var_name, i_ident, *elem_expr, body);
            }
            Stmt::DoWhile { body, cond } => {
                crate::ssa_lower_stmt_do_while::lower(self, body, *cond);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                crate::ssa_lower_stmt_switch::lower(self, *scrutinee, cases, default);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                crate::ssa_lower_stmt_for::lower(self, init.as_deref(), *cond, *step, body);
            }
            Stmt::Break => {
                crate::ssa_lower_stmt_break_continue::lower_break(self);
            }
            Stmt::Continue => {
                crate::ssa_lower_stmt_break_continue::lower_continue(self);
            }
            Stmt::Throw(eid) => {
                crate::ssa_lower_stmt_throw::lower(self, *eid);
            }
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                crate::ssa_lower_stmt_try::lower(
                    self,
                    body,
                    *had_catch,
                    catch_param.as_ref(),
                    catch_type.as_ref(),
                    catch_body,
                    finally_body.as_ref(),
                );
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                crate::ssa_lower_stmt_if::lower(self, *cond, then_branch, else_branch.as_deref());
            }
            Stmt::Return(maybe) => {
                crate::ssa_lower_stmt_return::lower(self, *maybe);
            }
            Stmt::Expr(eid) => {
                // Multi-arg `console.log` per-arg inspect dispatch —
                // shared with lower_top_stmt; without this the legacy
                // `coerce_to_str` joiner below would panic on typed
                // `Arr<T>` args inside try-body / nested block scopes.
                if crate::ssa_lower_console_log_multiarg::try_lower(self, s) {
                    return;
                }
                // Result discarded. Expression may still produce SSA insts as
                // side effects (its own value), e.g. nested Calls.
                let _ = self.lower_expr(*eid);
            }
            Stmt::TypeDecl { .. } => {
                // Pass 0 of `lower()` already registered the alias and
                // interned the layout. Re-encountering during the body
                // walk is a no-op.
            }
            Stmt::ImportDecl { .. } => {
                // K.1 single-file mode: no semantic effect. K.2 will
                // wire this into a cross-file symbol table.
            }
            Stmt::ExportDecl { .. } => {
                // K.1: `unwrap_exports` desugar should have flattened
                // the declaration-form. Bare named-export (`export {
                // ... }`) reaches here and is a no-op in single-file
                // mode.
            }
            other => {
                // Friendly classification for the most common shapes
                // that hit this catch-all so users get a readable
                // message instead of the raw AST debug print.
                let label = match other {
                    Stmt::FnDecl { name, .. } => format!(
                        "nested function declaration `{name}` inside a block / switch (planned: function-statement hoisting, see roadmap)"
                    ),
                    Stmt::ClassDecl { name, .. } => format!(
                        "nested class declaration `{name}` inside a block (planned: same hoisting story as nested functions)"
                    ),
                    _ => format!("statement shape not yet implemented: {other:?}"),
                };
                panic!("{label}");
            }
        }
    }

    /// Intern a string literal and return a Type::Str SSA value pointing at
    /// a fresh heap-allocated `{u64 len; u8 data[]}` copy. The static bytes
    /// live as a `[N x i8]` global (no NUL, len is explicit); `__torajs_str_alloc`
    /// copies them into a heap StrRepr at runtime. Every literal use does
    /// one alloc — caller is responsible for emitting Drop at scope end
    /// (P2.2.b.2 wires that up; this sub-step intentionally leaks one
    /// alloc per literal use, which is fine for one-shot bench programs).
    pub(crate) fn intern_string_literal(&mut self, s: &str) -> ValueId {
        // Phase P-rpn — every string-literal expression resolves to a
        // Str-shaped `StaticStrRef` global (rc_inc / rc_dec / free
        // all no-op via the STATIC_LITERAL flag). Encoding decision
        // happens in `StringLiteral::encode_from_str` (P11.1-S2-a).
        let lit = ssa::StringLiteral::encode_from_str(s);
        let sid = ssa::StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings.push(lit);
        self.f
            .append_inst(self.cur_block, InstKind::StaticStrRef(sid), Type::Str, None)
    }

    /// v0.3 #4 D-3 — outer wrapper that stamps every Inst emitted
    /// while lowering `eid` with `current_origin = Some(eid)` so
    /// debug-info emission can resolve the source span for DWARF.
    /// Recursive `self.lower_expr(...)` calls re-enter this wrapper
    /// so nested exprs get their own tighter origin scoped to the
    /// inner subtree (RAII-style save/restore on the prev value).
    pub(crate) fn lower_expr(&mut self, eid: ExprId) -> Operand {
        let prev = self.f.current_origin;
        self.f.current_origin = Some(eid);
        let result = self.lower_expr_inner(eid);
        self.f.current_origin = prev;
        result
    }

    fn lower_expr_inner(&mut self, eid: ExprId) -> Operand {
        crate::ssa_lower_expr_inner::lower(self, eid)
    }

    // (`lower_logical_and` / `lower_logical_or` live in
    // `ssa_lower_logical.rs`.)

    // (`coerce_to_bool` lives in `ssa_lower_logical.rs`.)

    /// Phase H.3.b — set of runtime class tags that satisfy `instanceof
    /// class_name`: `class_name` itself plus every transitively-extending
    /// subclass. Empty if `class_name` isn't a declared class. Same
    /// algorithm as instanceof's lower path, factored out so the
    /// `__dispatch_<M>` interception can reuse it.
    fn compute_descendant_tags(&self, class_name: &str) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        if !self.ast.class_parents.contains_key(class_name) {
            return out;
        }
        for c in self.ast.class_parents.keys() {
            let mut cur = Some(c.clone());
            let mut depth = 0u32;
            while let Some(name) = cur {
                if depth > 64 {
                    break;
                }
                if name == *class_name {
                    if let Some(tag) = self.class_name_to_tag.get(c) {
                        out.push(*tag);
                    }
                    break;
                }
                cur = self.ast.class_parents.get(&name).and_then(|p| p.clone());
                depth += 1;
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

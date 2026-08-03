//! `LowerCtx<'a>` struct definition extracted from `ssa_lower.rs`
//! chunk 370.
//!
//! The god-context struct threaded through every LowerCtx method (see
//! `impl<'a> LowerCtx<'a>` in `ssa_lower.rs`). Kept as a bare-bones move
//! — visibility, field layout, and doc comments are byte-for-byte
//! preserved from the source. Methods and construction sites stay
//! outside; siblings and ssa_lower.rs address it via
//! `crate::ssa_lower::LowerCtx` through the pub(crate) re-export.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId};
use crate::ssa::{self, BakedRegexEntry, BlockId, FuncId, Operand, Type, ValueId};
use crate::ssa_lower::{CallRetargets, Intrinsics, LocalInfo, PreReserveState};
use crate::ssa_lower_stmt_break_continue::LabelFrame;

pub(crate) struct LowerCtx<'a> {
    pub(crate) f: &'a mut ssa::Function,
    pub(crate) ast: &'a Ast,
    pub(crate) fn_table: &'a HashMap<String, FuncId>,
    /// FuncId → return type, populated in pass 1 of `lower`. Lets call-site
    /// lowering pick the right SSA result type even when the callee hasn't
    /// been body-lowered yet (forward refs, mutual recursion, bool returns).
    pub(crate) signatures: &'a HashMap<FuncId, Type>,
    /// FuncId → SigId for every user FnDecl, populated in pass 1. Used by
    /// `let f = global_fn` to allocate the right FnSig slot type and by
    /// `FnAddr(fid)` to type its result. M2 Phase B Stage 4.
    pub(crate) fn_sig_ids: &'a HashMap<FuncId, ssa::SigId>,
    /// Resolved FuncIds for the runtime intrinsics. Read at every site that
    /// emits a runtime call — string-literal lowering needs `str_alloc`,
    /// `console.log` needs `print_i64` / `str_print`, etc.
    pub(crate) intrinsics: Intrinsics,
    /// User-declared type aliases (`type Point = { ... }` → Type::Obj).
    /// Threaded through so `parse_type("Point", ...)` resolves at let-decl
    /// + function-signature sites.
    pub(crate) aliases: &'a HashMap<String, Type>,
    /// T-15.g.6.b (v0.5.0) — per-Expr check::Type map (from
    /// check::check_with_types). Lets the await Member-access
    /// dispatch recover Promise<T>'s inner T at the call site
    /// without PromiseId interning. Empty when constructed via
    /// the legacy `lower(...)` entry — those programs see the
    /// pre-T-15.g.6.b await-result-type-erased behavior (await on
    /// a heap-typed Promise yields i64 at SSA, breaking
    /// console.log direct-form dispatch).
    pub(crate) expr_types: &'a HashMap<ExprId, crate::check::Type>,
    /// T-28 — per-Call ExprId count of trailing args to pad with
    /// ANY_UNDEF Any-box at the call site (caller passed fewer args
    /// than callee's param count, trailing missing params all
    /// Type::Any). Empty when constructed via the legacy lower(...)
    /// entry — those programs keep strict arity.
    pub(crate) arity_pad_count: &'a HashMap<ExprId, usize>,
    /// Chunk 702 — array literals whose Array<Any> type came from a
    /// let-decl annotation (checker contextual-typing walker). The
    /// Expr::Array flavor gate keys off this set, NOT off expr_types:
    /// infer-widened Array<Any> shapes (`["a", undefined]`) share the
    /// type but belong to the typed lane (Str undefined sentinel).
    pub(crate) contextual_any: &'a std::collections::HashSet<ExprId>,
    /// W1 (ann-width RFC) — module-wide F64-poisoned number-slot set
    /// from `num_width::f64_slots`. Consulted at the let-decl site so
    /// a `: number` (or un-annotated) binding whose reaching values
    /// include a statically-possible f64 takes the F64 slot.
    pub(crate) num_f64_slots: &'a crate::num_width::WidthTable,
    /// ②.6b — synthesized promise-callback ABI thunks (bits-adapters
    /// the `.then` / `.catch` lowering wraps f64-faced handlers in).
    pub(crate) promise_thunks: &'a crate::ssa_lower_promise_thunk::PromiseThunks,
    /// C3a-2 — lifted-closure body fid → boxed dual-entry adapter
    /// (fid, sig) for the construction site's boxed_entry store.
    pub(crate) boxed_entries: &'a HashMap<FuncId, (FuncId, ssa::SigId)>,
    /// Mutable view of the lowering-phase Array element-type interner.
    /// Let-decl annotations encountered during body lowering may
    /// introduce new `T[]` instantiations; they intern lazily here.
    /// Written into `module.arr_layouts` at the end of `lower()`.
    pub(crate) arr_layouts: &'a mut Vec<Type>,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-6 — host-built baked
    /// DFA buffer. `try_bake_regex_dfa` pushes one entry per AOT-
    /// eligible literal regex; written into
    /// `module.baked_regex_entries` at the end of `lower()`, then
    /// `cmd_build` forwards them into `LinkConfig::baked_regex_entries`
    /// for the user_regex_baked_layout pipeline (C-5b/c) to lay out
    /// the `BakedDfaMeta` + `[DfaState; N]` payload in __DATA_CONST.
    /// Ineligible literals + `new RegExp(...)` keep routing through
    /// `__torajs_regex_compile`.
    pub(crate) baked_regex_buf: &'a mut Vec<BakedRegexEntry>,
    /// Mutable view of the lowering-phase fn-pointer signature interner.
    /// `__fn(P1|P2)->R` annotations intern lazily; written into
    /// `module.signatures` at the end of `lower()`. M2 Phase B Stage 2.
    pub(crate) fn_sigs: &'a mut Vec<(Vec<Type>, Type)>,
    /// Mutable view of the struct-layouts interner. M3.4 lets parse_type
    /// instantiate a generic-struct annotation (`Pair<number|string>`)
    /// during body lowering and intern the resulting concrete layout
    /// here on-demand. Pre-M3.4 this was an immutable snapshot, but
    /// generic instantiation needs to grow the table. Detached from
    /// `module.struct_layouts` at the top of `lower()` and written back
    /// at the end.
    pub(crate) struct_layouts: &'a mut Vec<Vec<(String, Type)>>,
    /// Generic-instantiation memo: full instantiation key
    /// (`Rec<number>`) → its reserved StructId. Reserve-first so a
    /// recursive alias closes its back-edge on the in-flight sid
    /// instead of recursing forever; persistent across the whole
    /// lower() so every mention of one key shares one sid.
    pub(crate) inst_memo: &'a mut HashMap<String, ssa::StructId>,
    /// M3.4 — generic struct decls indexed by name. Used by parse_type
    /// to instantiate `Foo<arg|...>` annotations in let-decl / fn-arg /
    /// closure-construction sites.
    pub(crate) generic_struct_decls: &'a HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    /// Phase H.1.b — `class name → runtime tag`. Keyed by name (not
    /// sid) because classes with structurally identical fields share
    /// a single sid; sid-keyed tags would alias them and silently
    /// mis-route `__dispatch_<M>`. Plain `type` aliases aren't keys
    /// here (they get a 0 tag at allocation time).
    pub(crate) class_name_to_tag: &'a HashMap<String, u32>,
    /// W-J Phase A1 — `anonymous-ObjectLit sid → runtime tag`. Snapshot
    /// built at Pass 1.5 boundary; sids interned later inside Pass 2
    /// (e.g. generic mono) miss this map and stamp tag 0 (MVP fallback).
    /// Tag space starts at `class_name_to_tag.len() + 1` so it doesn't
    /// collide with named class tags; cycle visitor / reflection helper
    /// indexes class_layouts via `class_tag - 1`, push order in the
    /// class_layouts emit loop mirrors this map's enumeration.
    pub(crate) anon_stamp_pool: &'a crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    /// M4 — innermost-active try block's catch-block target. Each
    /// `Stmt::Try` lowering pushes the catch BlockId before lowering its
    /// body and pops after; user-fn calls in scope insert a cond_br on
    /// `__torajs_throw_check()` that targets `*top` (or the fn's
    /// propagate-out path if empty).
    pub(crate) try_stack: Vec<BlockId>,
    /// M4.3.b — fn names that may throw (directly or transitively).
    /// `emit_throw_check` skips the check after a call to a callee
    /// whose name isn't in this set; intrinsics + verified-pure
    /// user fns are exempt. Recovers the per-call cost M4.1 paid.
    pub(crate) may_throw_fns: &'a std::collections::HashSet<String>,
    /// review #0001 fix — innermost-active finally block whose body
    /// should run before the enclosing fn's `return` actually fires.
    /// `Stmt::Return` inside a try-with-finally pushes its value into
    /// `pending_return_slot` (fn-wide), sets `pending_return_flag`, and
    /// branches to the top of this stack. The finally tail dispatches:
    /// pending_return AND we're outermost → `load + ret`; otherwise →
    /// `br` to the next outer finally to keep unwinding.
    pub(crate) try_finally_stack: Vec<BlockId>,
    /// Every for-of whose body is being lowered right now, outermost
    /// first. `break` reaches its loop's exit block and closes the
    /// iterator there; `return` leaves for the epilogue instead, so it
    /// emits the same §7.4.9 close + slot release for each frame here.
    pub(crate) for_of_teardown_stack: Vec<crate::ssa_lower_for_of_teardown::ForOfTeardown>,
    /// Lazily-allocated alloca slot for a pending return value across
    /// finally blocks. Type matches the enclosing fn's ret type. None
    /// until the first try-with-finally lowering observes a return
    /// would need to flow through it.
    pub(crate) pending_return_slot: Option<ValueId>,
    /// Companion bool flag for `pending_return_slot` — set by Return
    /// inside a try-with-finally, checked at finally tail to decide
    /// whether to ret vs continue normally.
    pub(crate) pending_return_flag: Option<ValueId>,
    /// name → (alloca-ptr value, contents type, moved flag). Every local —
    /// including the function's own parameters — sits behind an alloca.
    /// mem2reg lifts them to SSA values at -O1+.
    ///
    /// `moved` mirrors check.rs's affine pass: when a binding's value is
    /// consumed (let-rhs, assign-rhs, non-Copy call-arg, return), the
    /// flag flips to true and Drop emission at fn-end skips that local.
    /// NOTE: HashMap iteration order is random per process — any walk
    /// that feeds instruction emission must sort first (see
    /// `emit_drops_for_owned_locals` in `ssa_lower_drops.rs`), or the
    /// `tr build` output becomes non-reproducible.
    pub(crate) locals: HashMap<String, LocalInfo>,
    /// RFC 20260708-closure-argc-abi chunk 2 — params of THIS fn
    /// whose ann carried the `__clsargc(` prefix (mono-instantiated
    /// real-argc closure slot). The closure-local call arm prepends
    /// `ConstI64(user arg count)` for these names, same as the
    /// chunk-1 `ast.closure_argc_locals` binding set.
    pub(crate) argc_locals: std::collections::HashSet<String>,
    /// RFC 20260708-variadic — locals (params / let bindings) whose
    /// ann carried a `__rest(` segment (`(...args: E[]) => R`). The
    /// closure-local call arm routes these through the boxed dual
    /// entry (`closure_call_variadic`) instead of the static
    /// declared-pair ABI — one binding serves closures of any
    /// declared arity.
    pub(crate) variadic_locals: std::collections::HashSet<String>,
    /// RFC 20260719-ns-static-value-reify — bindings initialized
    /// from a namespace-static VALUE read, name → table id. The
    /// `.length` member fold reads the spec length off the shared
    /// table row instead of the checker-sig param count (which is a
    /// coincidence for Math and wrong for console's rest-shaped 0).
    pub(crate) ns_static_locals: std::collections::HashMap<String, i64>,
    /// RFC 20260725-str-method-value-reify — bindings initialized
    /// from a String-receiver builtin method VALUE read
    /// (`const m = s.slice`), name → method id. The `.length` /
    /// `.name` member folds read the spec row off the shared
    /// torajs-rc meta table instead of the checker-sig shape.
    pub(crate) builtin_mv_locals: std::collections::HashMap<String, (i64, i64)>,
    /// Stack of names declared in each enclosing lexical scope, with the
    /// fn-root scope as `scope_stack[0]`. M1.3 — at `}` close we pop the
    /// top frame and emit drops for owners declared at that depth, then
    /// remove them from `locals`. Cross-scope `let n = s` looks at this
    /// stack to detect that s lives in an outer scope (alias-only rule).
    pub(crate) scope_stack: Vec<Vec<String>>,
    /// Parallel to `scope_stack`. When a `let X` shadows an outer-scope
    /// `X`, the OLD `LocalInfo` for X is pushed here (in the current top
    /// frame) before the inner binding overwrites `locals[X]`. On scope
    /// close, after the inner frame's bindings are dropped + removed,
    /// each (name, prev_info) here is reinstated into `locals`. Without
    /// this, inner-block close `locals.remove(name)` would also evict the
    /// outer X (HashMap is keyed by name only) and any subsequent outer
    /// reference would crash with `unknown ident X`.
    pub(crate) shadow_stack: Vec<Vec<(String, LocalInfo)>>,
    /// Parallel to `try_finally_stack` — `loop_stack.len()` recorded at
    /// the time each finally was pushed. Used by `Stmt::Break` /
    /// `Stmt::Continue` to detect whether the topmost finally is
    /// "between" the current site and the innermost enclosing loop. If
    /// so, break/continue must route through finally first (set the
    /// pending flag, branch to finally; finally tail dispatches the
    /// pending flag back to the loop's break/continue target). Without
    /// this, `for { try { break } finally { … } }` would skip the
    /// finally body — spec violation.
    pub(crate) try_finally_loop_depth: Vec<usize>,
    /// Bool slot allocated lazily on first break-inside-finally; set by
    /// the break site, checked at finally tail. Same lifecycle as
    /// `pending_return_flag`.
    pub(crate) pending_break_flag: Option<ValueId>,
    /// Same shape for continue.
    pub(crate) pending_continue_flag: Option<ValueId>,
    /// Loop control-flow stack — innermost loop on top. M1.7. Each entry
    /// is `(continue_target, break_target)`: a `break` inside the loop
    /// body branches to break_target; a `continue` branches to
    /// continue_target. For while-loops, continue_target = loop header
    /// (re-evaluates cond). For for-loops, continue_target = step block
    /// (so the step still runs on continue, then back to header).
    pub(crate) loop_stack: Vec<(BlockId, BlockId)>,
    /// Active labeled statements — innermost on top (ES §13.13). A
    /// `break label` / `continue label` inside the labeled statement's
    /// body resolves its target through this stack. See
    /// [`crate::ssa_lower_ctx_struct::LabelTarget`] for the two kinds.
    pub(crate) label_stack: Vec<(String, LabelFrame)>,
    pub(crate) cur_block: BlockId,
    /// New string literals encountered during this lowering pass (currently
    /// only main collects them). Caller appends these to the module's
    /// strings table; StringId offsets are pre-assigned via string_id_base.
    pub(crate) new_strings: &'a mut Vec<ssa::StringLiteral>,
    pub(crate) string_id_base: usize,
    /// M2 — capture-types side channel shared across all fn lowerings.
    /// Construction site (`Expr::Closure`) populates the entry for the
    /// lifted FnDecl name; the lifted body's `lower_fn` reads it to emit
    /// env-load preambles. Outliving any individual lower_fn call.
    pub(crate) closure_captures: &'a mut HashMap<String, Vec<(String, Type, bool)>>,
    /// M3 — per-call-site `ExprId → mono_name` retarget map. The
    /// monomorphization pre-pass produced one specialized FnDecl per
    /// `(generic_name, type_args)`; at each generic call site, the
    /// `Expr::Call` arm rewrites the callee Ident to the mono name from
    /// this map before falling through to the regular call lowering.
    pub(crate) call_retargets: &'a CallRetargets,
    /// Names of `let` bindings in the current fn body that are
    /// captured by an escape closure (one whose env outlives the
    /// construction frame — detected via the enclosing fn's return
    /// type being a Closure type). These lets get heap-allocated
    /// slots at let-decl so the env can hold a stable pointer to
    /// them. The env-drop fn frees the slot (along with the env)
    /// when the closure value is dropped.
    /// Empty for non-escape-context fns; populated at fn-entry by
    /// scanning `body` for `Expr::Closure` captures.
    pub(crate) escape_captured_lets: std::collections::HashSet<String>,
    /// RFC 20260710 — names assigned anywhere in the program
    /// (Assign-Ident / PostIncr targets, lifted closure bodies
    /// included). A captured non-Copy binding that appears here is
    /// MUTATED and promotes to a capture box so every closure shares
    /// the live binding (ES §9.1) instead of an env-owns snapshot.
    pub(crate) mutated_captured_lets: std::collections::HashSet<String>,
    /// RFC 20260710 — non-Copy bindings in the current fn that were
    /// promoted to a capture box (`LocalInfo.slot` is the box value
    /// slot). Drives the byref capture write, the scope-close /
    /// fn-exit `capture_box_drop_heap` release, and nested-capture
    /// byref propagation. `LocalInfo.borrowed == false` marks the
    /// stake owner (the declaring frame); a closure-body preamble
    /// binding is borrowed (the env owns its stake).
    pub(crate) boxed_noncopy_lets: std::collections::HashSet<String>,
    /// Bindings whose capture box was opened AHEAD of their own
    /// declaration because a closure earlier in the same statement list
    /// captures them (mutually recursive arrows). Each name is removed
    /// by the declaration that fills it; see
    /// [`crate::ssa_lower_stmt_let_decl_recursive`].
    pub(crate) hoisted_closure_lets: std::collections::HashSet<String>,
    /// The same hoist for an ORDINARY binding, whose slot type is not
    /// knowable that early: the box goes up provisionally and each env
    /// that took it is noted as `(lifted closure fn, capture index)` for
    /// the declaration to correct. See
    /// [`crate::ssa_lower_stmt_let_decl_general::bind_let_slot`].
    pub(crate) forward_capture_boxes: std::collections::HashMap<String, Vec<(String, usize)>>,
    /// v0.6+1 perf checkpoint — push-loop pre-reserve fast-push state.
    ///
    /// When the for-loop lowerer detects a canonical fill loop
    /// (`for (let i = 0; i < N; i++) xs.push(_)`), it:
    ///   1. Emits `arr_reserve(xs, len + N)` once before the loop.
    ///   2. Hoists `head_x8 + 24` (the byte offset of slot[0] from
    ///      arr_ptr) into a loop-invariant register; allocas an i64
    ///      `len_slot` initialized to the array's len.
    ///   3. Inside the loop, arr.push lower emits inline IR:
    ///      `StoreDyn val at (arr_ptr + head_off + len*8)` plus
    ///      `len_slot++`. NO call to arr_push_unchecked, NO per-iter
    ///      head load — head_off is hoisted, len lives in the
    ///      mem2reg-promotable alloca.
    ///   4. After the loop, the final len is written back to the
    ///      array's len field at +8.
    ///
    /// Multi-array support deliberate: a body that pushes to two
    /// distinct arrays in lockstep still benefits — each gets its
    /// own state entry. Conservative: only fires when the for-loop's
    /// full body shape matches the detector.
    pub(crate) push_unchecked_for: std::collections::HashMap<String, PreReserveState>,
    /// V0.2 perf — fn-scope const RegExp LICM cache. Keyed by
    /// `(pattern, flags)` literal pair; populated lazily at the
    /// first `Expr::Regex { pattern, flags }` site within the fn.
    /// The first occurrence hoists `__torajs_regex_compile(pat,
    /// flags)` to the entry block (BlockId(0)); subsequent
    /// occurrences with identical key reuse the SSA `ValueId`.
    /// Mirrors V8/JSC hoist of regex literals out of hot loops —
    /// `str-replace-100k` bench: -25.6% wall (hoisting alone
    /// saves ~400 ns/iter of regex parse+compile+heap alloc).
    /// Spec edge: ES §22.2.4.1 says `/x/g` evaluates fresh per
    /// occurrence (lastIndex state), but `String.prototype.{replace,
    /// match}` reset lastIndex internally — fn-scope sharing is
    /// unobservable on the common surface. test262 regressions
    /// are caught by the conformance gate.
    pub(crate) regex_lit_cache: std::collections::HashMap<(String, String), ssa::ValueId>,
    /// P1.5/P1.8 — per-binop scratch flags carrying which side (if any)
    /// is a frontend Type::Undefined source. Set by lower_binop_with_ids
    /// before dispatching to the inner impl, restored after. The Eq/Neq
    /// Any-side packing reads these to pick ANY_UNDEF=5 vs ANY_NULL=0.
    pub(crate) binop_left_undef_id: Option<ExprId>,
    /// RFC 20260708-typed-arr-oob-read chunk 2 — the left/right
    /// binop operand is an F64 that may hold the undefined-NaN
    /// sentinel (number[] index read / alias); `=== undefined`
    /// compares the bits instead of the cross-type false fold.
    pub(crate) binop_left_f64_undefable: bool,
    pub(crate) binop_right_f64_undefable: bool,
    /// Guard-dominated bounds-check elision — `(i, xs)` pairs
    /// proven in-bounds by an enclosing `i < xs.length` loop guard
    /// (see [`crate::ssa_lower_bounds_proven`]). Loop lowerers push
    /// / pop; the Block arm evicts tainted pairs per statement.
    pub(crate) bounds_proven: Vec<(String, String)>,
    pub(crate) binop_right_undef_id: Option<ExprId>,
    /// Chunk 612 companion — which side (if any) is a frontend
    /// Type::Null source (the `null` literal or a Null-typed
    /// binding). The Eq/Neq nullish folds combine these with the
    /// undef flags to answer undefined-vs-null statically for
    /// bindings, not just literals (a Null/Undefined-typed Load is
    /// not ConstPtrNull, so operand-shape checks alone miss it).
    pub(crate) binop_left_null_id: Option<ExprId>,
    pub(crate) binop_right_null_id: Option<ExprId>,
    /// S9 square carve — set by lower_binop_with_ids when both Mul
    /// operands are the same identifier (`x * x`): a value times
    /// itself can never be negative×zero, so -0 is unmintable and
    /// the int path keeps (mirrors the width_of square carve).
    pub(crate) binop_mul_square: bool,
    /// P7.4-a-b — set by `lower_binop_inner` when a bigint
    /// Div/Mod/Pow/Shl/Shr is dispatched (those runtime helpers can
    /// call `__torajs_throw_range_error`). The enclosing `Expr::BinOp`
    /// arm `std::mem::take`s it and emits the throw-check AFTER the
    /// refcounted operands are dropped — emitting it inside
    /// `lower_binop_inner` would split the block before those drops and
    /// strand them across the split. #13's binding-slot entry-hoist
    /// keeps the `let c = …` slot in the entry block so the post-split
    /// scope-end drop's load still resolves.
    pub(crate) bigint_op_may_throw: bool,
    /// Phase K.3 — module-level data globals (top-level `let X: T = init`
    /// where T is a primitive Copy type). Read by the ident-read fallback
    /// to emit `GlobalRef + Load` for cross-fn reads, and by the LetDecl
    /// arm in `main` to emit `GlobalRef + Store` for the init expression.
    /// Refcount-typed globals (string / array / object / class instance)
    /// are NOT in this map yet — they fall through to the existing
    /// implicit-main-local path; lifting them requires a destructor at
    /// program exit and is deferred to a later phase.
    pub(crate) globals: &'a HashMap<String, Type>,
    /// Phase K.3 — true while lowering the synthesized `main` fn. The
    /// LetDecl arm uses this to decide whether a top-level let in
    /// `globals` should write to the global slot (in main) or skip
    /// declaration entirely (in named fns — they only ever read/write
    /// the slot via the ident-read / Assign-Ident fallbacks).
    pub(crate) is_main_fn: bool,
    /// V3-05 — sids currently being inlined by `emit_drop_value`.
    /// Self-referential class layouts (`class Node { next: Node | null }`)
    /// would otherwise inline-recurse forever at codegen. When the
    /// drop-emitter sees a sid already on this stack, it routes the
    /// child drop through `__torajs_value_drop_heap` (runtime tag
    /// dispatch) instead of inlining another copy of the field walk.
    /// Note: today value_drop_heap's default branch leaks Obj inner
    /// refs — proper class-layout-driven child drop lands in V3-09.
    pub(crate) drop_inline_stack: std::collections::HashSet<u32>,
    /// 11-A1 — fn-local deque-unsafe Array binding names. Populated
    /// by `ssa_lower_deque_escape::collect_deque_arr_names_in_stmt`
    /// at fn entry; queried by `arr_expr_is_non_deque` at every
    /// Index emit site to pick the fast-path offset.
    pub(crate) deque_arrs: std::collections::HashSet<String>,
    /// 11-A2-a — fn-local escape-bound Obj-typed binding names.
    /// Populated by `ssa_lower_obj_escape::collect_escape_obj_let_
    /// names_in_stmt` at fn entry. Queried at the typed-Obj
    /// `LetDecl` alloc emit site: a binding whose init is
    /// `ObjectLit { ... }` and whose name `∉ escape_obj_lets` swaps
    /// `Call(__torajs_obj_alloc)` for `AllocaBytes(size)` and is
    /// recorded in `stack_alloced_locals` so end-of-scope drop
    /// emission skips its rc-dec branch + drop_sized call.
    pub(crate) escape_obj_lets: std::collections::HashSet<String>,
    /// RC-4 F1c — init ExprIds of defineProperty-receiver `let`
    /// declarations (`crate::dynobj_degrade` scope-correct walk).
    /// An unannotated ObjectLit `LetDecl` in the set lowers through
    /// the P3.2 dynobj-init lane (binding type `any`), mirroring the
    /// checker's `dynobj_degraded` so the two sides can't drift.
    pub(crate) dynobj_degraded: std::collections::HashSet<crate::ast::ExprId>,
    /// RFC 20260804-mutable-let-widen — mirror of the checker's set.
    pub(crate) cross_type_widened: std::collections::HashSet<crate::ast::ExprId>,
    /// RC-4 F1a — binding names whose let-init is an exec/match
    /// method call (Nullable<Array<Str>> per the checker). Filled
    /// in declaration order by the LetDecl arm; `.length` / index
    /// loads on these receivers emit the `arr_null_check` guard so
    /// a miss (null) is a catchable TypeError, not a SIGSEGV.
    pub(crate) nullable_arr_lets: std::collections::HashSet<String>,
    /// RFC 20260707-undefined-sentinel-repr chunk 1 — binding names
    /// whose let-init is an element load off a nullable-arr source
    /// (`const c = m[1]` with m an exec/match binding) or an alias
    /// of such a binding. The slot may hold NULL (missed capture);
    /// `.length` loads emit the `str_null_check` guard and the
    /// inline str-eq-with-literal fast path declines to the
    /// null-guarded `str_eq` runtime call.
    pub(crate) nullable_str_lets: std::collections::HashSet<String>,
    /// RFC 20260708-typed-arr-oob-read chunk 2 — bindings whose
    /// init was a `number[]` index read (may hold the undefined-NaN
    /// sentinel); typeof / strict-eq / nullish / print / box
    /// consumers gate on membership.
    pub(crate) undefable_f64_lets: std::collections::HashSet<String>,
    /// RFC 20260707 residual chunk — binding names whose let-init
    /// is a string INDEX read (`const c = s[i]`) or an alias of
    /// such a binding. The Substr slot may hold the Substr-shaped
    /// undefined sentinel (OOB read); the inline
    /// str-eq-with-literal fast path declines to the
    /// identity-aware `substr_eq_str` runtime call for these.
    pub(crate) undefable_substr_lets: std::collections::HashSet<String>,
    /// RFC 20260722-find-miss chunk B — binding names whose let-init
    /// is a heap-elem `find`/`findLast` call (miss answers the
    /// generic undefined cell) or an alias of a Nullable heap
    /// source. Member reads / typeof / strict-eq / box consumers on
    /// these bindings take the undef-cell-aware lanes.
    pub(crate) undefable_heap_lets: std::collections::HashSet<String>,
    /// 11-A2-a — set of binding names whose backing storage was
    /// allocated on the stack (`AllocaBytes`) instead of the heap.
    /// `emit_drops_for_owned_locals` and sibling drop emitters skip
    /// any local whose name is in this set: no rc-dec branch, no
    /// `__torajs_obj_drop_sized` call. Stack reclaim is automatic
    /// at fn return.
    pub(crate) stack_alloced_locals: std::collections::HashSet<String>,
    /// 11-A2-a — short-lived hint set by the `LetDecl` arm before
    /// lowering an `ObjectLit` init, when (a) the binding is in
    /// the safe set (`name ∉ escape_obj_lets`) and (b) the init
    /// is syntactically a direct `ObjectLit`. Read once by the
    /// `ObjectLit` arm at its alloc site via `.take()`. If consumed
    /// and the runtime layout contains no refcounted field, the
    /// alloc swaps `Call(obj_alloc)` for `AllocaBytes(size)` and
    /// the name is inserted into `stack_alloced_locals`.
    pub(crate) let_stack_alloc_hint: Option<String>,
    /// Chunk 780 — short-lived declared-layout hint set by the
    /// `LetDecl` arm before lowering an `ObjectLit` init whose
    /// annotation parsed to `Type::Obj(sid)`. Read once by
    /// `resolve_objlit_layout` via `.take()`: two same-shaped
    /// literals under DIFFERENT declared struct types (`Box<number>`
    /// vs `Box<string>` — both lower `{v: undefined-ptr, label:
    /// str}`) otherwise first-match into whichever compatible
    /// layout registered first, so the writer stores the wrong
    /// slot repr for the reader's declared layout (probe gD:
    /// Any-boxed undefined written, `load str` + STR sentinel
    /// compared → eq answers false).
    pub(crate) let_declared_obj_layout: Option<crate::ssa::StructId>,
    /// RFC 20260705 chunk 555 — take-once redispatch hint: a
    /// try_lower dispatcher that already lowered a receiver and then
    /// declined (`ssa_lower_str::try_lower_method_call`'s `None`
    /// fall-through) parks the lowered operand here instead of
    /// abandoning it. `lower_expr` consumes it when the next cascade
    /// arm re-lowers the same `ExprId`, so a side-effecting receiver
    /// (`getNum().toString()`) evaluates exactly once. The eid guard
    /// keeps unrelated lowerings untouched; unconsumed entries are
    /// dead values, never re-emitted.
    pub(crate) redispatch_lowered: Option<(ExprId, Operand)>,
    /// RFC 20260712 chunk B — fresh-owned operands parked in a str
    /// method's argv (ToString-coerced searchValue/replaceValue,
    /// fresh temp args) that must drop AFTER the runtime helper
    /// call consumes them. `populate_argv` pushes; `dispatch_
    /// intrinsic` drains right after the emit. Pre-B these leaked
    /// (300k `replace(n.slice(0,1), ..)` churned 16MB).
    pub(crate) argv_owned_temps: Vec<(Operand, crate::ssa::Type)>,
    /// Chunk 637 — Member-read ExprIds whose lowering answered an
    /// OWNED value: the receiver was itself an owned temp (Call /
    /// New / As-of-Call shape), so `ssa_lower_member::lower` inc'd
    /// the non-Copy field result to detach it from the receiver and
    /// released the receiver before returning. Consumers consult
    /// this via `expr_owned_shape` / `expr_is_fresh_owned` so they
    /// take the ref over instead of adding their own (double inc)
    /// or ignoring it (leak). ExprIds are arena-unique, so entries
    /// never need eviction. Chunk 722 — Ternary / Nullish joins
    /// whose branches were unified to owned ride the same track
    /// (the set carries every "this eid's result is owned" verdict
    /// decided at lowering time, not just member reads).
    pub(crate) owned_member_reads: std::collections::HashSet<ExprId>,
}

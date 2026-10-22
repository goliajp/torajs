#![allow(dead_code)] // step 1: types only; lowerer (step 2) + backend (step 3) consume the rest

// SSA IR: what the frontend (lex/parse/check) lowers into and the
// native backend consumes.
//
// - Operands carry constants inline (Operand::ConstI64 etc.) rather
//   than through their own SSA value.
// - Newtype IDs for ValueId/BlockId/FuncId.
// - The per-function value table holds each ValueId's type and an
//   optional debug name the pretty printer prefers over the number.

mod arr_layouts;
pub mod block_freq;
pub mod branch_prob;
pub mod dominator;
mod function_methods;
pub mod id_hash;
pub mod liveness_blocks;
pub mod loop_analysis;
mod module_class_layouts;
mod module_extras;
mod module_methods;
mod op_impls;
pub mod suspend;
mod type_def;
pub mod vec;
mod visit;

/// Mach-O names of the two runtime statics user code reads INLINE
/// (`GlobalRef` + `Load`) instead of calling an accessor — rotation
/// 470, each such call was a measurable slice of a method-call loop:
/// the in-flight-throw flag `torajs_throw::__torajs_throw_active`
/// (read after every call that may raise; the e-graph's
/// self-tail-call matcher anchors on it) and the live-WeakRef-
/// observer count `torajs_rc::__torajs_weakref_active` (read before
/// a class instance dies).
pub const THROW_ACTIVE_SYM: &str = "___torajs_throw_active";
pub const WEAKREF_ACTIVE_SYM: &str = "___torajs_weakref_active";
/// The any-slot tag of a heap cell as `__torajs_anyv_box_from_pair`
/// reads it (torajs-rc `AnySlotTag::Heap`); every other tag boxes an
/// immediate, which no rc kernel touches.
pub const ANY_SLOT_TAG_HEAP: i64 = torajs_rc::AnySlotTag::Heap as i64;
/// Heap-header flag bits (the u16 at +6) that change without a call
/// naming the object: the cycle collector's buffered and color bits.
/// Every other flag moves only inside a runtime call handed the object.
pub const HEADER_FLAGS_COLLECTOR: u16 = torajs_rc::FLAG_BUFFERED | torajs_rc::COLOR_MASK;
/// The header word of a dense `any[]`: `FLAG_ARR_ANY` in the flag
/// half, as the inline element read tests it (`and word, mask` then
/// `icmp eq` against this).
pub const HEADER_WORD_ARR_ANY: i64 = (torajs_rc::FLAG_ARR_ANY as i64) << 48;
/// The NaN-box number encoding the typed number nodes (`AnyIsNumber`
/// and the five beside it) expand with: the int32 tag in the top 16
/// bits, and the offset a double's bits are shifted by.
pub use torajs_anyvalue::nanbox::{DOUBLE_ENCODE_OFFSET, TAG_TYPE_NUMBER};

pub use arr_layouts::ArrLayouts;
pub use module_class_layouts::{ClassLayoutMeta, FieldMetaSpec, MethodMetaSpec, field_type_tag_of};
pub use module_extras::{LOCAL_LABEL_PREFIX, demo_fib40, is_local_label};
pub use module_methods::{
    BakedDfa, BakedRegexEntry, DataGlobal, FnNameEntry, Module, PinFact, PinWord, StringLiteral,
    VtableGlobal,
};
pub use op_impls::{BinOp, FPred, IPred, Operand};
pub use suspend::{SuspendFn, SuspendKind, SuspendLane, SuspendRuntime};
pub use type_def::Type;
pub use vec::{VecInst, VecOp, VecShape};
pub use visit::{lowers_to_call, rewrite_value_operands, visit_value_operands};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StringId(pub u32);

/// Index into `Module.struct_layouts`. Two `StructId`s compare equal iff
/// they refer to the same interned layout (i.e. structurally equal types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructId(pub u32);

/// Index into `Module.arr_layouts`. Each entry holds one `Array<T>`
/// instantiation's element type. Two `ArrId`s compare equal iff they
/// refer to the same element type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArrId(pub u32);

/// Index into `Module.signatures`. Each entry holds one fn-pointer
/// signature `(Vec<param_types>, ret_type)`. Two `SigId`s compare equal
/// iff their signatures are identical. M2 Phase B Stage 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SigId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InstKind {
    BinOp(BinOp, Operand, Operand),
    ICmp(IPred, Operand, Operand),
    FCmp(FPred, Operand, Operand),
    Call(FuncId, Vec<Operand>),
    /// `%p = alloca <ty>` — stack-allocate a slot of `ty`. Result type is Ptr.
    /// Used for mutable locals; mem2reg lifts these to SSA values at -O1+.
    Alloca(Type),
    /// `%p = alloca_bytes <n>` — stack-allocate `n` raw bytes (8-byte
    /// aligned). Result type is Ptr. Used for ABI-shaped buffers like
    /// the 48-byte SplitIter struct or the 32-byte Substr borrow slot
    /// where the SSA Type system can't express the precise byte size.
    AllocaBytes(u64),
    /// `%v = load <ty>, <ptr>+<offset>` — load a value of `ty` from
    /// pointer + byte_offset. Offset is 0 for plain alloca-slot loads;
    /// non-zero for object field reads (offset = field_index * 8 in the
    /// MVP layout).
    Load(Type, Operand, u64),
    /// `store <value>, <ptr>+<offset>` — void result; value's type
    /// determines the store width. Same offset convention as Load.
    Store(Operand, Operand, u64),
    /// `%v = load_dyn <ty>, <ptr>+<dyn_byte_offset>` — like Load but the
    /// byte offset is an SSA value instead of a constant. Used for
    /// dynamic array indexing `xs[i]` where `i` isn't statically known.
    /// Backends compute `addr = base + offset` then load.
    LoadDyn(Type, Operand, Operand),
    /// `store_dyn <value>, <ptr>+<dyn_byte_offset>` — symmetric for the
    /// load. Used for `xs[i] = v`.
    StoreDyn(Operand, Operand, Operand),
    /// `%v = load_dyn_s8 <ty>, <base> + <idx>*8` — scaled register-
    /// offset load. The e-graph folds `LoadDyn(ty, base, Shl(idx, 3))`
    /// into this when the shift has no other use: aarch64's
    /// `LDR Xd, [Xn, Xm, LSL #3]` does the scale inside the AGU, so
    /// the standalone shift on the address dependency chain disappears
    /// (S7 knife c2). Only the 8-byte stride exists — the scaled
    /// register form requires shift == log2(access width).
    LoadDynScaled8(Type, Operand, Operand),
    /// `store_dyn_s8 <value>, <base> + <idx>*8` — symmetric scaled
    /// store.
    StoreDynScaled8(Operand, Operand, Operand),
    /// `%v = load_u8_dyn <base> + <idx>` — load ONE byte at
    /// `base + idx` and zero-extend to i64. aarch64 has this as a
    /// single instruction (`LDRB Wd, [Xn, Xm]` — writing Wd zeroes
    /// the upper 32 bits, and LDRB itself zeroes bits 8..32), so a
    /// byte scan costs exactly one load per probe. Introduced for
    /// the S7-r2 SplitIter fast path: the per-token
    /// `__torajs_split_iter_next` cross-archive call is replaced by
    /// an inline Latin-1 byte scan, which needs byte-granularity
    /// reads the 8/4-byte `LoadDyn` family can't express. Result
    /// type is always I64.
    LoadU8Dyn(Operand, Operand),
    /// `%v = load_u16_dyn <base> + <idx>*2` — load one UTF-16 code
    /// unit at `base + idx*2` and zero-extend to i64 (`LDRH Wd, [Xn,
    /// Xm, LSL #1]`, the scale inside the AGU). The wide-string arm
    /// of the inline `charCodeAt` read. Result type is always I64.
    LoadU16Dyn(Operand, Operand),
    /// `%v = sitofp <i64-operand>` — signed integer to f64 cast. Used to
    /// promote i64 operands when mixed with f64 in arithmetic / comparisons.
    SiToFp(Operand),
    /// `%v = fptosi <f64-operand>` — float to signed i64 cast (truncates).
    /// Mirrors JS's ToInt32 / ToUint32 prefix behaviour on the truncation
    /// step. Used at call sites whose runtime intrinsic expects an i64
    /// integer parameter (Math.imul, Math.clz32, anywhere accepting a
    /// "numeric integer index" the user might have written as 0.5).
    FpToSi(Operand),
    /// `%v = fptoint32 <f64-operand>` — ECMA-262 §7.1.6 ToInt32 as a
    /// sign-extended i64: truncate toward zero, modulo 2^32, low 32
    /// bits read signed; NaN / ±Inf → 0. Not `FpToSi`: that saturates
    /// at |x| ≥ 2^63 and on ±Inf, so its low 32 bits are wrong there
    /// (`1e20 | 0` is 1661992960, `Infinity | 0` is 0). The bitwise
    /// operators' operand step and the `Math.imul` / `Math.clz32`
    /// int32 sink. One FJCVTZS Wd, Dn plus a SXTW on aarch64.
    FpToInt32(Operand),
    /// `%v = zext <bool-operand>` — zero-extend an i1 / Bool value to i64.
    /// Needed when storing booleans into uniform 8-byte slots (`Array<bool>`,
    /// `Object` fields with bool type, etc.) and when passing them to
    /// runtime intrinsics whose signature is i64-shaped.
    ZExtBoolToI64(Operand),
    /// `%v = zext <i32-operand>` — zero-extend an i32 to i64. Introduced
    /// in P11.1-S1 alongside the Str layout flip: `length` moves from
    /// `u64 @8` to `u32 @8 + reserved u32 @12`, so the SSA `.length`
    /// property-access arm now reads u32 and widens to the i64-shaped
    /// `Type::Number` value that flows downstream. Distinct from
    /// `ZExtBoolToI64` because the LLVM source type differs (i32 vs
    /// i1) — emitter dispatch picks the right `build_int_z_extend`
    /// source width based on the variant.
    ZExtI32ToI64(Operand),
    /// `%v = bitcast <f64-operand>` — pun an f64's IEEE 754 bit pattern
    /// into an i64 without value conversion. Used by T-10.d's tagged-slot
    /// Array<Any>: ANY_F64 slots stash the f64 bits in their value field
    /// and decode back via the symmetric `BitCastI64ToF64` at read time.
    /// LLVM lowers to `bitcast double %x to i64`.
    BitCastF64ToI64(Operand),
    /// `%v = bitcast <i64-operand>` — symmetric reverse: read an Any
    /// slot's value field as an f64 bit pattern. LLVM lowers to
    /// `bitcast i64 %x to double`.
    BitCastI64ToF64(Operand),
    /// T-15.g.6.c (v0.5.0) — `%v = inttoptr <i64-operand>` — cast
    /// an i64 to ptr-shape (opaque pointer at LLVM 22). Used by
    /// the await Member-access dispatch when Promise<T>'s inner T
    /// is heap-typed: the runtime helper returns `int64_t` per its
    /// C ABI, but the SSA value-table needs the result typed as
    /// the actual ptr-shape (Type::Str / Type::Arr / etc.) so
    /// downstream Member-access / Index instructions dispatch
    /// correctly. LLVM lowers to `inttoptr i64 %x to ptr`.
    IntToPtr(Operand),
    /// Step 7d (v0.7 Phase 3 NaN-box) — `%v = ptr → i64`. Inverse of
    /// `IntToPtr`. Used by the NaN-box AnyValue bridge: ssa_lower's
    /// Type::Any operand is ptr-shaped at the LLVM level, but its
    /// bit pattern is an immediate AnyValue (u64). To pass it to
    /// a `__torajs_anyv_*` shim that takes `i64`, ssa_lower emits
    /// `PtrToInt(any)` to expose the immediate bits as i64.
    /// LLVM lowers to `ptrtoint ptr %x to i64`.
    PtrToInt(Operand),
    /// T-15.g.6.c (v0.5.0) — `%v = trunc <i64-operand> to i1` —
    /// narrow an i64 (typically a Promise-packed Bool: 0 or 1)
    /// back to i1. Used by the await Member-access dispatch when
    /// Promise<boolean> is awaited; the helper returns int64_t per
    /// its C ABI, but `print_bool` expects i1 / Bool ssa-type.
    /// Symmetric reverse of `ZExtBoolToI64`.
    TruncI64ToBool(Operand),
    /// `%v = string_ref <id>` — yields a (ptr, len) pair to a global string
    /// constant. Result type is Ptr; the length lives in the module's
    /// `strings` table alongside the bytes.
    StringRef(StringId),
    /// `%v = static_str_ref <id>` — yields a Type::Str ptr to a static
    /// Str-shaped global (`[hdr:8 STATIC flag set][len:8][bytes:N]`),
    /// drop-in compatible with a heap-alloc'd Str. rc_inc / rc_dec /
    /// str_free / arr_free no-op via the STATIC flag, so the same global
    /// can serve every callsite of a literal in a hot loop without per-
    /// iter alloc + memcpy + drop. Used by `intern_string_literal` to
    /// short-circuit the `StringRef + str_alloc` pair.
    StaticStrRef(StringId),
    /// Phase K.3 — `%v = global_ref <name>` — pointer to a module-level
    /// data global slot (top-level `let X: T = init`). Result type is
    /// always Ptr; the slot's value type is stored in `Module::data_globals`
    /// so the backend can pick the right load/store width. Pair with
    /// `Load(ty, ptr, 0)` / `Store(value, ptr, 0)` for read / write.
    GlobalRef(String),
    /// `%v = fn_addr <fid>` — take the address of a known function.
    /// Result type is `Type::FnSig(sig_id)` matching the function's
    /// signature. M2 Phase B Stage 3.
    FnAddr(FuncId),
    /// `%v = boxed_entry_addr <fid>` — the address of a closure's
    /// synthesized any-ABI adapter (`__boxed_<name>`), taken only to
    /// be stored into the cell's `boxed_entry` slot. Same result type
    /// and codegen as `FnAddr`, but under its own symbol alias
    /// (`__torajs_boxed_<n>`) so the link can tell a mint apart from
    /// the other address-takers of the same adapter and judge it
    /// (RFC 20260824-s2-5 刀 4 A1; `torajs_rc::closure_entry`).
    BoxedEntryAddr(FuncId),
    /// `%v = call_indirect <sig_id>, <ptr>, <args>` — call through a
    /// function pointer. The signature is looked up via `module.signature(sig_id)`
    /// at codegen so the backend can build the right calling convention.
    /// M2 Phase B Stage 3.
    CallIndirect(SigId, Operand, Vec<Operand>),
    /// P-OPT Phase 1 — `id <op>`: egraph rewrite placeholder; elaborate aliases via `set_opt_value`.
    Identity(Operand),
    /// P-OPT Phase 2 chunk 11b — `%v = neg <op>` two's-complement
    /// negate. Emitted by the egraph `SubNegate` rule rewriting
    /// `sub 0 x → Neg(x)` (the asymmetric LHS-zero case left by
    /// `SubZero`, which only handles RHS-zero). Codegen: aarch64
    /// `neg Xd, Xn` is the `sub Xd, XZR, Xn` alias (ARM ARM C7.2.273);
    /// LLVM: `sub i64 0, %x`. i64 only — FP Neg deferred.
    Neg(Operand),
    /// `%v = ctpop <op>` — population count of an i64 (number of set
    /// bits, 0..=64). Produced only by the egraph `ctpop_idiom` pass
    /// (LLVM LoopIdiomRecognize analogue) replacing the Kernighan
    /// `while (n) { n &= n - 1; c++ }` loop; never lowered directly
    /// from source. Codegen: aarch64 has no scalar popcount at the
    /// base ISA — the canonical sequence is `fmov d,x; cnt v.8b;
    /// addv b,v.8b; fmov x,d` (what LLVM emits for `llvm.ctpop.i64`).
    Ctpop(Operand),
    /// `%v = cttz <op>` — count of trailing zero bits of an i64 (64 for
    /// zero), the bit index of the lowest set bit. Codegen: aarch64
    /// `rbit Xd, Xn; clz Xd, Xd` (what LLVM emits for `llvm.cttz.i64`).
    Cttz(Operand),
    /// `%v = copy <ty> <op>` — register-level move, the destruction
    /// product of mem2reg's φ placement (LLVM PHIElimination's mov
    /// shape). Emitted at predecessor-block ends so a φ-merged value
    /// has one home; the SAME result ValueId may be defined by several
    /// `Copy`s on different predecessors — the one deliberate non-SSA
    /// shape in the IR, introduced only after the egraph pass (GVN /
    /// elaborate never see it) and consumed by codegen, whose liveness
    /// merges multi-def intervals (`entry().or_insert` + CFG fix-point)
    /// exactly like a classic virtual register. `ty` picks the GPR vs
    /// FPR move at emit.
    Copy(Type, Operand),
    /// `%v = select <ty> <cond>, <then>, <else>` — branchless conditional
    /// move: yields `then` when `cond` (Bool/i1) is true, `else`
    /// otherwise. Produced only by the egraph select-formation pass
    /// (if-conversion of CondBr diamonds whose arms are pure; LLVM
    /// SimplifyCFG's FoldTwoEntryPHINode analogue) — never lowered
    /// directly from source, and introduced after the egraph pass so
    /// GVN / elaborate never see it. Both value arms are ALWAYS
    /// evaluated (speculation) — formation must only hoist trap-free,
    /// side-effect-free defs. `ty` is the result type and picks the
    /// csel (GPR) vs fcsel (FPR) form at emit; formation currently
    /// gates to non-F64 so only the GPR form is implemented.
    /// Codegen: aarch64 `cmp cond, #0; csel Xd, Xn, Xm, NE`
    /// (ARM ARM C6.2.53).
    Select(Type, Operand, Operand, Operand),
    /// `%sum = ctpop.range.sum <start>, <bound>, <acc_init>` — canned
    /// popcount-reduction super-instruction:
    /// `acc_init + Σ ctpop(i) for i in [start, bound)` over i64 bit
    /// patterns (empty sum when start ≥ bound). Produced only by the
    /// egraph ctpop-range-sum formation pass (the counted
    /// `total += ctpop(i)` loop collapses to this one inst — LLVM
    /// LoopIdiomRecognize's canned-replacement approach applied to a
    /// reduction; RFC 20260719-ctpop-range-sum) — never lowered
    /// directly from source, and introduced after the egraph pass so
    /// GVN / elaborate never see it. Codegen emits a self-contained
    /// 8-wide SIMD reduction loop (cnt.16b + udot.4s + uadalp.2d,
    /// 4 accumulators) plus a scalar tail — see
    /// compile/ctpop_range_sum.rs. Register contract: the emitter
    /// clobbers V0-V7 + V16-V18 and X9-X12, so the inst is an
    /// `inst_emits_bl` clobber site — the allocator relocates
    /// live-across values to callee-saved homes exactly as for a
    /// Call (the clobber set is a subset of a call's).
    CtpopRangeSum(Operand, Operand, Operand),
    /// `rc_inc <ptr>` — void. Retain a heap cell whose operand is a
    /// real pointer or null (never a NaN-box immediate: `Any` values
    /// keep the `__torajs_rc_inc` call, which tells cells from
    /// immediates). Same effect as `HeapHeader::inc_ref`: null and
    /// FLAG_STATIC_LITERAL cells are left alone, otherwise the u32
    /// refcount at +0 goes up by one, wrapping like the runtime's.
    /// Codegen expands it in place with no call, so values live
    /// across it keep caller-saved homes.
    RcInc(Operand),
    /// `rc_inc_any <v>` — void. `RcInc` for a value that may be a
    /// NaN-box immediate (`Any`, untyped pointers): only a cell (non-
    /// zero, top 16 bits clear, TAG_BIT_TYPE_OTHER clear — ShortStr and
    /// every number / sentinel encoding fail it) is retained. Same
    /// effect as `__torajs_rc_inc` / `__torajs_anyv_rc_inc`.
    RcIncAny(Operand),
    /// `rc_dec_any <v>` — void. Release one reference held by a value
    /// that may be a NaN-box immediate: `__torajs_value_drop_heap` on a
    /// cell, nothing otherwise. Codegen decrements in place only where
    /// the runtime would do nothing else (a count above one on a Str
    /// cell, or on a cell already in the cycle collector's root buffer)
    /// and calls the runtime for the rest, so it is a call site for
    /// register allocation, weighted as a slow path.
    RcDecAny(Operand),
    /// `box_any <tag> <v>` — the NaN-box encoding of `v` under an
    /// `AnySlotTag` known at compile time (0 null · 1 bool · 2 int ·
    /// 3 double bits · 4 heap pointer · 5 undefined), the answer of
    /// `__torajs_anyv_box_from_pair(tag, v)`. A pure encode: no
    /// refcount traffic, no call — codegen expands it in place.
    BoxAny(i64, Operand),
    /// `any_is_number <v>` — Bool: the `any` encodes a number (an
    /// int32 or a double). Cells, ShortStr and the sentinels answer
    /// false.
    AnyIsNumber(Operand),
    /// `any_is_int32 <v>` — Bool: the `any` is the int32 encoding.
    AnyIsInt32(Operand),
    /// `any_to_int32 <v>` — I64: the int32 an `any` encodes, sign-
    /// extended. Defined only where `any_is_int32` held.
    AnyToInt32(Operand),
    /// `any_to_f64 <v>, <is_int>` — F64: the number an `any` encodes,
    /// `is_int` being `any_is_int32` of it. Defined only where
    /// `any_is_number` held.
    AnyToF64(Operand, Operand),
    /// `box_int32 <i>` — Any: the int32 encoding of an I64 that fits
    /// an int32 (the caller's proof; nothing is checked).
    BoxInt32(Operand),
    /// `box_f64 <f>` — Any: the double encoding of an F64, every NaN
    /// stored as the canonical one (§6.1.6.1 makes them
    /// indistinguishable; only a NaN payload can wrap into another
    /// tag's range). The same answer as `box_any 3` of its bits.
    ///
    /// These six are the typed faces of the NaN-box number encoding:
    /// the lowering emits them, the egraph reads the number flow
    /// through them (a cell every def boxes and every use unboxes can
    /// live unboxed), and `anynum::expand` lowers what remains to the
    /// bit operations after φ promotion.
    BoxF64(Operand),
    /// SIMD lane arithmetic over `Type::Vec` values (see `vec`).
    Vec(VecInst),
    /// `throw_signal` — void, no operands: the `Ret` ending this block
    /// returns because an exception is propagating out of the function,
    /// and its value is the throw sentinel, not a result. Lowering puts
    /// it last in the propagate block. It reads and writes nothing and
    /// emits no machine code; passes must neither drop it nor count it
    /// as work. The inliner discards it with the `Ret` it marks.
    ThrowSignal,
    /// `%v = pinned <word>` — a header word of a pinned array field,
    /// which a cluster member finds in the register its entry loaded
    /// it into (`Module::pins`). No operands, no machine code; codegen
    /// homes the value in that register.
    Pinned(PinWord),
    /// `%v = suspend <kind> <lane>, %src [held…]` — the function parks
    /// here and returns to its caller; it is re-entered here, and `%v`
    /// is the value the suspension produced (see `suspend`). `held`
    /// are the values the body owns at this point, which the frame's
    /// trace reports while it waits. The split pass removes every one
    /// before codegen.
    Suspend(SuspendKind, SuspendLane, Operand, Vec<Operand>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Inst {
    pub result: Option<ValueId>, // None for void calls
    pub kind: InstKind,
    /// v0.3 #4 D-3 — AST ExprId this instruction was lowered from
    /// (or None for synthetic insts emitted between lower_expr
    /// calls). Debug-info emission derives the `.ts:line:col` from
    /// `ast.expr_spans[origin]` through this so DWARF backtraces
    /// resolve to the right source position.
    pub origin: Option<crate::ast::ExprId>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    Br(BlockId),
    CondBr {
        cond: Operand,
        then_blk: BlockId,
        else_blk: BlockId,
    },
    Ret(Option<Operand>),
    Unreachable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub insts: Vec<Inst>,
    pub term: Terminator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValueInfo {
    pub ty: Type,
    /// Debug-only display name. Pretty printer prefers this over the numeric
    /// id; codegen ignores it.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<ValueId>,
    pub ret: Type,
    pub blocks: Vec<Block>,
    pub values: Vec<ValueInfo>, // index = ValueId.0
    /// v0.3 #4 D-3 — current AST ExprId being lowered. ssa_lower's
    /// `lower_expr(eid)` sets/restores this; `append_inst` /
    /// `append_void` stamp it as the new Inst's `origin`.
    /// `#[serde(skip)]`-equivalent: not part of any persistent SSA
    /// dump, just a transient build-time slot.
    pub current_origin: Option<crate::ast::ExprId>,
}

/// Hand-built fib(n: i64) -> i64 module — the same shape the retired
/// LLVM-gate spike (labs/0002, removed with the inkwell backend)
/// emitted as LLVM IR. Used by `tr ssa-demo` to validate the IR types
/// + pretty printer before the lowerer (step 2) existed.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_set_type_wiring() {
        // P6.1 substrate sanity — Type::Map / Type::Set are first-class
        // SSA types: refcounted heap pointers with their own as_str
        // names. Affine (non-Copy) like every other heap-owned type.
        assert_eq!(Type::Map.as_str(), "map");
        assert_eq!(Type::Set.as_str(), "set");
        assert!(Type::Map.is_refcounted());
        assert!(Type::Set.is_refcounted());
        assert!(!Type::Map.is_copy());
        assert!(!Type::Set.is_copy());
        assert!(Type::Map.is_pointer_shaped());
        assert!(Type::Set.is_pointer_shaped());
    }

    #[test]
    fn map_iter_type_wiring() {
        // P6.4b substrate sanity — Type::MapIter is a refcounted
        // heap pointer (holds strong ref to the source Map), affine,
        // distinct as_str so type-erased call sites can detect it.
        assert_eq!(Type::MapIter.as_str(), "mapiter");
        assert!(Type::MapIter.is_refcounted());
        assert!(!Type::MapIter.is_copy());
        assert!(Type::MapIter.is_pointer_shaped());
    }

    #[test]
    fn arr_iter_type_wiring() {
        // P6.4c-C3 — Type::ArrIter parallel to MapIter (Array<Any>
        // source side, same iteration substrate shape).
        assert_eq!(Type::ArrIter.as_str(), "arriter");
        assert!(Type::ArrIter.is_refcounted());
        assert!(!Type::ArrIter.is_copy());
        assert!(Type::ArrIter.is_pointer_shaped());
    }

    #[test]
    fn fib40_pretty_prints() {
        let m = demo_fib40();
        let mut s = String::new();
        m.write_to(&mut s).unwrap();
        // sanity: covers all the structural pieces the printer emits, not a
        // golden match — format is allowed to drift if the test still passes.
        assert!(s.contains("fn fib(%n: i64) -> i64"));
        assert!(s.contains("%t = icmp slt %n, 2"));
        assert!(s.contains("cond_br %t, bb1, bb2"));
        assert!(s.contains("ret %n"));
        assert!(s.contains("%a = sub %n, 1"));
        assert!(s.contains("%r1 = call fib(%a)"));
        assert!(s.contains("%s = add %r1, %r2"));
        assert!(s.contains("ret %s"));
    }
}

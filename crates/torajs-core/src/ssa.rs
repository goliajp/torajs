#![allow(dead_code)] // step 1: types only; lowerer (step 2) + backend (step 3) consume the rest

// SSA IR for the new codegen path (P3.5).
//
// This is the IR that frontend (lex/parse/check) lowers into, and that the
// LLVM backend (P3.5+) and Cranelift backend (P3.6) both consume. It exists
// alongside the stack-machine `ir.rs` (which feeds the tree-walk interpreter
// and is on the retirement list with the wasm-via-C path).
//
// Step 1 of P3.5: define the types + pretty printer + a hand-built fib40
// demo that round-trips through `tr ssa-demo`. The lowerer (AST → SSA) is
// step 2; the LLVM backend (SSA → Inkwell) is step 3.
//
// Design notes:
// - **Operands carry constants inline** (Operand::ConstI64 etc.) rather than
//   going through their own SSA value. Matches LLVM IR's actual textual
//   shape and keeps the pretty-printed output readable.
// - **Newtype IDs** for ValueId/BlockId/FuncId — cheap type safety, harder
//   to confuse a value index with a block index.
// - **Per-function value table** holds the type and optional debug name of
//   each ValueId. Optional name is what makes `%n` / `%t` / `%r1` show up
//   in the pretty output instead of `%0` / `%4` / `%7`.
// - **No phi nodes yet** — fib40 only needs branching, not loop carry. Phis
//   will land in step 2 when we lower `while`.

mod function_methods;
mod module_class_layouts;
mod module_extras;
mod module_methods;
mod op_impls;
mod type_def;
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

pub use module_class_layouts::{ClassLayoutMeta, FieldMetaSpec, MethodMetaSpec, field_type_tag_of};
pub use module_extras::demo_fib40;
pub use module_methods::{
    BakedRegexEntry, DataGlobal, FnNameEntry, Module, StringLiteral, VtableGlobal,
};
pub use type_def::Type;
pub use visit::visit_value_operands;

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

// PartialEq + Eq + Hash are implemented manually below so `ConstF64`
// can compare and hash by IEEE 754 bit pattern (NaN-stable; required
// by the egraph GVN map where two textually identical f64 constants
// must collapse even if one of them is NaN, matching Cranelift's
// `gvn_map.rs` treatment of the same edge case).
#[derive(Debug, Clone, Copy)]
pub enum Operand {
    Value(ValueId),
    ConstI64(i64),
    /// i32 constants only ever come up as `main`'s `ret 0` for now.
    ConstI32(i32),
    ConstF64(f64),
    ConstBool(bool),
    /// `null` literal value for a pointer-shaped slot (Str / Obj / Arr /
    /// Closure / FnSig). At codegen we emit `ptr_t.const_null()` —
    /// exactly the in-band 0 sentinel JS treats as nullish. Cheaper
    /// than ConstI64(0) since no inttoptr is needed.
    ConstPtrNull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    // Integer
    Add,
    Sub,
    Mul,
    SDiv,
    SRem,
    And,
    Or,
    Xor,
    Shl,
    AShr, // arithmetic (signed) shift right
    LShr, // logical shift right
    // Floating point
    FAdd,
    FSub,
    FMul,
    FDiv,
    /// Floating-point remainder — IEEE 754 fmod-shaped, used for JS
    /// Number `%` when either operand is f64 (V3-18 m1.h.41).
    FRem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IPred {
    Eq,
    Ne,
    Slt,
    Sgt,
    Sle,
    Sge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FPred {
    Oeq,
    One,
    Olt,
    Ogt,
    Ole,
    Oge,
    /// Unordered-or-not-equal — true if either operand is NaN OR
    /// the values differ. Required for JS `!==` / `!=` on f64
    /// (NaN !== NaN must be true per spec §7.2.16).
    Une,
}

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
    /// `%v = sitofp <i64-operand>` — signed integer to f64 cast. Used to
    /// promote i64 operands when mixed with f64 in arithmetic / comparisons.
    SiToFp(Operand),
    /// `%v = fptosi <f64-operand>` — float to signed i64 cast (truncates).
    /// Mirrors JS's ToInt32 / ToUint32 prefix behaviour on the truncation
    /// step. Used at call sites whose runtime intrinsic expects an i64
    /// integer parameter (Math.imul, Math.clz32, anywhere accepting a
    /// "numeric integer index" the user might have written as 0.5).
    FpToSi(Operand),
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
    /// Step 7d (v0.7-Phase3-nanbox) — `%v = ptr → i64`. Inverse of
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
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    pub insts: Vec<Inst>,
    pub term: Terminator,
}

#[derive(Debug, Clone)]
pub struct ValueInfo {
    pub ty: Type,
    /// Debug-only display name. Pretty printer prefers this over the numeric
    /// id; codegen ignores it.
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
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

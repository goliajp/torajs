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
mod module_methods;
mod op_impls;

pub use module_methods::{ClassLayoutMeta, DataGlobal, Module, VtableGlobal, demo_fib40};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    I64,
    F64,
    I32,
    Bool,
    Void,
    /// LLVM 22 uses opaque pointers — no need to track what's pointed at.
    /// The Load instruction carries the loaded type explicitly; Store
    /// derives it from the value operand's type.
    Ptr,
    /// Owned heap-string handle. At codegen this lowers to the same
    /// machine type as Ptr (a single pointer), but at the SSA layer it
    /// stays distinct from a generic alloca pointer so that:
    ///   - `console.log(s)` can dispatch to print_str vs print_i64 by
    ///     reading the operand's SSA type
    ///   - drop emission (P2.2.b) knows which slots need free()
    ///   - future inline-small-string layout can change the codegen
    ///     without touching the SSA shape
    /// Step 2.2.a: only static-pointer (literal) backed strings — the
    /// pointer is a `[N x i8]` global; drop is a no-op. Concat + true
    /// heap allocation lands in 2.2.b/c.
    Str,
    /// Substring view — non-owning slice of an owned `Str`. Layout:
    /// `[header:8][len:8][parent_ptr:8][offset:8]` (32 bytes). The
    /// view holds a refcount on its parent Str so the source bytes
    /// stay alive; view's drop dec's parent's refcount before free.
    ///
    /// Created by `s.split(sep)`, `s.slice(start, end)` etc. when the
    /// result is a borrow into the source's bytes (zero `memcpy`,
    /// zero per-substring byte alloc). Mirrors Swift's `Substring` /
    /// Rust's `&str` slice — separate type from `Str` so the OWNED
    /// hot-path doesn't pay any indirection cost. At codegen this
    /// also lowers to a single pointer (same as `Str`), but the SSA
    /// distinction routes `.charCodeAt` / `=== "literal"` / etc. to
    /// view-aware variants that load bytes via `parent_data + offset`
    /// instead of `self + 16`.
    ///
    /// Type system: TS source has no separate syntax for substring
    /// (only `string`); the compiler infers `Substr` for split / slice
    /// outputs and propagates it through let-binds + for-of. At fn-
    /// call boundaries that expect `Str`, the call site auto-coerces
    /// (Phase Substr.B materializes; Phase Substr.C will mono-
    /// morphize the callee for both Str and Substr arg types to keep
    /// view performance across boundaries).
    Substr,
    /// Owned heap object handle pointing at a struct with the layout
    /// stored at `module.struct_layouts[id]`. Like Str, lowers to a
    /// single pointer at codegen — the SSA-level distinction lets
    /// drop emission look up which fields are non-Copy and (in P2.4.d)
    /// recursively drop them before freeing the outer struct.
    /// P2.4.c MVP: layout is N×8-byte slots in field declaration order;
    /// only Copy fields supported (recursive drop comes in P2.4.d).
    Obj(StructId),
    /// Owned heap array of `T`. Layout: `{u64 len, u64 cap, T data[cap]}`
    /// with uniform 8-byte slots regardless of element type — primitives
    /// store directly, heap-typed elements (Str / Obj / nested Arr)
    /// store a pointer. M1.2 MVP. The element type interns into
    /// `module.arr_layouts[id]`.
    Arr(ArrId),
    /// Function-pointer value, typed by interned signature. Lowers to
    /// pointer-width at codegen; the signature info routes indirect
    /// calls (`InstKind::CallIndirect`) so backends can build the
    /// right calling convention. M2 Phase B Stage 2.
    FnSig(SigId),
    /// Closure value — a heap pointer to an env block whose layout is
    /// `[i64 fn_ptr, capture_0, capture_1, ...]`. SigId is the
    /// **user-visible** signature (without the env first param).
    /// Codegen lowers to a single pointer; calling a closure loads the
    /// fn pointer from env+0 and indirect-calls with env as the first
    /// argument. Heap-owned, non-Copy (the env block is freed when the
    /// last owner of the closure binding goes out of scope).
    Closure(SigId),
    /// Compiled regex instance — a heap pointer to a struct whose
    /// layout is `{ universal_heap_header; nfa_state_count; nfa_states;
    /// num_groups; flags; source_str_ptr }`. Built by a `/pat/flags`
    /// literal lowering through `__torajs_regex_compile`. Member calls
    /// (`.test`, `.exec`, ...) lower to the matching `__torajs_regex_*`
    /// runtime helpers. ARC-owned (universal heap header); drop routes
    /// through `__torajs_rc_dec` like every other heap object. Lowers
    /// to a single pointer at codegen.
    RegExp,
    /// Date instance — heap pointer to `{ universal_heap_header; i64
    /// ms_since_epoch }` (16 bytes). Built by `new Date(...)` lowering
    /// through `__torajs_date_now` / `__torajs_date_from_ms`. Member
    /// calls (`.getTime`, `.toISOString`, ...) lower to
    /// `__torajs_date_*` runtime helpers. ARC-owned via universal
    /// heap header.
    Date,
    /// T-13.a (v0.4.0) — `Type::Symbol` value. Heap-allocated 16-byte
    /// block: universal heap header + owned description Str ptr (NULL
    /// when no description supplied). Identity is pointer identity —
    /// each `Symbol(desc)` call allocates fresh, so equality is the
    /// natural ICmp Eq on Ptr operands. console.log dispatches to
    /// `__torajs_symbol_print` which formats `Symbol(<desc>)`.
    /// Lowers to a single pointer at codegen.
    Symbol,
    /// T-15 (v0.5.0) — `Type::Promise` value. Heap-allocated 32-byte
    /// block managed by `runtime_promise.c`: universal heap header +
    /// state byte + i64 value slot + callbacks linked-list head.
    /// Lowers to a single pointer at codegen. T-15.f.2 ships only
    /// the type variant; T-15.g wires Promise.resolve / .then /
    /// await dispatch through ssa_lower. The element type from
    /// check.rs's Type::Promise(Box<Type>) is type-erased here at
    /// the SSA layer — the runtime always sees an i64-shaped value
    /// slot regardless of T.
    Promise,
    /// T-25 (v0.7) — `Type::BigInt`. Sign-magnitude heap struct
    /// `runtime_bigint.c`: universal heap header + sign u32 + len u32
    /// + words u64[len]. Lowers to a single pointer at the SSA layer.
    /// Drop routes through `__torajs_value_drop_heap`'s TAG_BIGINT
    /// case (rc-aware free).
    BigInt,
    /// T-26 (v0.7) — `Type::WeakRef`. 16-byte heap struct
    /// `runtime_weakref.c`: universal heap header + target ptr.
    /// Target observation is via the global hash registry; no
    /// strong rc held on the target. `wr.deref()` returns the
    /// target rc-bumped (caller takes ownership) or null when the
    /// target has been reclaimed. Lowers to a single pointer.
    WeakRef,
    /// T-26.B (v0.7) — `Type::WeakMap`. Heap struct holding an
    /// internal bucket table keyed by pointer identity; entries
    /// auto-evict when their key dies via the shared weakref
    /// registry. Lowers to a single pointer.
    WeakMap,
    /// T-26.B (v0.7) — `Type::WeakSet`. Same shape as WeakMap
    /// minus the value side.
    WeakSet,
    /// P6.1 — `Type::Map`. Strong-ref `Map<K,V>` heap struct
    /// (`runtime_map.c`): universal heap header + open-addressing
    /// robin-hood hash table; entries are tagged-Any key + tagged-Any
    /// value. Key equality follows SameValueZero (string byte-equal,
    /// number IEEE-754 with NaN == NaN, pointer identity for objects /
    /// arrays / functions / etc). Lowers to a single pointer at the
    /// SSA layer; drop routes through `__torajs_value_drop_heap`'s
    /// TAG_MAP case (walks live entries, drops both key + value rc's,
    /// frees the bucket array).
    Map,
    /// P6.1 — `Type::Set`. Strong-ref `Set<T>` wrapped over a
    /// `Map<T, undefined>` storage; same SameValueZero key equality.
    /// Lowers to a single pointer.
    Set,
    /// P6.4b — `Type::MapIter`. Stateful iterator returned by
    /// `m.keys() / .values() / .entries()`. Holds a strong ref to
    /// the source `Map` (so the entries[] array stays live during
    /// iteration) + a cursor + kind tag. The user surface is
    /// `iter.next()` returning an `IteratorResult<T>` struct; the
    /// runtime helper just produces the `(tag, payload)` pair and
    /// the SSA side wraps it into the spec-shaped struct. Lowers
    /// to a single pointer.
    MapIter,
    /// P6.4c-C3 — `Type::ArrIter`. Same shape as `MapIter` but
    /// scanning an `Array<Any>` source. Returned by
    /// `arr.keys() / .values() / .entries()`. Restricted to
    /// `Array<Any>` for now — typed-T arrays have an 8B-per-slot
    /// layout that the runtime helper can't walk without an
    /// elem-tag parameter (P5.4 follow-up).
    ArrIter,
    /// T-10 (v0.4.0) — `Type::Any` carries a tagged value at runtime:
    /// either a primitive (i64 / f64 / bool / null) or a heap pointer
    /// (Str / Obj / Arr / Closure / RegExp / Date). At the SSA layer
    /// it lowers to a single 64-bit pointer so existing slot / param /
    /// return paths work unchanged; the type tag lives in the runtime
    /// representation (heap-allocated Any-box for primitives;
    /// pointer-only for already-heap values, with the type discoverable
    /// via the universal heap header's `type_tag` field). T-10.a only
    /// wires the type-system plumbing — `let xs: any[] = []` accepted
    /// + length() works. T-10.b lands the tagged-slot Array<Any>
    /// runtime; T-10.c the codegen for heterogeneous Array literals.
    Any,
}

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

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone, Copy)]
pub enum IPred {
    Eq,
    Ne,
    Slt,
    Sgt,
    Sle,
    Sge,
}

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone)]
pub struct Inst {
    pub result: Option<ValueId>, // None for void calls
    pub kind: InstKind,
    /// v0.3 #4 D-3 — AST ExprId this instruction was lowered from
    /// (or None for synthetic insts emitted between lower_expr
    /// calls). ssa_inkwell looks this up to attach a DILocation
    /// derived from `ast.expr_spans[origin]` so DWARF backtraces
    /// resolve to the right `.ts:line:col`.
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

/// Hand-built fib(n: i64) -> i64 module — the same shape that
/// labs/0002-inkwell-spike emits as LLVM IR. Used by `tr ssa-demo` to
/// validate the IR types + pretty printer before the lowerer (step 2)
/// exists.

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

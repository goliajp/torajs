//! `ssa::Type` — the SSA-level type lattice (chunk 485; split out of
//! `ssa.rs` when the ctpop-idiom pass pushed it past the 500-line
//! HARD limit). Re-exported at `crate::ssa::Type` so every existing
//! caller path is unchanged.

use super::{ArrId, SigId, StructId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

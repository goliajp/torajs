//! `check::Type` enum + `impl Type` — file-size decomp of `check.rs`
//! (rotation 119 chunk 8). Zero coupling to Checker state; enum
//! variant docs + is_copy body verbatim from pre-chunk-8 parent.
//!
//! parent `check.rs` re-exports `Type` so the `crate::check::Type`
//! import path all callers depend on stays canonical (chunk 485
//! ssa.rs → ssa/type_def.rs handoff手法).

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Number,
    String,
    Boolean,
    Void,
    /// T-25 — arbitrary-precision integer (`123n` literal). Only
    /// equal to other BigInts. Cross-type ops with Number are a
    /// TypeError per spec; same-type ops produce BigInt.
    BigInt,
    /// T-26 — observed reference. `new WeakRef(target)` returns
    /// this; `wr.deref()` returns `Nullable<Any>` (type narrowing
    /// over generic T deferred — at the SSA layer the target slot
    /// is ptr-shaped regardless of the original T, and conformance
    /// cases work fine with `as` casts on the deref result).
    WeakRef,
    /// T-26.B — `WeakMap`. Pointer-identity-keyed map with auto-
    /// eviction on key death. `m.get(k)` returns `Nullable<Any>`,
    /// `m.has/delete` return Boolean, `m.set` returns the map (we
    /// stick with Void for the SSA result since chaining isn't
    /// observed in conformance fixtures yet — adjust if test262
    /// surfaces it).
    WeakMap,
    /// T-26.B — `WeakSet`. Set of pointer-identity-keyed entries
    /// with auto-eviction.
    WeakSet,
    /// P6.1 — `Map<K,V>`. Strong-ref hash map with SameValueZero
    /// key equality (string byte-equal / IEEE-754 number with NaN ==
    /// NaN / pointer identity for objects). `m.get(k)` returns
    /// `Nullable<V>`; `m.set/has/delete/clear` return Boolean / map /
    /// Number per spec.
    Map,
    /// P6.1 — `Set<T>`. Strong-ref hash set backed by `Map<T, undef>`.
    /// `s.add(v)` / `s.has(v)` / `s.delete(v)` / `s.size` per spec.
    Set,
    /// P6.4b — `MapIter`. Stateful iterator returned by
    /// `m.keys() / .values() / .entries()`. `iter.next()` returns
    /// `IteratorResult<any>` = `{ value: any, done: boolean }`.
    /// Holds a strong ref to the source Map.
    MapIter,
    /// P6.4c-C3 — `ArrIter`. Parallel to MapIter but scanning
    /// `Array<Any>` source. Returned by `arr.keys() / .values() /
    /// .entries()`. `iter.next()` returns the same
    /// `IteratorResult<any>` shape.
    ArrIter,
    /// v0 hack — `console.log`'s parameter accepts any printable type.
    /// Replace with a sum/union type later.
    Any,
    Function(Vec<Type>, Box<Type>),
    /// RFC 20260708-variadic — rest-param sentinel, legal ONLY as the
    /// trailing element of a `Function` param vec (from
    /// `(...args: E[]) => R` annotations, ann marker `__rest(E[])`).
    /// Holds the ELEMENT type E. Never a value type: call admit
    /// accepts any arity at/past the fixed prefix and unifies each
    /// rest-position argument against E.
    Rest(Box<Type>),
    /// Hardcoded global stand-ins (currently only `console`). Real
    /// user-defined object types use `Type::Struct`.
    Object(&'static str),
    /// Homogeneous array. Owned by `Rc<Vec<Value>>` at runtime in v0.
    Array(Box<Type>),
    /// Structural object type — fields in declaration order. Two
    /// `Type::Struct` are equal iff they share field names + types in
    /// matching order. (TS-style structural compatibility, not nominal.)
    /// P2.4 introduced this; backed by heap allocation in P2.4.c.
    Struct(Vec<(String, Type)>),
    /// V3-05 — nominal class reference by name. Returned by
    /// `resolve_type_ann_full` when the type-ann names a declared
    /// class that's still in its pre-register placeholder phase
    /// (i.e. self / forward / mutual references during Pass 0). Use
    /// `resolve_class_ref(t, aliases)` at consumer sites to dereference
    /// to the current `aliases[name]` (which after Pass 0 is the
    /// real `Type::Struct(real_fields)`).
    ///
    /// Without ClassRef, `class Node { next: Node | null }` would
    /// embed a `Type::Struct(empty)` placeholder by-value into Node's
    /// own field — leading to unify mismatches when assigning a
    /// real Node into the field. ClassRef + lazy resolution avoids
    /// the by-value capture pitfall.
    ClassRef(String),
    /// M3 — type-parameter placeholder. Only legal inside the body of a
    /// generic FnDecl; at call sites the typechecker infers a concrete
    /// substitution and the `ssa_lower` monomorphization pass produces
    /// one specialized fn per `(name, type_args)` tuple. Two `TypeVar`s
    /// compare equal iff their names match, so distinct generic fns
    /// must use distinct param names (the per-fn alias scope makes
    /// this naturally true).
    TypeVar(String),
    /// `null` literal value. Only useful in unification with
    /// `Type::Nullable(T)` — a bare `let x: null = null` is legal but
    /// not very useful.
    Null,
    /// P1.1 — `undefined` literal value. Distinct from `Null` per
    /// ES spec §6.1.1 / §6.1.2. Currently behaves identically to
    /// `Null` everywhere except (eventually) typeof: `typeof undefined`
    /// must return `"undefined"` while `typeof null` returns `"object"`.
    /// Pre-P1.1 tora aliased `undefined` to `Null` end-to-end, which
    /// silently wrong-ed the typeof distinction; this variant is the
    /// substrate for fixing it incrementally without touching the
    /// 250+ Type::Null match arms in one big diff. The follow-up
    /// P1 sub-items (P1.5 typeof, P1.8 strict-eq, P1.3 default param,
    /// P1.4 OOB read) flip the per-arm behavior site-by-site.
    Undefined,
    /// `T | null` — pointer-shaped T may carry the in-band 0 sentinel
    /// at runtime. Restricted to T ∈ {String, Array<_>, Struct, …};
    /// number/boolean nullables would need a tag bit and aren't in v0.
    Nullable(Box<Type>),
    /// Compiled regex instance produced by a `/.../flags` literal or
    /// (eventually) `new RegExp(...)`. Heap-owned, non-Copy, ARC under
    /// the universal heap header. Distinct from `Type::Object("RegExp")`
    /// (the global constructor) — `RegExp` is the *value* type of `re`
    /// in `let re = /foo/i;`. Method dispatch (`.test`, `.exec`, ...)
    /// resolves through the Member arm against this variant.
    RegExp,
    /// Date instance produced by `new Date(...)` or
    /// `Date.parse(...)` (the latter returns Number, not Date).
    /// Heap-owned, non-Copy, ARC under the universal heap header.
    /// Underlying storage is `int64_t ms_since_epoch`. Method
    /// dispatch (`.getTime`, `.toISOString`, `.getFullYear`, ...)
    /// resolves through the Member arm against this variant.
    /// Distinct from `Type::Object("Date")` (the global constructor +
    /// static methods like Date.now()).
    Date,
    /// T-13.a (v0.4.0) — Symbol value. Each `Symbol(desc?)` call
    /// allocates a fresh heap block; identity is pointer identity.
    /// Heap-owned, non-Copy, ARC. Distinct from `Type::Object("Symbol")`
    /// (the constructor + future Symbol.for / well-known statics).
    /// `=== / !==` on Symbol operands compares ptr; console.log
    /// formats `Symbol(<desc>)`.
    Symbol,
    /// T-15 (v0.5.0) — built-in `Promise<T>` value. Heap-allocated
    /// 32-byte block managed by `runtime_promise.c`; carries state
    /// (PENDING / FULFILLED / REJECTED) + value + callback list.
    /// Heap-owned, non-Copy, ARC under the universal heap header
    /// (type_tag=8). Distinct from `Type::Object("Promise")` (the
    /// constructor + static methods like Promise.resolve).
    ///
    /// Type-system support shipped in T-15.f; runtime wiring follows
    /// in T-15.g (Promise.resolve / .then dispatch). Async function
    /// desugar (existing in ast.rs) will be migrated to use the
    /// built-in in T-15.h, replacing the user-class MVP that was
    /// the only async/await pattern through v0.4.0.
    Promise(Box<Type>),
}

impl Type {
    /// Cheap-to-duplicate types live entirely in registers / stack — using
    /// the binding twice just produces two independent copies with no
    /// runtime cost. Affine types own heap storage and follow Rust-shaped
    /// move semantics: each binding is the unique owner; consuming the
    /// binding (let-rhs / assign-rhs / call-arg / return) transfers
    /// ownership and the source name is marked moved.
    pub fn is_copy(&self) -> bool {
        matches!(self, Type::Number | Type::Boolean | Type::Void | Type::Any)
        // Struct, String, Function, Array — all heap-owned, all affine.
        // TypeVar — conservatively NOT Copy (instantiation may produce a
        // heap-owned type); irrelevant in practice since generic bodies
        // never witness their own end-of-fn drop walk (monomorphization
        // produces concrete-type bodies before the SSA layer runs).
    }

    /// §13.3.6.2 step 6 — value types statically KNOWN to have no
    /// [[Call]]: invoking one is a runtime TypeError under every
    /// goal, not a compile stop (arguments still evaluate, step 4).
    /// The checker types such a call `Any` (its value is
    /// unreachable) and the lowering wedge
    /// (`ssa_lower_call_uncallable`) routes it through the runtime
    /// any-call dispatcher, whose non-closure arm raises the
    /// catchable TypeError. Both sides call THIS predicate — single
    /// source. Conservative measured set; types not listed keep the
    /// loud compile reject.
    pub(crate) fn is_uncallable_value(&self) -> bool {
        matches!(
            self,
            Type::Number
                | Type::String
                | Type::Boolean
                | Type::Void
                | Type::Null
                | Type::Undefined
                | Type::RegExp
                | Type::BigInt
        )
    }
}

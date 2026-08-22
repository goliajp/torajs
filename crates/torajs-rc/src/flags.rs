//! [`crate::HeapHeader`] `flags` field — bit-position constants.
//!
//! Split out of `lib.rs` (file-size hard limit; RFC
//! 20260713-defprop-residual-cluster chunk A pushed the root file
//! over 500). Re-exported at crate root so downstream crates keep
//! writing `torajs_rc::FLAG_FROZEN` etc.
//!
//! ## Occupancy map (u16)
//!
//! | bits  | user | scope |
//! |-------|------|-------|
//! | 0     | [`FLAG_SUBCLASSED`] | universal |
//! | 1     | [`FLAG_SPLIT_BLOCK`] | Str |
//! | 2     | [`FLAG_STATIC_LITERAL`] | universal |
//! | 3     | [`FLAG_ARR_ANY`] (Arr) / [`FLAG_FN_GENERATOR`] (Closure) | disjoint-by-tag |
//! | 4     | [`FLAG_FROZEN`] | universal |
//! | 5     | [`FLAG_BUFFERED`] | universal (cycle collector) |
//! | 6     | NULL_PROTO (torajs-dynobj private, DynObj) / [`FLAG_CLASS_METHOD_THIS_FREE`] (Closure) / [`FLAG_ARR_SPARSE_TAIL`] (Arr) | disjoint-by-tag |
//! | 7     | [`FLAG_ERROR`] (Obj) / [`FLAG_ARR_LENGTH_RO`] (Arr) / [`FLAG_FN_ASYNC`] (Closure) | disjoint-by-tag |
//! | 8     | [`FLAG_NON_EXTENSIBLE`] | universal |
//! | 9     | [`FLAG_SEALED`] | universal |
//! | 10-12 | element-kind field (`arr_kind.rs`) | Arr |
//! | 12    | [`FLAG_CLOSURE_RECV_FIRST`] (Closure) / [`FLAG_OBJ_EXPANDO`] (Obj) | disjoint-by-tag with Arr kind |
//! | 10-11 | [`FLAG_FN_NAME_DELETED`] / [`FLAG_FN_LENGTH_DELETED`] | Closure (disjoint-by-tag with Arr kind) |
//! | 10    | [`FLAG_DYNOBJ_CLASS_CTOR`] | DynObj (disjoint-by-tag with Closure / Arr) |
//! | 11    | [`FLAG_DYNOBJ_RAW_JSON`] (DynObj) / `STR_FLAG_HAS_CAPACITY` (torajs-str private, Str) | disjoint-by-tag with Closure / Arr |
//! | 13-14 | cycle-collector color field (`color.rs`) | **universal — never place a flag here** |
//! | 15    | [`FLAG_ARR_EXOTIC_INDEX`] (Arr) / [`FLAG_FN_PROTO`] (Closure) / [`FLAG_OBJ_EXOTIC_FIELD`] (Obj) | disjoint-by-tag |
//!
//! Bits 13-14 look free in a flag-constants-only read but are painted
//! by the cycle collector on EVERY tag: buffering a candidate sets
//! Purple = bit 14 (RFC 20260713-defprop-residual-cluster chunk A —
//! `defineProperties(arr, {})`'s receiver dec buffered the array
//! Purple, which read back as a locked length).

/// Exotic builtin cell minted as a user-class instance (RFC
/// 20260730-exotic-backed-class-instance blade 0) — `class C extends
/// Array` mints a real `Tag::Arr` cell and marks it here; the class
/// identity (class_tag + prototype cell) lives in torajs-meta's
/// subclass instance side table keyed on the cell pointer. Readers
/// (instanceof / getPrototypeOf / method dispatch / drop) gate on the
/// bit before touching the table, so plain builtin instances pay
/// nothing on paths that already loaded the header. Bit 0 is free on
/// every tag — universal.
pub const FLAG_SUBCLASSED: u16 = 1 << 0;
/// `str_split` single-malloc block carrying N inline substrs.
pub const FLAG_SPLIT_BLOCK: u16 = 1 << 1;
/// `Tag::Arr` cell is a materialized `arguments` object (the
/// `__torajs_arguments` local both desugar lanes mint) — its
/// `"length"` face carries §10.4.4.6 arguments-exotic attributes
/// ({writable: true, enumerable: false, **configurable: true**})
/// instead of §10.4.2's non-configurable array length. Readers that
/// answer the length descriptor / delete / hasOwnProperty gate on
/// this bit; a delete leaves a hole shadow entry under the `"length"`
/// key in the expando props dynobj (the element-domain tombstone
/// mechanism, RFC 20260712 chunk C — every enumerator already skips
/// holes). Bit 1 is Tag::Arr-private (disjoint-by-tag reuse of the
/// Str-only [`FLAG_SPLIT_BLOCK`]).
pub const FLAG_ARR_ARGUMENTS: u16 = 1 << 1;
/// rc_inc / rc_dec / str_free no-op when set (immortal literal).
pub const FLAG_STATIC_LITERAL: u16 = 1 << 2;
/// Array<Any>: 16-byte slots instead of 8.
pub const FLAG_ARR_ANY: u16 = 1 << 3;
/// `Object.freeze(obj)` set — field stores become silent no-ops.
pub const FLAG_FROZEN: u16 = 1 << 4;
/// "this object is in the cycle-collector buffer right now" gate
/// to avoid traversing the buffer for dedup on every `rc_dec`.
pub const FLAG_BUFFERED: u16 = 1 << 5;
/// `Tag::Obj` instance is an Error-derived class (Error itself or a
/// transitive `extends Error` subclass — TypeError/RangeError/… and
/// user subclasses). Set at the `__new_<C>` factory alloc site so the
/// uncaught-throw reporter can render `name: message` (fields are at
/// the Error layout prefix: message @ field0, name @ field1). Bit 6 is
/// taken by torajs-dynobj's NULL_PROTO marker (Tag::DynObj, disjoint
/// tag), so this uses bit 7.
pub const FLAG_ERROR: u16 = 1 << 7;

/// `Object.preventExtensions(obj)` / `Object.seal(obj)` — clear the
/// `[[Extensible]]` internal slot per spec §10.1. Default 0 means the
/// object is extensible (the common case), so fresh allocs do not
/// need to touch this bit. `Object.isExtensible` reads it; `seal` sets
/// it AND clears every entry's `configurable` flag.
pub const FLAG_NON_EXTENSIBLE: u16 = 1 << 8;

/// `Object.seal(obj)` marker — distinguishes "user explicitly called
/// `Object.seal`" from "user called `Object.preventExtensions` but not
/// `seal`". `isSealed` returns true iff this bit OR the DynObj entry
/// walk reports all-non-configurable; pure prevent-only sets
/// [`FLAG_NON_EXTENSIBLE`] alone and leaves this clear, matching bun's
/// "typed class instance after `preventExtensions` is not sealed (own
/// keys still configurable per spec)" semantics.
pub const FLAG_SEALED: u16 = 1 << 9;

/// `delete fn.name` tombstone (Tag::Closure cells; RFC
/// 20260711-closure-reflection chunk C). tr carries the ES §20.2.4
/// `name` / `length` own properties virtually off the fn-metadata
/// chain — a successful delete (they are configurable) sets the bit
/// so every reader (has / enumerable / member read / gOPD) skips
/// the virtual answer; a later write recreates a plain expando
/// entry. Interned method cells share the bit process-wide — the
/// spec object is a singleton, so `delete String.prototype.slice.name`
/// is a global effect in bun too.
///
/// Bits 10-11 are Tag::Closure-private (disjoint-by-tag reuse of the
/// Tag::Arr element-kind field, bits 10-12); see the module-doc
/// occupancy map for why 13-14 are off-limits.
pub const FLAG_FN_NAME_DELETED: u16 = 1 << 10;
/// `delete fn.length` tombstone — see [`FLAG_FN_NAME_DELETED`].
pub const FLAG_FN_LENGTH_DELETED: u16 = 1 << 11;
/// `Tag::DynObj` cell that is a class-constructor object — the
/// `__class_<C>` singleton dynobj, marked by
/// `__torajs_anyv_class_register` at module init (RFC
/// 20260717-class-first-class-value knife A). ES models class
/// constructors as function objects, but tr's class object is a
/// dynobj whose tag alone reads "object"; `typeof` answers
/// `"function"` off this bit. Bit 10 is Tag::DynObj-private
/// (disjoint-by-tag reuse of Tag::Closure's [`FLAG_FN_NAME_DELETED`]
/// / Tag::Arr's element-kind field).
pub const FLAG_DYNOBJ_CLASS_CTOR: u16 = 1 << 10;
/// `Tag::DynObj` cell carrying the ES §25.5.1 `[[IsRawJSON]]`
/// internal slot — minted only by `JSON.rawJSON` (torajs-anyvalue
/// `json_raw.rs`): a frozen null-prototype object whose single
/// `"rawJSON"` own property holds a validated scalar JSON text.
/// `JSON.isRawJSON` answers off this bit, and the any-lane
/// `JSON.stringify` walk splices the stored text verbatim instead of
/// serializing the object shape. Bit 11 is Tag::DynObj-private
/// (disjoint-by-tag reuse of Tag::Closure's
/// [`FLAG_FN_LENGTH_DELETED`] / Tag::Arr's element-kind field).
pub const FLAG_DYNOBJ_RAW_JSON: u16 = 1 << 11;
/// Tag::Closure cell whose lifted body takes the call-site `this` as
/// its first declared param (`(__env, __this, ...user)`) — a
/// function-expression accessor face (RFC 20260717-fnexpr-this-channel
/// knife 1). Receiver-aware invokers (the AccessorPair boxed channel)
/// read the bit to put the receiver in argv[0]; the closure never
/// reaches a receiver-unaware call path (the AST pass promotes only
/// zero-alias inline face positions). Bit 12 is Tag::Closure-private
/// (disjoint-by-tag reuse of Tag::Arr's element-kind field, bits
/// 10-12); 13-14 stay off-limits per the module-doc occupancy map.
pub const FLAG_CLOSURE_RECV_FIRST: u16 = 1 << 12;
/// Tag::Closure reified class-method face whose mono body never
/// reads its receiver (S2.38 — the compiler proves `__this` unused
/// at the SSA level and bakes the bit through the MethodMeta flags
/// word). A bare / primitive-`this` call of such a face runs the
/// adapter with a null receiver instead of the this-undefined
/// TypeError — ES §10.2.1.2 runs a this-free body regardless of the
/// thisArgument. Bit 6 is Tag::Closure-private (disjoint-by-tag
/// reuse of the DynObj NULL_PROTO bit).
pub const FLAG_CLASS_METHOD_THIS_FREE: u16 = 1 << 6;
/// `Tag::Arr` cell whose logical `len` exceeds its materialized slot
/// extent (RFC 20260810-arr-sparse-grow) — `arr.length = N` past the
/// dense limit writes only the length field (§10.4.2.4 grow is
/// O(1); the tail is implicit holes with no storage, no shadow
/// entries). Invariant while set: the deque head is 0 and the
/// materialized extent equals `cap` (the sparse transition compacts
/// the head and trims the buffer), so `min(len, cap)` is the bound
/// every slot-buffer walk must use instead of `len`
/// (torajs-arr `layout::arr_live_extent`). Only ever set on a
/// [`FLAG_ARR_ANY`] cell — typed arrays keep the materializing grow,
/// so the typed static load/store emit never meets a sparse tail.
/// Bit 6 is Tag::Arr-private (disjoint-by-tag reuse of the DynObj
/// NULL_PROTO / Closure this-free bit).
pub const FLAG_ARR_SPARSE_TAIL: u16 = 1 << 6;
/// `Tag::Arr` cell carries at least one array index with non-default
/// property attributes (RFC 20260712-arr-exotic-define chunk B) — set
/// by `Object.defineProperty(arr, index, desc)` when the resulting
/// per-index flags differ from the implicit `{writable: true,
/// enumerable: true, configurable: true}`. The flags live as shadow
/// entries (canonical index key, value slot dead) in the array's
/// expando props dynobj; readers (gOPD / element writes / delete)
/// fast-path on this bit staying clear. Bit 15 is Tag::Arr-private.
pub const FLAG_ARR_EXOTIC_INDEX: u16 = 1 << 15;
/// `Tag::Obj` class instance carries at least one DECLARED field with
/// non-default property attributes (RFC
/// 20260806-declared-field-redefine) — the exact mirror of
/// [`FLAG_ARR_EXOTIC_INDEX`] one tag over, down to the mechanism: the
/// attributes live as a shadow entry (field-name key, value slot
/// dead) in the instance's `+24` expando dict, and readers fast-path
/// on this bit staying clear.
///
/// Deliberately NOT raised by an expando write — see
/// [`FLAG_OBJ_EXPANDO`], which is a different question asked by
/// different readers. Sharing one bit would have made every store to a
/// declared field of an instance that happens to carry a dynamic
/// property probe that dict for a sidecar that is not there.
///
/// It is what lets the typed `o.field = v` store stay one store. That
/// site already loads the header to check FROZEN, so the writability
/// question costs it one wider immediate in the same test — and the
/// dict is consulted only on an instance that was actually
/// redefined. Bit 15 is Tag::Obj-private (disjoint-by-tag reuse of
/// Arr's exotic-index bit and Closure's `prototype` bit).
pub const FLAG_OBJ_EXOTIC_FIELD: u16 = 1 << 15;
/// `Tag::Obj` class instance owns at least one key its layout never
/// mentions — a dynamic property (RFC 20260714-struct-dynamic-props),
/// whether written as `o.k = v` through the `any` lane or defined
/// through `Object.defineProperty`.
///
/// The readers are the surfaces that unfold a compile-time member
/// list: they answer only what the layout declares, which stops being
/// the whole own set the moment this bit goes up. They test it
/// together with [`FLAG_OBJ_EXOTIC_FIELD`] in one masked compare, so
/// the second question is free to them — and invisible to the typed
/// store, which asks only the first.
///
/// Bit 12 is Tag::Obj-private (disjoint-by-tag with Arr's element-kind
/// field at 10-12 and the Closure / DynObj bits at 10-11).
pub const FLAG_OBJ_EXPANDO: u16 = 1 << 12;
/// `Tag::Arr` `length` property lock (RFC 20260712-arr-exotic-define
/// chunk D) — `Object.defineProperty(arr, "length", {writable:
/// false})` sets it; every later length mutation (assign / define /
/// fresh-index append through defineProperty) throws. §10.4.2.4 makes
/// the lock one-way (non-configurable length can never regain
/// writability). Bit 7 is Tag::Arr-private (disjoint-by-tag reuse of
/// Tag::Obj's [`FLAG_ERROR`]).
pub const FLAG_ARR_LENGTH_RO: u16 = 1 << 7;
/// `Tag::Closure` cell minted from an `async` function form (RFC
/// 20260721-builtin-method-reflection 刀 4+9) — its `.constructor`
/// reflects %AsyncFunction% and the fn owns NO `prototype` property
/// (§27.7.2.2: async functions are not constructors). Stamped by the
/// compiler's closure-env alloc off the parser's async side-channels.
/// Bit 7 is Tag::Closure-private (disjoint-by-tag reuse of
/// [`FLAG_ERROR`] / [`FLAG_ARR_LENGTH_RO`]).
pub const FLAG_FN_ASYNC: u16 = 1 << 7;
/// `Tag::Closure` cell minted from a PLAIN `function` form (decl or
/// expression — not arrow / async / generator) — it owns a lazily
/// materialized `.prototype` object per §10.2.5 MakeConstructor
/// (writable, non-enumerable, with a `constructor` back-reference).
/// Bit 15 is Tag::Closure-private (disjoint-by-tag reuse of
/// [`FLAG_ARR_EXOTIC_INDEX`]).
pub const FLAG_FN_PROTO: u16 = 1 << 15;
/// `Tag::Closure` cell that is a GENERATOR factory (RFC 20260721
/// 刀 2) — its [[Prototype]] is `%GeneratorFunction.prototype%`
/// (§27.3; the torajs-meta genfn trio), not %Function.prototype%;
/// combined with [`FLAG_FN_ASYNC`] it marks an async generator
/// (§27.4). Bit 3 is Tag::Closure-private (disjoint-by-tag reuse of
/// [`FLAG_ARR_ANY`]).
pub const FLAG_FN_GENERATOR: u16 = 1 << 3;

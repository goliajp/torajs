//! Per-heap-object type tag (`Tag`) — split out of `lib.rs`
//! (file-size limit, RFC 20260710 C2b chunk). Re-exported at the
//! crate root (`torajs_rc::Tag`), all call sites unchanged.

/// Per-heap-object type tag stored in [`HeapHeader::type_tag`].
/// Drives drop dispatch in `__torajs_value_drop_heap` (still in
/// the glue C for now; rewrite of dispatch is queued for the
/// later phase — see `docs/architecture-rewrite.md`).
///
/// Values are stable wire-format; do not renumber. Adding a new
/// type takes the next free integer + a new variant here + a
/// new `case` in the dispatcher.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// `Str` — `[header:8][len:8][bytes:N]`.
    Str = 0,
    /// `Obj` — static-layout class instance / property bag.
    Obj = 1,
    /// `Arr<T>` — head-aware deque.
    Arr = 2,
    /// `Closure` — env-first ABI; the cell is the env:
    /// `{ hdr | fn_ptr@8 | drop_fn@16 | props@24 | boxed_entry@32 |
    /// trace_fn@40 | caps@48+ }` (see torajs-core `ssa_lower.rs`
    /// CLOSURE_* offsets).
    Closure = 3,
    /// `RegExp` — compiled NFA + flags.
    RegExp = 4,
    /// `Date` — `{ ms_since_epoch }`.
    Date = 5,
    /// **Reserved (was `AnyBox`).** The boxed `Type::Any` heap
    /// struct was removed in v0.7 Step 7 NaN-box AnyValue cutover;
    /// the discriminant `6` is kept reserved so later variants
    /// don't shift in the wire ABI.
    Reserved6 = 6,
    /// `Symbol` — `{ desc_str_ptr }`.
    Symbol = 7,
    /// `Promise<T>` — own drop path (not via value_drop_heap).
    Promise = 8,
    /// `fetch()` `Response`.
    Response = 9,
    /// `BigInt` — sign-magnitude limbs.
    BigInt = 10,
    /// `WeakRef<T>` — `{ target_ptr | null }`.
    WeakRef = 11,
    /// `WeakMap<K, V>`.
    WeakMap = 12,
    /// `WeakSet<K>`.
    WeakSet = 13,
    /// Dynamic-property object (HashMap-backed).
    DynObj = 14,
    /// Strong-ref `Map<K, V>`.
    Map = 15,
    /// `MapIter` — stateful Map iterator.
    MapIter = 16,
    /// `ArrIter` — stateful Array<Any> iterator.
    ArrIter = 17,
    /// `AccessorPair` — `{ get_closure, set_closure }` backing a
    /// dynobj property defined with a get/set descriptor (RFC C3).
    /// Stored as the entry's `value_anyv` cell; resolved by reading
    /// the pointee's `HeapHeader::type_tag` (the NaN-box itself has no
    /// free tag space — V8/JSC AccessorPair model).
    AccessorPair = 18,
    /// Strong-ref `Set<T>` — shares the `Map` heap layout
    /// (`torajs-collections::layout::Map`, Set stores entries with
    /// `value_anyv = ANY_UNDEF`) but gets its own type_tag so the
    /// AnyValue tag-walker (inspect.rs) can route to the bun-correct
    /// `Set(N) {…}` / `Set {}` printer instead of mis-printing as Map.
    Set = 19,
    /// The JS `undefined` oddball (RFC 20260710 C2b) — the tag of the
    /// one immortal [`crate::undef_cell::__TORAJS_UNDEF_CELL`] header
    /// block that pointer-shaped struct slots (Obj / Arr / Closure)
    /// store as their in-band `undefined` repr (NULL keeps meaning
    /// JS `null`). Never allocated at runtime; identity-compared by
    /// every consumer. Runtime tag-dispatch walkers route it to
    /// their rc-gated catch-alls (`FLAG_STATIC_LITERAL` short-
    /// circuits every rc/drop path).
    Undefined = 20,
    /// `NumberWrapper` — `[header:8][num:8]` fixed 16B block for
    /// `new Number(x)` (spec §21.1.1.1). `[[NumberData]]` stored
    /// verbatim; substrate implemented in `torajs-wrapper`
    /// (RFC 20260716-primitive-wrapper-substrate 刀 1).
    NumberWrapper = 21,
    /// `StringWrapper` — `[header:8][str_cell_ptr:8]` fixed 16B block
    /// for `new String(x)` (spec §22.1.1.1). Holds an owning `+1`
    /// reference to a Tag::Str cell as `[[StringData]]`.
    StringWrapper = 22,
    /// `BooleanWrapper` — `[header:8][val:1 + pad:7]` fixed 16B block
    /// for `new Boolean(x)` (spec §20.3.1.1). `[[BooleanData]]` is
    /// 0 (false) or 1 (true).
    BooleanWrapper = 23,
    /// `SymbolWrapper` — `[header:8][sym_cell_ptr:8][props_slot:8]`
    /// block for `Object(sym)` (spec §7.1.18 ToObject step for
    /// Symbol). Holds an owning `+1` reference to a Tag::Symbol cell
    /// as `[[SymbolData]]`. No constructor form exists (`new
    /// Symbol()` throws per §20.4.1.1) — only the callable-coercion
    /// mint path allocates it.
    SymbolWrapper = 24,
    /// Iterator Helper cell (§27.1.4.x lazy helpers — `.map(fn)` et
    /// al.): `{ header:8 | underlying:8 | fn:8 | counter:8 | kind:1
    /// alive:1 pad:6 | inner:8 }`. Owns its underlying iterator and
    /// captured callback; substrate in `torajs-anyvalue::iter_helper`
    /// (RFC 20260730-iterator-global 刀 2).
    IterHelper = 25,
    /// `Proxy` — `{ header:8 | target:8 | handler:8 }` (24 B), both
    /// slots owning `AnyValue`s (RFC 20260823-proxy-substrate 刀 1).
    /// A revoked proxy stores `null` in both, exactly as §10.5.4.1
    /// says — `is_null(handler)` IS the revoked predicate, so there
    /// is no second flag byte that could drift away from it.
    Proxy = 26,
    /// `ArrayBuffer` — `{ header:8 | data:8 | byte_len:8 |
    /// max_byte_len:8 }` (32 B). `data == null` IS detached
    /// (§25.1.3.3 writes null and there is nothing else to read) and
    /// `max_byte_len == -1` IS "no `[[ArrayBufferMaxByteLength]]`" —
    /// absent is a real state and is not a maximum of zero. Substrate
    /// in `torajs-buffer` (RFC 20260823-typedarray-substrate 刀 1).
    ArrayBuffer = 27,
    /// `TypedArray` — the §10.4.5 integer-indexed exotic view:
    /// `{ header:8 | buffer:8 (AnyValue) | byte_offset:8 |
    /// array_len:8 | kind:1 pad:7 }`. `array_len == -1` is a
    /// length-tracking view, whose length is re-derived from the
    /// buffer on every access.
    TypedArray = 28,
    /// `DataView` — `{ header:8 | buffer:8 (AnyValue) |
    /// byte_offset:8 | byte_len:8 }`.
    DataView = 29,
}

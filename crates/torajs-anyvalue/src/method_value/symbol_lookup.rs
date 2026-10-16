//! Well-known-symbol method reify off a native receiver tag — the
//! `[Symbol.iterator]` F0 table plus the RegExp §22.2.6 protocol row
//! (r289), split out of the parent when the RegExp arm pushed it past
//! the 500-line cap.
//!
//! The `[Symbol.iterator]` fact is asked from two directions and both
//! live here so they cannot drift apart: [`builtin_symbol_method_lookup`]
//! answers it off an INSTANCE's heap tag (`m[Symbol.iterator]`), and
//! [`__torajs_proto_symbol_keys_install`] writes it as a real own entry on
//! the PROTOTYPE singleton (`Map.prototype[Symbol.iterator]`). Both
//! hand out the same interned cell, which is what §24.1.3.14's "the
//! initial value is the entries function" requires: the two reads and
//! `Map.prototype.entries` are one function object, not three.

use core::ffi::c_void;

use torajs_rc::Tag;

use super::{builtin_method_cell, symbol_static};

/// §22.2.6's RegExp protocol methods as (alphabetical WELL_KNOWN_NAMES
/// index, mid), in the clause order their prototype entries are
/// created — @@match, @@matchAll, @@replace, @@search, @@split. The
/// instance read and the prototype install both walk this one table.
const REGEXP_PROTOCOL: [(i64, i64); 5] = [
    (6, torajs_rc::ANY_METHOD_MATCH),
    (7, torajs_rc::ANY_METHOD_MATCH_ALL),
    (8, torajs_rc::ANY_METHOD_REPLACE),
    (9, torajs_rc::ANY_METHOD_SEARCH),
    (11, torajs_rc::ANY_METHOD_SPLIT),
];

/// The RegExp proto row (`family.rs`), which is also its prototype
/// tag (`torajs-rc/builtin_proto.rs`: RegExp = 7).
pub(crate) const REGEXP_FAMILY: i64 = 7;

/// Is `mid` one of the [`REGEXP_PROTOCOL`] methods?
pub(crate) fn regexp_protocol_mid(mid: i64) -> bool {
    REGEXP_PROTOCOL.iter().any(|&(_, m)| m == mid)
}

/// §20.2.4.2 SetFunctionName over a symbol key — each protocol
/// method's `name` is its key's description in brackets, in
/// [`REGEXP_PROTOCOL`] order.
const REGEXP_PROTOCOL_NAMES: [&[u8]; 5] = [
    b"[Symbol.match]",
    b"[Symbol.matchAll]",
    b"[Symbol.replace]",
    b"[Symbol.search]",
    b"[Symbol.split]",
];

static REGEXP_PROTOCOL_NAME_CELLS: [core::sync::atomic::AtomicU64; 5] =
    [const { core::sync::atomic::AtomicU64::new(0) }; 5];

/// The interned `name` of the reified method cell `ptr` carrying
/// `mid`: the symbol-keyed name for a RegExp protocol method, the
/// method-table `name` for every other one.
///
/// # Safety
/// `ptr` is a live reified method cell.
pub(crate) unsafe fn method_name_cell_of(
    ptr: *mut c_void,
    mid: i64,
    name: &'static str,
) -> *mut u8 {
    if unsafe { super::family::builtin_method_family(ptr) } == REGEXP_FAMILY
        && let Some(cell) = regexp_protocol_name_cell(mid)
    {
        return cell;
    }
    super::builtin_method_name_cell(mid, name)
}

/// The interned `name` of the RegExp protocol method `mid`, or `None`
/// for any other id.
fn regexp_protocol_name_cell(mid: i64) -> Option<*mut u8> {
    use core::sync::atomic::Ordering;
    let i = REGEXP_PROTOCOL.iter().position(|&(_, m)| m == mid)?;
    let slot = &REGEXP_PROTOCOL_NAME_CELLS[i];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return Some(p as *mut u8);
    }
    let cell = super::mint_immortal_str(REGEXP_PROTOCOL_NAMES[i]);
    slot.store(cell as u64, Ordering::Relaxed);
    Some(cell)
}

/// Well-known-symbol method read off a native tag.
///
/// `[Symbol.iterator]` — RFC 20260728-gen-forof-yieldstar F0. The
/// spec aliases each builtin's @@iterator to a named prototype method
/// (§23.1.3.40 Array → `values`, §24.1.3.12 Map → `entries`,
/// §24.2.3.11 Set → `values`), so the reify answers the SAME interned
/// cell as the named read — `a[Symbol.iterator] === a.values` holds
/// like the Set keys/values alias above.
///
/// RegExp protocol methods (r289) — §22.2.6's @@match / @@matchAll /
/// @@replace / @@search / @@split reify against the RegExp family
/// row; the index-call leg re-dispatches their mid with the receiver
/// in place, and the mid arm delegates to the Str home with the
/// operand order flipped (`re[@@match](s)` ≡ `s.match(re)`).
///
/// `None` for every other key / receiver tag (the symbol lane's dict
/// miss stays undefined).
///
/// # Safety
/// `key` is a live Symbol cell.
pub(crate) unsafe fn builtin_symbol_method_lookup(
    recv_tag: u16,
    key: *const c_void,
) -> Option<*mut u8> {
    // Alphabetical WELL_KNOWN_NAMES indices; the singletons are
    // immortal so pointer identity IS symbol identity.
    if recv_tag == Tag::RegExp as u16 {
        for (idx, mid) in REGEXP_PROTOCOL {
            if key == symbol_static::well_known_singleton(idx) {
                return Some(builtin_method_cell(REGEXP_FAMILY, mid));
            }
        }
        return None;
    }
    // Index 2 = "dispose" — §27.1.4.1 %Iterator.prototype%
    // [@@dispose], inherited by every iterator-protocol cell (RFC
    // 20260809 B6). The Iterator family row (proto tag 15), like the
    // return-this reify below.
    if key == symbol_static::well_known_singleton(2)
        && (recv_tag == Tag::MapIter as u16
            || recv_tag == Tag::ArrIter as u16
            || recv_tag == Tag::IterHelper as u16)
    {
        return Some(builtin_method_cell(
            15,
            torajs_rc::any_method_iter::ANY_METHOD_ITER_DISPOSE,
        ));
    }
    // Index 5 = "iterator".
    let iter_sym = symbol_static::well_known_singleton(5);
    if key != iter_sym {
        return None;
    }
    let (family, mid) = match recv_tag {
        t if t == Tag::Arr as u16 => (2, torajs_rc::ANY_METHOD_VALUES),
        t if t == Tag::Map as u16 => (11, torajs_rc::ANY_METHOD_ENTRIES),
        // §23.2.3.36 — `%TypedArray%.prototype[@@iterator]` IS the
        // same function object as `%TypedArray%.prototype.values`,
        // so it reifies on the row the named read answers from:
        // %TypedArray.prototype%'s (RFC 20260916-typedarray-intrinsic).
        t if t == Tag::TypedArray as u16 => (
            super::family::TYPEDARRAY_PROTO_FAMILY,
            torajs_rc::ANY_METHOD_VALUES,
        ),
        t if t == Tag::Set as u16 => (12, torajs_rc::ANY_METHOD_VALUES),
        // §22.1.3.36 — no named alias; the dedicated own id (a
        // Substr view shares Tag::Str). A StringWrapper inherits
        // the same String.prototype face (§22.1.5's this is
        // ToString-generic).
        t if t == Tag::Str as u16 || t == Tag::StringWrapper as u16 => {
            (3, torajs_rc::ANY_METHOD_STR_ITERATOR)
        }
        // §27.1.2.1 — iterator cells inherit the
        // %Iterator.prototype% return-this (Iterator family row,
        // proto tag 15; RFC 20260730-iterator-global 刀 4 长尾).
        t if t == Tag::MapIter as u16
            || t == Tag::ArrIter as u16
            || t == Tag::IterHelper as u16 =>
        {
            (15, torajs_rc::any_method_iter::ANY_METHOD_ITER_SELF)
        }
        _ => return None,
    };
    Some(builtin_method_cell(family, mid))
}

unsafe extern "C" {
    /// torajs-dynobj — §10.1.6.3 define with explicit W/E/C flags
    /// (`flags_byte` low 3 = values, bits 3-5 = present, bit 6 =
    /// value present). Consumes one rc of a heap `value`.
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-arr — the same define against an array's side props
    /// (`Array.prototype` is an Arr cell, §23.1.3), allocating the
    /// props dynobj on first write.
    fn __torajs_arrprops_define(
        arr: *mut c_void,
        key: *const c_void,
        tag: i64,
        value: i64,
        flags_byte: u64,
    );
    /// torajs-dynobj — the §23.1.3.44 unscopables object's mint pair,
    /// plus the null-[[Prototype]] mark that clause requires.
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
    fn __torajs_dynobj_set_fresh(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    );
    /// torajs-str — mint / release a Str cell for one of those keys
    /// (the entry takes its own share on insert).
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// `ANY_HEAP` slot tag (torajs-dynobj `layout.rs` mirror).
const ANY_HEAP: u64 = 4;

/// Entry attrs {W:1, E:0, C:1} — §23.1.3.40 / §24.1.3.14 / §24.2.3.13
/// / §22.1.3.36 all give `[Symbol.iterator]` the standard method
/// attributes. Writable + configurable set, all three present, value
/// present (flag-byte mirror of `torajs_dynobj::layout::DEFINE_*`).
const METHOD_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2) | 1;

/// Which named prototype method a builtin prototype's
/// `[Symbol.iterator]` IS, as the (family row, mid) its interned cell
/// lives under. `None` for a prototype the spec gives no such entry.
///
/// The aliases mirror the instance table above — §23.1.3.40 Array →
/// `values`, §24.1.3.14 Map → `entries`, §24.2.3.13 Set → `values`,
/// §23.2.3.36 %TypedArray% → `values`;
/// §22.1.3.36 String has no named alias and carries a dedicated id.
/// Tags are `torajs-rc/builtin_proto.rs` order.
///
/// The Array row was taken back out once (rotation 383) for a SIGTRAP
/// it was blamed for. That blame was wrong: the fault was a
/// use-after-free already present without this row, and this row only
/// shifted binary layout enough to make it trap. It is fixed at its
/// source in `ssa_lower_object_lit.rs` (rotation 384).
pub(crate) fn proto_tag_installs_iterator(proto_tag: i64) -> bool {
    proto_tag_iterator_alias(proto_tag).is_some()
}

/// The family prototype's `[Symbol.iterator]` alias — `(family, mid)`.
fn proto_tag_iterator_alias(proto_tag: i64) -> Option<(i64, i64)> {
    Some(match proto_tag {
        2 => (2, torajs_rc::ANY_METHOD_VALUES),
        3 => (3, torajs_rc::ANY_METHOD_STR_ITERATOR),
        11 => (11, torajs_rc::ANY_METHOD_ENTRIES),
        12 => (12, torajs_rc::ANY_METHOD_VALUES),
        // §23.2.3.36 — %TypedArray.prototype%'s `[Symbol.iterator]`
        // is its `values`.
        38 => (38, torajs_rc::ANY_METHOD_VALUES),
        _ => return None,
    })
}

/// §22.2.6.8 / .9 / .11 / .12 / .15 — `RegExp.prototype`'s five
/// symbol-keyed protocol methods as real own entries, the same interned
/// cells an instance's `re[Symbol.search]` answers. Before this only
/// the instance read existed: `RegExp.prototype[Symbol.search]` read
/// undefined, so every `.call(fakeRe, …)` spelling of the protocol —
/// the way the spec's own tests drive it with a plain object as `this`
/// — threw on calling undefined.
///
/// # Safety
/// `proto` is the freshly allocated `RegExp.prototype` dynobj.
unsafe fn install_regexp_protocol(proto: *mut c_void) {
    let mut slot = proto;
    for (idx, mid) in REGEXP_PROTOCOL {
        let key = symbol_static::well_known_singleton(idx);
        if key.is_null() {
            return;
        }
        let cell = builtin_method_cell(REGEXP_FAMILY, mid);
        unsafe {
            __torajs_dynobj_define_plain(
                &mut slot,
                key as *mut c_void,
                ANY_HEAP,
                cell as u64,
                METHOD_ENTRY_FLAGS,
            )
        };
    }
    REGEXP_PROTO_MINT_LEN.store(
        unsafe { (slot.cast::<u8>().add(DYNOBJ_ENTRIES_LEN_OFF) as *const u32).read() },
        core::sync::atomic::Ordering::Relaxed,
    );
}

/// torajs-dynobj header mirrors: live-entry count and the dense entry
/// array's used length (holes included — it only grows until a
/// resize compacts it).
pub(crate) const DYNOBJ_COUNT_OFF: usize = 8;
pub(crate) const DYNOBJ_ENTRIES_LEN_OFF: usize = 16;

/// The dense entry length %RegExp.prototype% had when its mint
/// finished installing the protocol methods. Any entry a program adds
/// later appends past it, and a delete drops the live count below it.
pub(crate) static REGEXP_PROTO_MINT_LEN: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(u32::MAX);

/// §23.1.3.44 — the method names a `with` block must NOT bring into
/// scope, in the clause's creation order. `Object.keys` on the
/// unscopables object answers that order, so it is observable and this
/// list is not free to be sorted.
const UNSCOPABLE_KEYS: [&str; 16] = [
    "at",
    "copyWithin",
    "entries",
    "fill",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flat",
    "flatMap",
    "includes",
    "keys",
    "toReversed",
    "toSorted",
    "toSpliced",
    "values",
];

/// Entry attrs {W:0, E:0, C:1} — §23.1.3.44 gives the unscopables
/// entry itself a NON-writable face, unlike the method entries above.
/// Same flag-byte encoding with the writable value bit cleared.
const UNSCOPABLES_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

/// Build §23.1.3.44's unscopables object and define it on
/// `Array.prototype`. Every entry is `true` with the default data
/// attributes CreateDataPropertyOrThrow gives, on an object whose
/// [[Prototype]] is null.
///
/// Pre-fix the property simply did not exist: reading
/// `Array.prototype[Symbol.unscopables]` answered undefined and
/// `getOwnPropertySymbols(Array.prototype)` listed one symbol where bun
/// lists two. Same "behaviour right, property absent" shape rotation
/// 382 closed for `@@toStringTag` and 383 for the `@@iterator` alias --
/// except here there is no behaviour either, since `with` is the only
/// consumer and tr has no `with`.
///
/// # Safety
/// `proto` is the freshly allocated `Array.prototype` Arr cell.
unsafe fn install_unscopables(proto: *mut c_void) {
    // Index 14 = "unscopables" in the alphabetical well-known table.
    let key = symbol_static::well_known_singleton(14);
    if key.is_null() {
        return;
    }
    let obj = unsafe { __torajs_dynobj_alloc() };
    if obj.is_null() {
        return;
    }
    unsafe { __torajs_dynobj_mark_null_proto(obj) };
    let mut slot = obj;
    for name in UNSCOPABLE_KEYS {
        let k = unsafe { __torajs_str_alloc(name.as_ptr(), name.len() as i64) };
        // Tag 1 = AnySlotTag::Bool, value 1 = `true`. The entry takes
        // its own share of the key, so the mint's stake is ours to
        // give back.
        unsafe { __torajs_dynobj_set_fresh(&mut slot, k as *mut c_void, 1, 1) };
        unsafe { __torajs_str_drop(k as *mut c_void) };
    }
    // The define consumes the fresh object's only stake, and the key
    // is the immortal well-known singleton.
    unsafe {
        __torajs_arrprops_define(
            proto,
            key,
            ANY_HEAP as i64,
            slot as i64,
            UNSCOPABLES_ENTRY_FLAGS,
        )
    };
}

/// Define the well-known-symbol own entries a freshly minted builtin
/// prototype's spec clause gives it — `[Symbol.iterator]` for the four
/// iterable families, plus `[Symbol.unscopables]` on `Array.prototype`
/// — and nothing for the rest.
///
/// `Array.prototype` takes the other kernel: §23.1.3 makes it an
/// array exotic object, so its own entries live in the Arr side props
/// rather than in a dynobj.
///
/// Before this the fact only existed on the instance side: reading
/// `Map.prototype[Symbol.iterator]` answered undefined while
/// `new Map()[Symbol.iterator]` answered the entries cell, and
/// `getOwnPropertySymbols(Map.prototype)` never listed it — the same
/// "behaviour right, property absent" shape rotation 382 closed for
/// `@@toStringTag`.
///
/// Entries go in SPEC CLAUSE ORDER, because §10.1.11.1 lists own symbol
/// keys in creation order: §23.1.3.40 `@@iterator` before §23.1.3.44
/// `@@unscopables`, which is what `getOwnPropertySymbols` must answer.
///
/// Called from the builtin-proto mint before the CAS install, so a
/// race loser leaks a fully-formed dynobj (the posture every sibling
/// install shares).
///
/// # Safety
/// FFI face; `proto` is NULL or the freshly allocated dynobj.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proto_symbol_keys_install(proto: *mut c_void, idx: i64) {
    if proto.is_null() {
        return;
    }
    // §23.2.7.1 — a per-kind typed-array prototype's one data constant
    // (it has no iterator alias, so this runs before the early return).
    unsafe {
        crate::closure_proto::typedarray_proto_accessor::install_bytes_per_element(proto, idx)
    };
    // §20.4.3.5 / §21.4.4.45 — before the Symbol prototype's
    // `@@toStringTag` (the mint installs that next), in clause order.
    unsafe { crate::closure_proto::to_primitive_proto::install(proto, idx) };
    if idx == REGEXP_FAMILY {
        unsafe { install_regexp_protocol(proto) };
        return;
    }
    let is_array = idx == torajs_rc::builtin_proto::ARRAY_PROTO_TAG as i64;
    let Some((family, mid)) = proto_tag_iterator_alias(idx) else {
        return;
    };
    // Index 5 = "iterator" in the alphabetical well-known table.
    let key = symbol_static::well_known_singleton(5);
    if key.is_null() {
        return;
    }
    // The define takes its own stake on key and value; both are
    // process-lifetime immortals (the well-known symbol singleton and
    // the interned method cell), so neither stake is ever given back.
    let cell = builtin_method_cell(family, mid);
    if is_array {
        unsafe {
            __torajs_arrprops_define(proto, key, ANY_HEAP as i64, cell as i64, METHOD_ENTRY_FLAGS)
        };
        unsafe { install_unscopables(proto) };
        return;
    }
    let mut slot = proto;
    unsafe {
        __torajs_dynobj_define_plain(
            &mut slot,
            key as *mut c_void,
            ANY_HEAP,
            cell as u64,
            METHOD_ENTRY_FLAGS,
        )
    };
    // §23.2.3.38 — %TypedArray.prototype%'s `[Symbol.toStringTag]`
    // accessor, after its `[Symbol.iterator]`.
    if idx == torajs_rc::builtin_proto::TYPEDARRAY_PROTO_TAG as i64 {
        unsafe { crate::closure_proto::typedarray_proto_accessor::install(slot) };
    }
}

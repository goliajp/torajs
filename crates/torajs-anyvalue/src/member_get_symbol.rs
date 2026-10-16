//! §6.1.7 symbol-keyed member reads — the `o[sym]` lane.
//!
//! A symbol key is a wholly separate key domain from a string key, so
//! it gets its own short walk instead of threading through the
//! string cascade in [`crate::member_get`]. Every arm of that cascade
//! is a three-step shape — entry-table probe, then **name-keyed**
//! fallbacks (`key_is(key, b"prototype")`, the virtual `name` /
//! `length` pair, canonical-index decode, builtin-prototype method
//! reify), then the prototype chain. Only the first and last steps
//! mean anything for a symbol key, and the middle ones would read Str
//! payload offsets off a 16-byte Symbol cell.
//!
//! So this lane is exactly: probe the receiver's own property dict,
//! then walk what it inherits. §10.1.8.1 OrdinaryGet does not care
//! which key kind it is looking up, which is why the chain walk is
//! shared and the recursion covers grandparents.
//!
//! Both `get_tag` and `get_value` route here, so the pair is resolved
//! once per shape and the two extern faces cannot disagree.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::recv_cell;
use crate::nanbox::AnyValue;

pub(crate) mod dict;

use dict::{InheritedFrom, chain_next, inherited_dict, own_dict, probe_dict};

unsafe extern "C" {
    /// torajs-meta — borrow read of `PROTOS_BY_TAG_IMM[tag]`
    /// (RFC 20260802 刀 3a struct symbol-key inheritance).
    fn __torajs_obj_proto_borrowed(cell: *const c_void) -> u64;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// Disambiguates "absent" from "present, storing undefined" — both
    /// answer tag 5 from `get_tag`.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-throw — did the getter leave a pending throw?
    fn __torajs_throw_check() -> i64;
    /// nanbox decode for the value a getter returned (owned box).
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// `torajs_dynobj::layout::TAG_SYMBOL_KEY` mirror — a property-key
/// cell is a Str (tag 0) or a Symbol (tag 7).
const TAG_SYMBOL_KEY: u16 = 7;

/// `AnySlotTag::Undef` — the absent / undefined answer.
pub(crate) const TAG_UNDEF: u64 = 5;

/// True when the property-key cell is a Symbol rather than a Str.
///
/// # Safety
/// `key` must be non-NULL and point at a live key cell.
#[inline]
pub(crate) unsafe fn key_is_symbol(key: *const c_void) -> bool {
    unsafe { key.cast::<u8>().add(4).cast::<u16>().read() == TAG_SYMBOL_KEY }
}

/// §10.1.8.1 OrdinaryGet for a symbol key, with "absent" kept apart
/// from "present, storing undefined" — `None` is the genuine miss the
/// `in` face needs, `Some(pair)` is the read one.
///
/// Borrow-shaped like the string-key pair helpers: no rc traffic, the
/// dict keeps its own share of the value.
///
/// # Safety
/// Cell receivers are live heap pointers; `key` is a live Symbol cell.
unsafe fn symbol_key_probe(recv: AnyValue, key: *const c_void) -> Option<(u64, u64)> {
    // A primitive receiver boxes to a wrapper with no own symbol-keyed
    // property, but everything ABOVE that wrapper is reachable: its
    // family prototype's dict, the F0 `@@iterator` reify, and the
    // root. A ShortStr is not a cell, so it enters the shared tail
    // below under its Str family rather than through a dict probe.
    let Some((ptr, t)) = recv_cell(recv) else {
        let family = crate::method_value::family::recv_proto_family(recv);
        if family < 0 {
            return None;
        }
        let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(family) }
            as *const c_void;
        let proto_tag = if proto.is_null() {
            0
        } else {
            unsafe { proto.cast::<u8>().add(4).cast::<u16>().read() }
        };
        if !proto.is_null()
            && let Some(pair) = unsafe { probe_dict(own_dict(proto.cast_mut(), proto_tag), key) }
        {
            return Some(pair);
        }
        if crate::nanbox::is_short_str(recv)
            && let Some(cell) =
                unsafe { crate::method_value::builtin_symbol_method_lookup(Tag::Str as u16, key) }
        {
            return Some((4, cell as u64));
        }
        let next = unsafe { chain_next(proto, family, true) }?;
        return unsafe { symbol_key_probe(next, key) };
    };
    if let Some(pair) = unsafe { probe_dict(own_dict(ptr, t), key) } {
        return Some(pair);
    }
    let probed = match unsafe { inherited_dict(ptr, t) } {
        InheritedFrom::Receiver(parent) => return unsafe { symbol_key_probe(parent, key) },
        InheritedFrom::Dict(dict) => {
            if let Some(pair) = unsafe { probe_dict(dict, key) } {
                return Some(pair);
            }
            true
        }
        InheritedFrom::Nothing => false,
    };
    // RFC 20260728-gen-forof-yieldstar F0 — a native iterable tag's
    // `[Symbol.iterator]` reifies its aliased prototype method (a
    // monkey-patch in the dicts above shadows it, per the probes'
    // order). Tag 4 = Heap; the cell is immortal, borrow-shaped.
    //
    // BEFORE the root hop, not after: the reified faces are what the
    // family prototype OWNS, and %Object.prototype% is one link
    // further out.
    // EXCEPT for the one face the family prototype OWNS as a real
    // entry: `[Symbol.iterator]` is installed into Array / String /
    // Map / Set / %TypedArray% prototypes at mint
    // (`__torajs_proto_symbol_keys_install`), so once that dict has
    // been probed it is the authority — `delete
    // Array.prototype[Symbol.iterator]` must read as absent, not be
    // answered again from the reify.
    let family = crate::method_value::family::recv_proto_family(recv);
    let owned_by_family_proto = probed
        && key == crate::method_value::symbol_static::well_known_singleton(5)
        && crate::method_value::symbol_lookup::proto_tag_installs_iterator(family);
    // SAFETY: key is a live Symbol cell per the caller contract.
    if !owned_by_family_proto
        && let Some(cell) = unsafe { crate::method_value::builtin_symbol_method_lookup(t, key) }
    {
        return Some((4, cell as u64));
    }
    // NOT for a DynObj: that arm's `Nothing` is the genuine end of the
    // chain — the root itself, or an `Object.create(null)` receiver
    // whose [[Prototype]] IS null. Hopping there anyway made
    // `Object.create(null)[sym]` answer the root's entry. For every
    // other shape `Nothing` only means the family prototype held
    // nothing, and the chain goes on.
    if t == Tag::DynObj as u16 {
        return None;
    }
    let next = unsafe { chain_next(ptr, family, probed) }?;
    unsafe { symbol_key_probe(next, key) }
}

/// `(tag, value)` for a symbol-keyed read — `(5, 0)` when absent.
///
/// # Safety
/// Cell receivers are live heap pointers; `key` is a live Symbol cell.
pub(crate) unsafe fn symbol_key_pair(recv: AnyValue, key: *const c_void) -> (u64, u64) {
    unsafe { symbol_key_probe(recv, key) }.unwrap_or((TAG_UNDEF, 0))
}

/// §7.3.11 HasProperty for a symbol key — the same walk, asking only
/// whether anything on the chain claims the key. An entry storing
/// `undefined` is present, which is why this cannot be spelled as
/// `symbol_key_pair(..) != (TAG_UNDEF, 0)`.
///
/// # Safety
/// Cell receivers are live heap pointers; `key` is a live Symbol cell.
pub(crate) unsafe fn symbol_key_has(recv: AnyValue, key: *const c_void) -> bool {
    unsafe { symbol_key_probe(recv, key) }.is_some()
}

/// [`symbol_key_pair`] with the accessor sentinel RESOLVED, for the
/// runtime-internal protocol lookups (`@@toStringTag`, `@@hasInstance`,
/// `@@toPrimitive`, …). Answers `(tag, value, owned)`.
///
/// Why this may invoke while the pair may not: the SSA GET path asks
/// for tag and value through two separate externs, so a getter run
/// inside the probe would run TWICE (`member_get.rs`'s blade-5 note) —
/// hence the sentinel, which the emitted accessor arm then resolves in
/// one place. Every caller HERE asks exactly once, so the same sentinel
/// only ever needs resolving, never deferring.
///
/// That asymmetry is what the registered shape of this bug missed: the
/// probe was never the broken part. It answers `ANY_ACCESSOR` correctly
/// and the emitted path consumes it correctly (`o[Symbol.toStringTag]`
/// on an accessor DOES run the getter); it is the runtime-internal
/// consumers that read the sentinel as either a miss or a value.
///
/// `owned` = the value came out of a getter and the caller must drop
/// it. A getter that threw leaves the pending throw in place and
/// answers `(TAG_UNDEF, 0, false)` — callers that care must run
/// `__torajs_throw_check` (nothing is leaked on that path).
///
/// # Safety
/// Cell receivers are live heap pointers; `key` is a live Symbol cell.
#[cfg_attr(test, allow(dead_code))]
pub unsafe fn symbol_key_get(recv: AnyValue, key: *const c_void) -> OwnedPair {
    // §10.5.1 [[Get]] on a proxy runs the `get` trap whatever the key
    // kind is. A symbol read the PROGRAM writes (`p[Symbol.x]`) already
    // trapped — it enters through the member-get cascade, which
    // consults the handler on its way in. A read the RUNTIME makes did
    // not: this is the entry ToPrimitive uses to ask for
    // `@@toPrimitive` and `Object.prototype.toString` to ask for
    // `@@toStringTag`, and both walked straight past the handler into
    // the target's own dict. `String(proxy)` therefore showed the
    // handler one key where bun shows three.
    //
    // The trap's answer is owned, which is what this face returns;
    // `symbol_key_pair` below is the borrowed shape and has no way to
    // carry a freshly minted value, which is why the step belongs here
    // rather than one level down.
    if crate::proxy::is_proxy(recv)
        && let Some((ptr, _)) = recv_cell(recv)
    {
        let got = unsafe { crate::proxy::get(ptr, key, recv) };
        if unsafe { __torajs_throw_check() } != 0 {
            return OwnedPair::borrowed(TAG_UNDEF, 0);
        }
        return OwnedPair {
            tag: unsafe { __torajs_anyv_unbox_tag(got) } as u64,
            payload: unsafe { __torajs_anyv_unbox_value(got) } as u64,
            owned: true,
        };
    }
    let (tag, value) = unsafe { symbol_key_pair(recv, key) };
    if tag != crate::struct_probe::ANY_ACCESSOR_TAG {
        return OwnedPair::borrowed(tag, value);
    }
    let got = unsafe { crate::struct_probe::__torajs_any_accessor_get(recv, key, value) };
    if unsafe { __torajs_throw_check() } != 0 {
        return OwnedPair::borrowed(TAG_UNDEF, 0);
    }
    let t = unsafe { __torajs_anyv_unbox_tag(got) } as u64;
    let v = unsafe { __torajs_anyv_unbox_value(got) } as u64;
    OwnedPair {
        tag: t,
        payload: v,
        owned: true,
    }
}

/// The answer of a [`symbol_key_get`], releasing the getter's +1 when
/// it leaves scope.
///
/// Every runtime-internal consumer wrote the same three lines by hand:
/// hold `(tag, payload, owned)`, thread the pair through a call whose
/// `env` aliases the very cell the pair names, then release on each
/// exit — four exits in `iter_any_get_method`, five in
/// `index_any_method_call`. The rule that made those releases
/// error-prone is a *position* rule ("not before the invoke: `env` IS
/// that cell"), and a comment is the only thing that used to state it.
/// Held in this guard the rule becomes the scope, and the count
/// becomes one drop glue per exit that the compiler writes.
///
/// A borrowed pair (a plain data property, a dict entry, a nullish
/// answer, a getter that threw) carries no +1 and drops to nothing.
pub struct OwnedPair {
    tag: u64,
    payload: u64,
    owned: bool,
}

impl OwnedPair {
    /// A pair the caller does not own — the dict / prototype-walk
    /// answer, and the `(undefined, 0)` a thrown getter leaves behind.
    pub(crate) fn borrowed(tag: u64, payload: u64) -> Self {
        Self {
            tag,
            payload,
            owned: false,
        }
    }

    /// Borrow the pair for the read side. The guard outlives the
    /// borrow, so a `(tag, payload)` handed to an invoke cannot
    /// outlive the reference it names.
    pub(crate) fn pair(&self) -> (u64, u64) {
        (self.tag, self.payload)
    }

    /// The value half alone, for the consumers whose tag check already
    /// pinned it to a cell.
    pub(crate) fn payload(&self) -> u64 {
        self.payload
    }

    /// Hand the +1 out to a longer-lived owner (`Iterator.concat`
    /// parks the resolved `[[OpenMethod]]`): a getter-produced pair
    /// transfers the reference it already holds, a borrowed one takes
    /// its own first. Either way this guard stops releasing.
    #[cfg_attr(test, allow(dead_code))]
    #[inline]
    pub fn into_owned_value(self) -> AnyValue {
        // SAFETY: the pair came out of `symbol_key_get`, so it is
        // either an immediate or a live cell.
        let v = unsafe {
            crate::nanbox_encode::__torajs_anyv_box_from_pair(self.tag as i64, self.payload as i64)
        };
        if !self.owned {
            crate::payload_rc_inc(self.tag as i64, self.payload as i64);
        }
        core::mem::forget(self);
        v
    }
}

impl Drop for OwnedPair {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        // SAFETY: `owned` means the value came out of a getter and the
        // +1 is ours. A non-cell answer re-boxes to an immediate,
        // which `rc_dec` passes over.
        unsafe {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(
                crate::nanbox_encode::__torajs_anyv_box_from_pair(
                    self.tag as i64,
                    self.payload as i64,
                ),
            )
        };
    }
}

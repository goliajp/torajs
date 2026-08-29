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

use crate::member_get::{closure_props, is_wrapper_tag, recv_cell, wrapper_props};
use crate::member_get_own::{user_proto_cell, wrapper_proto_props};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-meta — borrow read of `PROTOS_BY_TAG_IMM[tag]`
    /// (RFC 20260802 刀 3a struct symbol-key inheritance).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
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
const TAG_UNDEF: u64 = 5;

/// True when the property-key cell is a Symbol rather than a Str.
///
/// # Safety
/// `key` must be non-NULL and point at a live key cell.
#[inline]
pub(crate) unsafe fn key_is_symbol(key: *const c_void) -> bool {
    unsafe { key.cast::<u8>().add(4).cast::<u16>().read() == TAG_SYMBOL_KEY }
}

/// The receiver's own property dict — the cell itself for a DynObj,
/// else the in-layout expando slot its shape carries. NULL when the
/// shape has no dict (or has not lazily allocated one yet), in which
/// case it holds no symbol-keyed property.
///
/// Shared by every symbol-key face in this crate (read / has / delete)
/// so "where does this shape keep its properties" is answered in one
/// place.
///
/// # Safety
/// `ptr` is a live cell whose `type_tag` is `t`.
pub(crate) unsafe fn own_dict(ptr: *mut c_void, t: u16) -> *const c_void {
    if t == Tag::DynObj as u16 {
        return ptr;
    }
    if t == Tag::Arr as u16 || t == Tag::Closure as u16 {
        // Both keep the expando dict in the same +24 slot.
        return unsafe { closure_props(ptr) };
    }
    // Rotation 354 — promise bag at +32 (the +24 slot is the
    // callback list).
    if t == Tag::Promise as u16 {
        return unsafe { crate::member_get::promise_props(ptr) };
    }
    // Blade 2 (RFC 20260714-struct-dynamic-props) — a struct cell
    // carries the same +24 expando slot; a symbol-keyed expando
    // (`(c as any)[sym] = v`) lives there like any other key.
    if t == Tag::Obj as u16 {
        return unsafe { crate::member_get_layout::struct_props(ptr) };
    }
    if is_wrapper_tag(t) {
        return unsafe { wrapper_props(ptr) };
    }
    // RFC 20260823 @@species knife — a buffer-family cell's expando
    // bag holds its symbol-keyed own entries like any other key.
    if crate::member_get_buffer::is_buffer_family(t) {
        return unsafe { crate::member_get_layout::buffer_props(ptr, t) };
    }
    core::ptr::null()
}

/// What the receiver INHERITS a symbol key from, as a borrowed cell
/// box: an explicit user [[Prototype]] for a DynObj, else the builtin
/// prototype singleton's own expando dict, which is where a
/// `Object.defineProperty(Array.prototype, sym, …)` monkey-patch
/// lands.
///
/// # Safety
/// `ptr` is a live cell whose `type_tag` is `t`.
unsafe fn inherited_dict(ptr: *mut c_void, t: u16) -> InheritedFrom {
    if t == Tag::DynObj as u16 {
        return match unsafe { user_proto_cell(ptr) } {
            Some(parent) => InheritedFrom::Receiver(parent),
            // No EXPLICIT parent is not the same as no parent: an
            // ordinary object still inherits from %Object.prototype%,
            // and a symbol the program installed there was reachable
            // from nowhere until this hop existed (the string-key twin
            // in `member_get.rs` had the same gap). `None` here is the
            // genuine end — the root itself, or an explicit null proto.
            None => match unsafe { crate::member_get_own::implicit_proto_parent(ptr) } {
                Some(root) => InheritedFrom::Receiver(root),
                None => InheritedFrom::Nothing,
            },
        };
    }
    // RFC 20260802-class-computed-member 刀 3a — a struct instance
    // inherits symbol keys through its class prototype chain: a
    // runtime-computed `[Symbol(...)]()` member lands on
    // `__proto_<C>` as a symbol-keyed own entry, which the recursive
    // DynObj probe above then answers (grandparents via
    // `user_proto_cell`). The raw table read is a borrow (the
    // registry slot is process-lifetime); 0 = unregistered tag.
    if t == Tag::Obj as u16 {
        let class_tag = unsafe { ptr.cast::<u8>().add(8).cast::<u32>().read() };
        let root = unsafe { __torajs_proto_cell_raw(class_tag as i64) };
        if crate::nanbox::is_cell(root) {
            return InheritedFrom::Receiver(root);
        }
        return InheritedFrom::Nothing;
    }
    // A wrapper keeps its own arm: the family map has no
    // SymbolWrapper row (its method reads intern family-less), while
    // `Object(Symbol())` does inherit through %Symbol.prototype% —
    // asking the family map here answered "nothing inherited" and
    // took `Object.defineProperty(Symbol.prototype, @@iterator, …)`
    // out with it.
    if crate::member_get_layout::is_wrapper_tag(t) {
        let proto = wrapper_proto_props(t);
        return if proto.is_null() {
            InheritedFrom::Nothing
        } else {
            InheritedFrom::Dict(proto)
        };
    }
    // Every other builtin cell inherits through the prototype
    // singleton its family owns — where a
    // `Object.defineProperty(Map.prototype, sym, …)` monkey-patch
    // lands, and where the eight spec-given `@@toStringTag` entries
    // already sit (`torajs_meta::proto_tostringtag_install`).
    //
    // This used to name three tags — Arr, Closure, and the wrappers
    // — and answer `Nothing` for the rest, so `Map.prototype[
    // Symbol.toStringTag]` said "Map" while `new Map()[
    // Symbol.toStringTag]` said undefined: the property was
    // installed and simply unreachable from an instance. The family
    // map already knew every row; it was only this walk that did
    // not ask it.
    //
    // The singleton reached this way can BE the receiver
    // (`Array.prototype` is itself an Arr cell), which is why the
    // answer is its dict rather than a recursion — the same reason
    // the three-tag version handed back a dict.
    let family = crate::method_value::family::recv_proto_family(crate::nanbox::box_void_ptr(ptr));
    if family < 0 {
        return InheritedFrom::Nothing;
    }
    let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(family) };
    if proto.is_null() || core::ptr::eq(proto, ptr) {
        return InheritedFrom::Nothing;
    }
    let proto_tag = unsafe { (proto.cast::<u8>().add(4) as *const u16).read() };
    let proto_props = unsafe { own_dict(proto, proto_tag) };
    if proto_props.is_null() {
        InheritedFrom::Nothing
    } else {
        InheritedFrom::Dict(proto_props)
    }
}

/// Where [`symbol_key_pair`] looks next on an own miss.
enum InheritedFrom {
    /// Another receiver — recurse (covers grandparents).
    Receiver(AnyValue),
    /// A prototype's property dict — one more probe, then the
    /// receiver's own reified faces, then [`builtin_root_parent`].
    /// Not a recursion, because the singleton reached this way can BE
    /// the receiver.
    Dict(*const c_void),
    Nothing,
}

/// Probe one dict for `key`. `Some(pair)` when the key is present,
/// including when it is present but stores `undefined`; `None` on a
/// genuine miss so the caller keeps walking.
///
/// # Safety
/// `dict` is NULL or a live DynObj; `key` is a live Symbol cell.
unsafe fn probe_dict(dict: *const c_void, key: *const c_void) -> Option<(u64, u64)> {
    if dict.is_null() {
        return None;
    }
    let tag = unsafe { __torajs_dynobj_get_tag(dict, key) };
    if tag != TAG_UNDEF {
        return Some((tag, unsafe { __torajs_dynobj_get_value(dict, key) }));
    }
    // An own entry STORING undefined is present, and shadows whatever
    // the chain would have answered.
    if unsafe { __torajs_dynobj_has(dict, key) } != 0 {
        return Some((TAG_UNDEF, 0));
    }
    None
}

/// The chain hop a builtin cell owes after its family prototype has
/// answered nothing: §10.1.8.1 step 4 continues through THAT
/// prototype's own [[Prototype]], which is %Object.prototype% for
/// every builtin (`torajs_meta::reflect_proto` states the same rule
/// for `getPrototypeOf`). `None` when the receiver IS the root, so
/// the walk terminates.
///
/// Without this hop the symbol lane was strictly shorter than the
/// string one: `Object.prototype.foo = 5; [1].foo` answered 5 while
/// `Object.defineProperty(Object.prototype, sym, …); [1][sym]`
/// answered undefined — the entry was installed and unreachable from
/// every non-dynobj receiver, the same shape `implicit_proto_parent`
/// closed for dynobjs.
///
/// # Safety
/// `ptr` is NULL or a live heap cell.
unsafe fn builtin_root_parent(ptr: *const c_void) -> Option<AnyValue> {
    let root = unsafe {
        torajs_rc::builtin_proto::__torajs_get_builtin_prototype(
            torajs_rc::builtin_proto::OBJECT_PROTO_TAG as i64,
        )
    };
    if root.is_null() || core::ptr::eq(root.cast_const(), ptr) {
        return None;
    }
    Some(crate::nanbox::box_void_ptr(root))
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
        let root = unsafe { builtin_root_parent(proto) }?;
        return unsafe { symbol_key_probe(root, key) };
    };
    if let Some(pair) = unsafe { probe_dict(own_dict(ptr, t), key) } {
        return Some(pair);
    }
    match unsafe { inherited_dict(ptr, t) } {
        InheritedFrom::Receiver(parent) => return unsafe { symbol_key_probe(parent, key) },
        InheritedFrom::Dict(dict) => {
            if let Some(pair) = unsafe { probe_dict(dict, key) } {
                return Some(pair);
            }
        }
        InheritedFrom::Nothing => {}
    }
    // RFC 20260728-gen-forof-yieldstar F0 — a native iterable tag's
    // `[Symbol.iterator]` reifies its aliased prototype method (a
    // monkey-patch in the dicts above shadows it, per the probes'
    // order). Tag 4 = Heap; the cell is immortal, borrow-shaped.
    //
    // BEFORE the root hop, not after: the reified faces are what the
    // family prototype OWNS, and %Object.prototype% is one link
    // further out.
    // SAFETY: key is a live Symbol cell per the caller contract.
    if let Some(cell) = unsafe { crate::method_value::builtin_symbol_method_lookup(t, key) } {
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
    let root = unsafe { builtin_root_parent(ptr) }?;
    unsafe { symbol_key_probe(root, key) }
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
pub(crate) unsafe fn symbol_key_get(recv: AnyValue, key: *const c_void) -> OwnedPair {
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
pub(crate) struct OwnedPair {
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
    pub(crate) fn into_owned_value(self) -> AnyValue {
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

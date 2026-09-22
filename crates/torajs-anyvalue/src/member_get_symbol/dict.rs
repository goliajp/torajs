//! Where a symbol-keyed property LIVES — the dict a shape keeps its
//! own properties in, and the prototype chain above it.
//!
//! Split out of [`super`] in rotation 693, when that file reached the
//! 500-line limit. The line between them is the question each answers:
//! this one says where to look, the parent says what the answer IS
//! (present / absent / accessor, borrowed or owned).
//!
//! Shared by every symbol-key face in the crate — read, `in`, delete —
//! so "where does this shape keep its properties" has one answer.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::closure_props;
use crate::member_get_own::{user_proto_cell, wrapper_proto_props};
use crate::nanbox::AnyValue;

use super::TAG_UNDEF;

unsafe extern "C" {
    /// torajs-meta — borrow read of `PROTOS_BY_TAG_IMM[tag]`
    /// (RFC 20260802 刀 3a struct symbol-key inheritance).
    fn __torajs_obj_proto_borrowed(cell: *const c_void) -> u64;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// Disambiguates "absent" from "present, storing undefined" — both
    /// answer tag 5 from `get_tag`.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
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
    // An array is the one shape where this lane and the string lane
    // differ: a symbol key never collides with the index domain, so
    // it reads straight from the +24 bag instead of going through
    // the `arrprops_*` kernels. Every other shape — closure, struct,
    // promise, wrapper, buffer, Map / Set / Date / RegExp — reads the
    // one shared table, the same one `member_set_symbol` writes
    // through, so the two never disagree about where a key lives.
    if t == Tag::Arr as u16 {
        return unsafe { closure_props(ptr) };
    }
    unsafe { crate::member_get_layout::expando_props(ptr, t) }
}

/// What the receiver INHERITS a symbol key from, as a borrowed cell
/// box: an explicit user [[Prototype]] for a DynObj, else the builtin
/// prototype singleton's own expando dict, which is where a
/// `Object.defineProperty(Array.prototype, sym, …)` monkey-patch
/// lands.
///
/// # Safety
/// `ptr` is a live cell whose `type_tag` is `t`.
pub(crate) unsafe fn inherited_dict(ptr: *mut c_void, t: u16) -> InheritedFrom {
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
        let root = unsafe { __torajs_obj_proto_borrowed(ptr.cast()) };
        if crate::nanbox::is_cell(root) {
            return InheritedFrom::Receiver(root);
        }
        return InheritedFrom::Nothing;
    }
    // A concrete typed-array constructor inherits from %TypedArray%
    // (§23.2.6), which owns `get [@@species]` for all twelve.
    if t == Tag::Closure as u16 {
        let parent = unsafe {
            crate::method_value::ctor_buffer::__torajs_builtin_ctor_parent_value(ptr.cast_const())
        };
        if parent != 0 {
            return InheritedFrom::Receiver(parent);
        }
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
    if proto.is_null() {
        return InheritedFrom::Nothing;
    }
    // A NULL dict is still a Dict answer, and the singleton BEING the
    // receiver still counts as one: both say "the family prototype has
    // been looked at", which is what tells [`chain_next`] to move on to
    // that prototype's own parent instead of to the prototype itself.
    // Answering `Nothing` for either left `Array.prototype` — an Arr
    // cell, its own family prototype — pointing at itself.
    let proto_tag = unsafe { (proto.cast::<u8>().add(4) as *const u16).read() };
    InheritedFrom::Dict(unsafe { own_dict(proto, proto_tag) })
}

/// Where [`symbol_key_pair`] looks next on an own miss.
pub(crate) enum InheritedFrom {
    /// Another receiver — recurse (covers grandparents).
    Receiver(AnyValue),
    /// A prototype's property dict — one more probe, then the
    /// receiver's own reified faces, then [`chain_next`].
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
pub(crate) unsafe fn probe_dict(dict: *const c_void, key: *const c_void) -> Option<(u64, u64)> {
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

/// The next link after everything the walk has already probed.
///
/// `probed` = the family prototype's own dict has been looked at, so
/// the chain continues through THAT prototype's parent
/// (`proto_parent_tag` — the one home for which parent each builtin
/// prototype has). When it has not been looked at — a struct cell with
/// no registered class, a tag the family map does not name — the
/// family prototype itself is the next link, and the ordinary root
/// stands in when there is no family at all.
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
pub(crate) unsafe fn chain_next(ptr: *const c_void, family: i64, probed: bool) -> Option<AnyValue> {
    let root = torajs_rc::builtin_proto::OBJECT_PROTO_TAG as i64;
    let tag = if family < 0 {
        root
    } else if probed {
        torajs_rc::builtin_proto::proto_parent_tag(family)
    } else {
        family
    };
    if tag < 0 {
        return None;
    }
    let cell = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(tag) };
    if cell.is_null() || core::ptr::eq(cell.cast_const(), ptr) {
        return None;
    }
    Some(crate::nanbox::box_void_ptr(cell))
}

//! Well-known-symbol method reify off a native receiver tag — the
//! `[Symbol.iterator]` F0 table plus the RegExp §22.2.6 protocol row
//! (r289), split out of the parent when the RegExp arm pushed it past
//! the 500-line cap.
//!
//! The `[Symbol.iterator]` fact is asked from two directions and both
//! live here so they cannot drift apart: [`builtin_symbol_method_lookup`]
//! answers it off an INSTANCE's heap tag (`m[Symbol.iterator]`), and
//! [`__torajs_proto_iterator_install`] writes it as a real own entry on
//! the PROTOTYPE singleton (`Map.prototype[Symbol.iterator]`). Both
//! hand out the same interned cell, which is what §24.1.3.14's "the
//! initial value is the entries function" requires: the two reads and
//! `Map.prototype.entries` are one function object, not three.

use core::ffi::c_void;

use torajs_rc::Tag;

use super::{builtin_method_cell, symbol_static};

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
        for (idx, mid) in [
            (6, torajs_rc::ANY_METHOD_MATCH),
            (7, torajs_rc::ANY_METHOD_MATCH_ALL),
            (8, torajs_rc::ANY_METHOD_REPLACE),
            (9, torajs_rc::ANY_METHOD_SEARCH),
            (11, torajs_rc::ANY_METHOD_SPLIT),
        ] {
            if key == symbol_static::well_known_singleton(idx) {
                // Family 7 = the RegExp proto row (`family.rs`).
                return Some(builtin_method_cell(7, mid));
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
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
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
/// The aliases mirror the instance table above — §24.1.3.14 Map →
/// `entries`, §24.2.3.13 Set → `values`; §22.1.3.36 String has no
/// named alias and carries a dedicated id. Tags are
/// `torajs-rc/builtin_proto.rs` order.
///
/// `Array.prototype` (§23.1.3.40, aliased to `values`) is absent on
/// purpose: tag 2 is an Arr cell whose side props have no
/// attribute-carrying define kernel, only a plain `set`. Its entry
/// waits on one rather than landing here as a `{W:1,E:1,C:1}`
/// approximation.
fn proto_tag_iterator_alias(proto_tag: i64) -> Option<(i64, i64)> {
    Some(match proto_tag {
        3 => (3, torajs_rc::ANY_METHOD_STR_ITERATOR),
        11 => (11, torajs_rc::ANY_METHOD_ENTRIES),
        12 => (12, torajs_rc::ANY_METHOD_VALUES),
        _ => return None,
    })
}

/// Define `[Symbol.iterator]` on a freshly minted builtin prototype
/// as a REAL own entry, if its spec clause gives it one — a no-op
/// for the rest (see the alias table for why Array is among them).
///
/// Before this the fact only existed on the instance side: reading
/// `Map.prototype[Symbol.iterator]` answered undefined while
/// `new Map()[Symbol.iterator]` answered the entries cell, and
/// `getOwnPropertySymbols(Map.prototype)` never listed it — the same
/// "behaviour right, property absent" shape rotation 382 closed for
/// `@@toStringTag`.
///
/// Called from the builtin-proto mint before the CAS install, so a
/// race loser leaks a fully-formed dynobj (the posture every sibling
/// install shares).
///
/// # Safety
/// FFI face; `proto` is NULL or the freshly allocated dynobj.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proto_iterator_install(proto: *mut c_void, idx: i64) {
    if proto.is_null() {
        return;
    }
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
    let mut slot = proto;
    unsafe {
        __torajs_dynobj_define(
            &mut slot,
            key as *mut c_void,
            ANY_HEAP,
            cell as u64,
            METHOD_ENTRY_FLAGS,
        )
    };
}

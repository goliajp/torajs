//! Well-known-symbol method reify off a native receiver tag — the
//! `[Symbol.iterator]` F0 table plus the RegExp §22.2.6 protocol row
//! (r289), split out of the parent when the RegExp arm pushed it past
//! the 500-line cap.

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

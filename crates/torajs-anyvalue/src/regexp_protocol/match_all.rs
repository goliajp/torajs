//! §22.2.6.9 `RegExp.prototype[@@matchAll]` and the §22.2.9 RegExp
//! String Iterator it answers.
//!
//! The iterator is a helper-shaped cell of its own kind: the matcher
//! sits in the underlying slot, the string in the callback slot and
//! the `global` / `fullUnicode` bits in the counter, the alive byte is
//! [[Done]], and the helper cell's executing gate is the generator
//! state §22.2.9.1's CreateIteratorFromClosure gives it. Every `next()`
//! is one §22.2.7.1 RegExpExec — nothing is matched ahead of the call
//! that asks for it.

use core::ffi::c_void;

use super::ops::{
    ABRUPT, Owned, OwnedStr, advance_string_index, box_number, flags_of, get_index, has_flag,
    last_index_length, regexp_exec, set_last_index, str_from_units, to_string,
};
use super::species::{construct, species_constructor};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_null};

/// The RegExp String Iterator's helper kind (`iter_helper` kinds run
/// 0-8); torajs-rc's `iter_proto` mirrors it to name the prototype.
pub(crate) const REGEXP_STRING_ITER_KIND: u8 =
    torajs_rc::builtin_proto::iter_proto::ITER_HELPER_KIND_REGEXP_STRING;

const GLOBAL_BIT: u64 = 1;
const FULL_UNICODE_BIT: u64 = 2;

/// §22.2.6.9 steps 3-13.
pub(super) unsafe fn symbol_match_all(r: AnyValue, string: AnyValue) -> AnyValue {
    unsafe {
        let Some(s) = to_string(string) else {
            return ABRUPT;
        };
        let Some(ctor) = species_constructor(r) else {
            return ABRUPT;
        };
        let Some(flags) = flags_of(r) else {
            return ABRUPT;
        };
        let flags_str = str_from_units(&flags);
        let Some(matcher) = construct(ctor.as_ref(), r, &flags_str) else {
            return ABRUPT;
        };
        let Some(last_index) = last_index_length(r) else {
            return ABRUPT;
        };
        if set_last_index(matcher.get(), box_number(last_index as f64)).is_none() {
            return ABRUPT;
        }
        let mut bits = 0;
        if has_flag(&flags, b'g') {
            bits |= GLOBAL_BIT;
        }
        if has_flag(&flags, b'u') || has_flag(&flags, b'v') {
            bits |= FULL_UNICODE_BIT;
        }
        let cell = crate::iter_helper::iter_helper_cell_alloc(REGEXP_STRING_ITER_KIND);
        let at = |off: usize| cell.add(off) as *mut u64;
        *at(crate::iter_helper::UNDERLYING_OFF) = matcher.take();
        *at(crate::iter_helper::FN_OFF) = s.take_boxed();
        *at(crate::iter_helper::COUNTER_OFF) = bits;
        crate::nanbox_encode::__torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// Whether `v` is a RegExp String Iterator cell.
pub(crate) fn is_regexp_string_iter(v: AnyValue) -> bool {
    if !is_cell(v) {
        return false;
    }
    let p = as_void_ptr(v).cast::<u8>();
    // SAFETY: a cell box addresses a live heap header; the kind byte
    // is read only off a helper-tagged cell.
    unsafe {
        (p.add(4) as *const u16).read() == torajs_rc::Tag::IterHelper as u16
            && p.add(crate::iter_helper::KIND_OFF).read() == REGEXP_STRING_ITER_KIND
    }
}

/// One §22.2.9.1 closure step: the owned match, or
/// [`super::ITER_STEP_DONE`] when the iterator is done — or completed
/// abruptly, with the throw pending (a generator that throws is done
/// either way).
///
/// # Safety
/// `recv` boxes a live RegExp String Iterator cell, under the helper's
/// executing gate.
pub(super) unsafe fn regexp_string_step(recv: AnyValue) -> AnyValue {
    unsafe {
        let ptr = as_void_ptr(recv).cast::<u8>();
        let slot = |off: usize| (ptr.add(off) as *const u64).read();
        let r = slot(crate::iter_helper::UNDERLYING_OFF);
        let s_box = slot(crate::iter_helper::FN_OFF);
        let bits = slot(crate::iter_helper::COUNTER_OFF);
        crate::nanbox_ffi::__torajs_anyv_rc_inc(s_box);
        let s = OwnedStr::adopt(as_void_ptr(s_box));
        match step(r, &s, bits) {
            Some((m, last)) => {
                if last {
                    ptr.add(crate::iter_helper::ALIVE_OFF).write(0);
                }
                m.take()
            }
            None => {
                ptr.add(crate::iter_helper::ALIVE_OFF).write(0);
                super::ITER_STEP_DONE
            }
        }
    }
}

/// The match and whether it is the last one the closure yields;
/// `None` for done or abrupt.
unsafe fn step(r: AnyValue, s: &OwnedStr, bits: u64) -> Option<(Owned, bool)> {
    unsafe {
        let m = regexp_exec(r, s)?;
        if is_null(m.get()) {
            return None;
        }
        if bits & GLOBAL_BIT == 0 {
            return Some((m, true));
        }
        let m0 = get_index(m.get(), 0)?;
        if to_string(m0.get())?.is_empty() {
            let this_index = last_index_length(r)?;
            let next = advance_string_index(&s.units(), this_index, bits & FULL_UNICODE_BIT != 0);
            set_last_index(r, box_number(next as f64))?;
        }
        Some((m, false))
    }
}

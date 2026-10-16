//! §22.2.6.14 `RegExp.prototype[@@split]`: a sticky splitter minted
//! through the receiver's species constructor, driven one position at
//! a time through RegExpExec.

use crate::ops::{
    ABRUPT, Owned, OwnedStr, advance_string_index, box_number, flags_of, get, get_index, has_flag,
    last_index_length, regexp_exec, set_last_index, str_from_units, to_length,
};
use crate::species::{construct, species_constructor};
use torajs_anyvalue::nanbox::{AnyValue, is_null, is_undefined};
use torajs_anyvalue::regexp_support as av;

unsafe extern "C" {
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any_boxed(arr: *mut u8, av: u64) -> *mut u8;
}

/// The array under construction (one owned stake).
struct Out(Owned);

impl Out {
    unsafe fn new() -> Self {
        Out(Owned::adopt(unsafe {
            av::__torajs_anyv_box_from_pair(4, __torajs_arr_alloc_any(0) as i64)
        }))
    }

    /// CreateDataProperty(A, lengthA, v) — the slot takes its own
    /// stake; `v`'s is released.
    unsafe fn push(&mut self, v: Owned) {
        unsafe {
            let raw = torajs_anyvalue::nanbox::as_void_ptr(self.0.get()).cast::<u8>();
            let grown = __torajs_arr_push_any_boxed(raw, v.get());
            if grown != raw {
                let old = core::mem::replace(
                    &mut self.0,
                    Owned::adopt(av::__torajs_anyv_box_from_pair(4, grown as i64)),
                );
                core::mem::forget(old);
            }
        }
    }

    unsafe fn push_units(&mut self, units: &[u16]) {
        unsafe { self.push(Owned::adopt(str_from_units(units).take_boxed())) }
    }
}

/// §22.2.6.14 steps 3-20.
pub(crate) unsafe fn symbol_split(rx: AnyValue, string: AnyValue, limit: AnyValue) -> AnyValue {
    unsafe {
        let Some(s) = crate::ops::to_string(string) else {
            return ABRUPT;
        };
        let Some(ctor) = species_constructor(rx) else {
            return ABRUPT;
        };
        let Some(flags) = flags_of(rx) else {
            return ABRUPT;
        };
        let unicode = has_flag(&flags, b'u') || has_flag(&flags, b'v');
        let mut new_flags = flags.clone();
        if !has_flag(&flags, b'y') {
            new_flags.push(b'y' as u16);
        }
        let new_flags = str_from_units(&new_flags);
        let Some(splitter) = construct(ctor.as_ref(), rx, &new_flags) else {
            return ABRUPT;
        };
        let lim = if is_undefined(limit) {
            u32::MAX
        } else {
            let n = av::__torajs_anyv_to_number(limit);
            if crate::ops::throw_pending() {
                return ABRUPT;
            }
            to_uint32(n)
        };
        let mut a = Out::new();
        if lim == 0 {
            return a.0.take();
        }
        split_with(splitter.get(), &s, unicode, lim, &mut a).map_or(ABRUPT, |()| a.0.take())
    }
}

/// Steps 13-20 over the constructed splitter.
unsafe fn split_with(
    splitter: AnyValue,
    s: &OwnedStr,
    unicode: bool,
    lim: u32,
    a: &mut Out,
) -> Option<()> {
    unsafe {
        let units = s.units();
        let size = units.len() as i64;
        if size == 0 {
            let z = regexp_exec(splitter, s)?;
            if is_null(z.get()) {
                a.push(Owned::adopt(str_from_units(&[]).take_boxed()));
            }
            return Some(());
        }
        let mut length_a: u32 = 0;
        let mut p: i64 = 0;
        let mut q: i64 = p;
        while q < size {
            set_last_index(splitter, box_number(q as f64))?;
            let z = regexp_exec(splitter, s)?;
            if is_null(z.get()) {
                q = advance_string_index(&units, q, unicode);
                continue;
            }
            let e = last_index_length(splitter)?.min(size);
            if e == p {
                q = advance_string_index(&units, q, unicode);
                continue;
            }
            a.push_units(&units[p as usize..q as usize]);
            length_a += 1;
            if length_a == lim {
                return Some(());
            }
            p = e;
            let len_z = get(z.get(), b"length")?;
            let n_captures = (to_length(len_z.get())? - 1).max(0);
            let mut i = 1;
            while i <= n_captures {
                a.push(get_index(z.get(), i)?);
                length_a += 1;
                if length_a == lim {
                    return Some(());
                }
                i += 1;
            }
            q = p;
        }
        a.push_units(&units[p as usize..]);
        Some(())
    }
}

/// §7.1.7 ToUint32 over the ToNumber answer.
fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4_294_967_296.0) as u32
}

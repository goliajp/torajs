//! The abstract operations the §22.2.6 protocol algorithms are written
//! in — Get, Set(…, true), ToString, ToLength, §22.2.7.1 RegExpExec,
//! §22.2.7.3 AdvanceStringIndex — each answering `None` when it left a
//! throw pending, which is how every `?` in the spec text propagates.
//!
//! Ownership is carried by [`Owned`] (one stake on a boxed value) and
//! [`OwnedStr`] (one stake on a Str cell), so an early return on any
//! abrupt step releases what the algorithm had built so far.

use core::ffi::c_void;

use torajs_anyvalue::nanbox::{
    AnyValue, VALUE_NULL, VALUE_UNDEFINED, as_void_ptr, box_double, box_int32, is_cell, is_null,
};
use torajs_anyvalue::regexp_support as av;
use torajs_anyvalue::regexp_support::__torajs_anyv_box_from_pair;
use torajs_anyvalue::regexp_support::member_pair_cell;
use torajs_anyvalue::regexp_support::{
    __torajs_anyv_rc_dec, __torajs_anyv_to_number, __torajs_anyv_to_str,
};

unsafe extern "C" {
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-str — an owned plain Str (+1) off a Str or Substr view.
    fn __torajs_str_any_to_owned(s: *const u8) -> *mut u8;
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-regex — §22.2.7.2 RegExpBuiltinExec: NULL on a miss, a
    /// fresh +1 match array (index / input / groups attached) on a hit.
    fn __torajs_regex_exec(re: *const c_void, s: *const c_void, want_exec: i64) -> *mut c_void;
    /// torajs-arr — stamp the heap element kind on `exec`'s product.
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    fn __torajs_anyv_unbox_tag(v: AnyValue) -> i64;
    fn __torajs_anyv_unbox_value(v: AnyValue) -> i64;
    /// torajs-str — an owned Str of the decimal digits of `n`.
    fn __torajs_i64_to_str(n: i64) -> *mut c_void;
}

/// `ARR_KIND_HEAP` mirror — the one kind `exec` match slots hold.
const KIND_HEAP_CHAIN: u64 = 4;

/// Str layout mirrors (torajs-str `layout`).
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;
const STR_FLAG_IS_LATIN1: u16 = 0x0002;

/// One owned stake on a boxed value, released on drop.
pub(crate) struct Owned(AnyValue);

impl Owned {
    /// Adopt a +1 the caller already holds.
    pub(crate) fn adopt(v: AnyValue) -> Self {
        Self(v)
    }

    pub(crate) fn get(&self) -> AnyValue {
        self.0
    }

    /// Hand the stake on to a longer-lived owner.
    pub(crate) fn take(self) -> AnyValue {
        let v = self.0;
        core::mem::forget(self);
        v
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the stake is ours; immediates pass through rc_dec.
        unsafe { __torajs_anyv_rc_dec(self.0) }
    }
}

/// One owned stake on a Str cell, released on drop.
pub(crate) struct OwnedStr(*mut c_void);

impl OwnedStr {
    /// Adopt a +1 the caller already holds on a Str cell.
    pub(crate) fn adopt(p: *mut c_void) -> Self {
        Self(p)
    }

    pub(crate) fn ptr(&self) -> *mut c_void {
        self.0
    }

    /// The string as a borrowed box (the guard keeps it alive).
    pub(crate) fn boxed(&self) -> AnyValue {
        // SAFETY: a live Str cell encodes as a heap box.
        unsafe { __torajs_anyv_box_from_pair(4, self.0 as i64) }
    }

    /// Hand the stake on as an owned box.
    pub(crate) fn take_boxed(self) -> AnyValue {
        let v = self.boxed();
        core::mem::forget(self);
        v
    }

    /// Hand the stake on as the raw cell.
    pub(crate) fn into_raw(self) -> *mut c_void {
        let p = self.0;
        core::mem::forget(self);
        p
    }

    /// Whether the string has no code units.
    pub(crate) fn is_empty(&self) -> bool {
        // SAFETY: the guard holds a live Str cell.
        unsafe { (self.0.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() == 0 }
    }

    /// The UTF-16 code units of the string.
    pub(crate) fn units(&self) -> Vec<u16> {
        // SAFETY: the guard holds a live Str cell.
        unsafe { str_units(self.0) }
    }
}

impl Drop for OwnedStr {
    fn drop(&mut self) {
        // SAFETY: the stake is ours.
        unsafe { __torajs_str_drop(self.0) }
    }
}

pub(crate) fn throw_pending() -> bool {
    // SAFETY: a pure probe of the pending-throw record.
    unsafe { __torajs_throw_check() != 0 }
}

pub(crate) unsafe fn type_error(msg: &core::ffi::CStr) {
    unsafe { __torajs_throw_type_error(msg.as_ptr()) }
}

/// A fresh Str cell over ASCII / UTF-8 bytes — the property keys.
pub(crate) unsafe fn key_str(name: &[u8]) -> OwnedStr {
    OwnedStr(unsafe { __torajs_str_alloc(name.as_ptr(), name.len() as i64) } as *mut c_void)
}

/// ? Get(O, P) for a key cell. `None` = a getter threw.
pub(crate) unsafe fn get_key(obj: AnyValue, key: *const c_void) -> Option<Owned> {
    unsafe {
        let (t, v) = member_pair_cell(obj, key);
        let out = if t == av::ANY_ACCESSOR_TAG {
            av::__torajs_any_accessor_get(obj, key, v)
        } else {
            av::payload_rc_inc(t as i64, v as i64);
            __torajs_anyv_box_from_pair(t as i64, v as i64)
        };
        let out = Owned(out);
        if throw_pending() {
            return None;
        }
        Some(out)
    }
}

/// ? Get(O, P) for an ASCII property name.
pub(crate) unsafe fn get(obj: AnyValue, name: &[u8]) -> Option<Owned> {
    unsafe {
        let key = key_str(name);
        get_key(obj, key.ptr())
    }
}

/// ? Get(O, ! ToString(𝔽(index))).
pub(crate) unsafe fn get_index(obj: AnyValue, index: i64) -> Option<Owned> {
    unsafe {
        let key = OwnedStr(__torajs_i64_to_str(index));
        get_key(obj, key.ptr())
    }
}

/// ? Set(O, "lastIndex", v, true) — `v` is borrowed.
pub(crate) unsafe fn set_last_index(obj: AnyValue, v: AnyValue) -> Option<()> {
    unsafe {
        let key = key_str(b"lastIndex");
        let tag = __torajs_anyv_unbox_tag(v);
        let payload = __torajs_anyv_unbox_value(v);
        // the store takes the pair over
        av::payload_rc_inc(tag, payload);
        let mut slot = obj;
        av::__torajs_any_member_set(
            &mut slot,
            key.ptr(),
            tag as u64,
            payload as u64,
            torajs_rc::ANY_RPROP_LAST_INDEX,
        );
        (!throw_pending()).then_some(())
    }
}

/// The number `n` as a box (an integral value in int32 range packs as
/// int32, like a literal would).
pub(crate) fn box_number(n: f64) -> AnyValue {
    let int32 = n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64;
    if int32 && !(n == 0.0 && n.is_sign_negative()) {
        box_int32(n as i32)
    } else {
        box_double(n)
    }
}

/// ? ToString(v) as an owned Str cell.
pub(crate) unsafe fn to_string(v: AnyValue) -> Option<OwnedStr> {
    unsafe {
        let s = __torajs_anyv_to_str(v);
        if throw_pending() {
            if !s.is_null() {
                __torajs_str_drop(s);
            }
            return None;
        }
        // a Substr view flattens, so every reader here sees a Str layout
        let flat = __torajs_str_any_to_owned(s.cast());
        __torajs_str_drop(s);
        Some(OwnedStr(flat.cast()))
    }
}

/// ? ToIntegerOrInfinity(v).
pub(crate) unsafe fn to_integer_or_infinity(v: AnyValue) -> Option<f64> {
    let n = unsafe { __torajs_anyv_to_number(v) };
    if throw_pending() {
        return None;
    }
    Some(if n.is_nan() { 0.0 } else { n.trunc() + 0.0 })
}

/// ? ToLength(v).
pub(crate) unsafe fn to_length(v: AnyValue) -> Option<i64> {
    let n = unsafe { to_integer_or_infinity(v) }?;
    Some(if n <= 0.0 {
        0
    } else {
        n.min(9_007_199_254_740_991.0) as i64
    })
}

/// ℝ(? ToLength(? Get(R, "lastIndex"))).
pub(crate) unsafe fn last_index_length(rx: AnyValue) -> Option<i64> {
    unsafe {
        let li = get(rx, b"lastIndex")?;
        to_length(li.get())
    }
}

/// ? ToString(? Get(R, "flags")) as code units.
pub(crate) unsafe fn flags_of(rx: AnyValue) -> Option<Vec<u16>> {
    unsafe {
        let key = key_str(b"flags");
        // A RegExp cell's direct `flags` read renders its own flag
        // word; the §22.2.6.4 getter it stands for reads the eight
        // flag properties, which an own `global` accessor overrides.
        let f = if av::regexp_cell(rx).is_some() && !av::shadowed(rx, key.ptr()) {
            // through the accessor face, the getter's one caller (it
            // inlines there)
            let v = Owned(
                av::regexp_accessor_call(rx, torajs_rc::ANY_METHOD_GET_FLAGS)
                    .unwrap_or(torajs_anyvalue::nanbox::VALUE_UNDEFINED),
            );
            if throw_pending() {
                return None;
            }
            v
        } else {
            get_key(rx, key.ptr())?
        };
        let s = to_string(f.get())?;
        Some(s.units())
    }
}

/// §22.2.7.1 RegExpExec(R, S): a callable `exec` decides the answer
/// (an Object or null, else TypeError); a builtin one — or none at
/// all — is §22.2.7.2 over the receiver's matcher, which a receiver
/// without one refuses. Answers the owned result, JS null on a miss.
pub(crate) unsafe fn regexp_exec(rx: AnyValue, s: &OwnedStr) -> Option<Owned> {
    unsafe {
        let exec = get(rx, b"exec")?;
        let e = exec.get();
        if av::is_callable(e) && !is_builtin_exec(e) {
            let arg = s.boxed();
            let r = Owned(av::__torajs_any_call_with_this(e, rx, &arg, 1));
            if throw_pending() {
                return None;
            }
            if !is_null(r.get()) && !av::av_is_object(r.get()) {
                type_error(c"RegExp exec method returned something other than an Object or null");
                return None;
            }
            return Some(r);
        }
        builtin_exec(rx, s)
    }
}

/// Whether `v` is the reified %RegExp.prototype.exec%.
unsafe fn is_builtin_exec(v: AnyValue) -> bool {
    if !is_cell(v) {
        return false;
    }
    let cell = as_void_ptr(v);
    unsafe {
        av::builtin_method_mid(cell) == Some(torajs_rc::ANY_METHOD_EXEC)
            && av::builtin_method_family(cell) == av::REGEXP_PROTO_FAMILY
    }
}

/// §22.2.7.2 RegExpBuiltinExec behind RequireInternalSlot.
unsafe fn builtin_exec(rx: AnyValue, s: &OwnedStr) -> Option<Owned> {
    unsafe {
        let Some(re) = av::regexp_cell(rx) else {
            type_error(c"RegExp.prototype.exec called on an incompatible receiver");
            return None;
        };
        let arr = __torajs_regex_exec(re, s.ptr(), 1);
        if arr.is_null() {
            return (!throw_pending()).then_some(Owned(VALUE_NULL));
        }
        __torajs_arr_mark_kind(arr, KIND_HEAP_CHAIN);
        let out = Owned(__torajs_anyv_box_from_pair(4, arr as i64));
        if throw_pending() {
            return None;
        }
        Some(out)
    }
}

/// §22.2.7.3 AdvanceStringIndex.
pub(crate) fn advance_string_index(s: &[u16], index: i64, unicode: bool) -> i64 {
    let len = s.len() as i64;
    if !unicode || index + 1 >= len {
        return index + 1;
    }
    let i = index as usize;
    let lead = s[i];
    if (0xD800..0xDC00).contains(&lead) && (0xDC00..0xE000).contains(&s[i + 1]) {
        index + 2
    } else {
        index + 1
    }
}

/// Whether the flag string carries `c`.
pub(crate) fn has_flag(flags: &[u16], c: u8) -> bool {
    flags.contains(&(c as u16))
}

/// A Str cell's UTF-16 code units.
///
/// # Safety
/// `s` is a live Str cell.
pub(crate) unsafe fn str_units(s: *const c_void) -> Vec<u16> {
    unsafe {
        let p = s.cast::<u8>();
        let len = (p.add(STR_LEN_OFF) as *const u32).read() as usize;
        let flags = (p.add(6) as *const u16).read();
        let data = p.add(STR_DATA_OFF);
        if flags & STR_FLAG_IS_LATIN1 != 0 {
            core::slice::from_raw_parts(data, len)
                .iter()
                .map(|&b| b as u16)
                .collect()
        } else {
            (0..len)
                .map(|i| u16::from_le_bytes([*data.add(2 * i), *data.add(2 * i + 1)]))
                .collect()
        }
    }
}

/// A fresh Str cell over UTF-16 code units (each unit spelled as its
/// own one-to-three-byte form, so a lone surrogate survives).
pub(crate) unsafe fn str_from_units(units: &[u16]) -> OwnedStr {
    let mut bytes = Vec::with_capacity(units.len());
    for &u in units {
        let c = u as u32;
        if c < 0x80 {
            bytes.push(c as u8);
        } else if c < 0x800 {
            bytes.push((0xC0 | (c >> 6)) as u8);
            bytes.push((0x80 | (c & 0x3F)) as u8);
        } else {
            bytes.push((0xE0 | (c >> 12)) as u8);
            bytes.push((0x80 | ((c >> 6) & 0x3F)) as u8);
            bytes.push((0x80 | (c & 0x3F)) as u8);
        }
    }
    unsafe { key_str(&bytes) }
}

/// `undefined`, for the abrupt exits.
pub(crate) const ABRUPT: AnyValue = VALUE_UNDEFINED;

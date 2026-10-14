//! The cast plan the object cast walks (`super::obj`): the slot's
//! static type, compiled by the lowering into rodata bytes
//! (`ssa_lower_checked_cast::plan`, which writes the same layout).
//!
//! ```text
//! plan  := u32 lists_off, node (the root, at offset 4), lists
//! node  := u8 kind, then by kind:
//!   ANY  INT  NUM  BOOL  STR            nothing
//!   BOXED   u8 member (0 number, 1 boolean): a `T | null` box
//!   OBJ     u8 flags (1 = a class slot), u32 class_tag,
//!           u32 class list index (u32::MAX: none), u32 vtable index,
//!           u16 field count, fields
//!   REF     u32 offset of an OBJ node (a type that contains itself)
//!   ARR     u8 element kind (`__torajs_arr_any_to_typed`), node
//!   CELL    u16 heap tag
//!   OPT     node: the node's type, `| null | undefined`; over NUM
//!           (an optional `number` member's f64 slot) `undefined` is
//!           the slot's sentinel and a field's `null` goes to the
//!           cell's sidecar (`super::obj`)
//! field := u32 end (offset past this field), u32 slot offset,
//!          u16 name length, name (WTF-8), node
//! lists := u32 count, then per list: u32 n, u32 tags[n]
//! ```
//!
//! Reads are unaligned: the bytes are a string literal block.

use core::cell::RefCell;
use core::ffi::c_void;
use std::collections::BTreeMap;

use torajs_rc::undef_cell::undef_cell_ptr;
use torajs_rc::{ARR_KIND_HEAP, Tag};

use super::cell_tag;
use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined,
};

pub(super) const ANY: u8 = 0;
pub(super) const INT: u8 = 1;
pub(super) const NUM: u8 = 2;
pub(super) const BOOL: u8 = 3;
pub(super) const STR: u8 = 4;
pub(super) const BOXED: u8 = 5;
pub(super) const OBJ: u8 = 6;
pub(super) const REF: u8 = 7;
pub(super) const ARR: u8 = 8;
pub(super) const CELL: u8 = 9;
pub(super) const OPT: u8 = 10;

/// Below 2^53 every integer-valued double converts to i64 exactly.
const I64_EXACT: f64 = 9_007_199_254_740_992.0;
const ARR_LEN_OFF: usize = 8;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_undef() -> *mut u8;
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
}

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_arr_any_to_typed(arr: *mut u8, kind: i64) -> *mut u8;
    fn __torajs_arr_alloc(cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
}

// Cargo-test stand-ins: torajs-arr is not linked into this crate's
// tests, and no test casts an array.
#[cfg(test)]
unsafe fn __torajs_arr_any_to_typed(_arr: *mut u8, _kind: i64) -> *mut u8 {
    unreachable!()
}
#[cfg(test)]
unsafe fn __torajs_arr_alloc(_cap: u64) -> *mut u8 {
    unreachable!()
}
#[cfg(test)]
unsafe fn __torajs_arr_push(_arr: *mut u8, _val: i64) -> *mut u8 {
    unreachable!()
}
#[cfg(test)]
unsafe fn __torajs_arr_mark_kind(_arr: *mut c_void, _chain: u64) {
    unreachable!()
}

/// Why a value did not cross: it is not of the slot's type (the
/// caller records the TypeError), or user code run on the way (a
/// getter) or a nested kernel already recorded a throw.
pub(super) enum Fail {
    Mismatch,
    Thrown,
}

/// A plan, the vtable addresses of its object nodes, and the copies
/// this cast has made so far: an object or array reached again (a
/// cycle, or one value under two names) answers the copy already made
/// for the same node, so a cycle stays a cycle and the walk ends.
pub(super) struct Plan {
    pub(super) base: *const u8,
    pub(super) vtables: *const *mut c_void,
    pub(super) copies: RefCell<BTreeMap<(AnyValue, usize), u64>>,
}

impl Plan {
    pub(super) fn new(base: *const u8, vtables: *const *mut c_void) -> Plan {
        Plan {
            base,
            vtables,
            copies: RefCell::new(BTreeMap::new()),
        }
    }

    /// The copy already made of `v` for the node at `at`, with a stake
    /// for the caller.
    pub(super) fn copy_of(&self, v: AnyValue, at: usize) -> Option<u64> {
        let copy = *self.copies.borrow().get(&(v, at))?;
        unsafe { __torajs_rc_inc(copy as *mut c_void) };
        Some(copy)
    }

    /// Record `copy` as the copy of `v` for the node at `at`, before
    /// its contents are filled in.
    pub(super) fn note_copy(&self, v: AnyValue, at: usize, copy: u64) {
        self.copies.borrow_mut().insert((v, at), copy);
    }

    pub(super) fn u8(&self, at: usize) -> u8 {
        unsafe { self.base.add(at).read() }
    }

    pub(super) fn u16(&self, at: usize) -> u16 {
        unsafe { self.base.add(at).cast::<u16>().read_unaligned() }
    }

    pub(super) fn u32(&self, at: usize) -> u32 {
        unsafe { self.base.add(at).cast::<u32>().read_unaligned() }
    }

    /// The class list `idx` of the lists table, in the layout
    /// `__torajs_anyv_struct_fits` reads (a u32 count, then the tags).
    pub(super) fn class_list(&self, idx: u32) -> *const u8 {
        let mut at = self.u32(0) as usize + 4;
        for _ in 0..idx {
            at += 4 + 4 * self.u32(at) as usize;
        }
        unsafe { self.base.add(at) }
    }

    /// The slot bits of `v` cast by the node at `at`, holding a stake
    /// of their own; `v` is borrowed.
    ///
    /// # Safety
    /// `at` is a node of this plan; a cell payload of `v` is live.
    pub(super) unsafe fn cast(&self, at: usize, v: AnyValue) -> Result<u64, Fail> {
        let tag = is_cell(v).then(|| unsafe { cell_tag(v) });
        match self.u8(at) {
            ANY => {
                if is_cell(v) {
                    unsafe { __torajs_rc_inc(as_void_ptr(v)) };
                }
                Ok(v)
            }
            INT if is_int32(v) => Ok(as_int32(v) as i64 as u64),
            INT if is_double(v)
                && as_double(v).fract() == 0.0
                && as_double(v).abs() < I64_EXACT =>
            {
                Ok(as_double(v) as i64 as u64)
            }
            NUM if is_int32(v) => Ok((as_int32(v) as f64).to_bits()),
            NUM if is_double(v) => Ok(as_double(v).to_bits()),
            BOOL if is_bool(v) => Ok(u64::from(as_bool(v))),
            STR if is_short_str(v) || tag == Some(Tag::Str as u16) => {
                Ok(unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(v) } as u64)
            }
            BOXED if is_null(v) || is_undefined(v) => Ok(v),
            BOXED if self.u8(at + 1) == 0 && (is_int32(v) || is_double(v)) => Ok(v),
            BOXED if self.u8(at + 1) == 1 && is_bool(v) => Ok(v),
            OPT if is_undefined(v) => Ok(match self.u8(at + 1) {
                STR => (unsafe { __torajs_str_undef() }) as u64,
                NUM => torajs_rc::F64_UNDEF_SENTINEL_BITS,
                _ => undef_cell_ptr() as u64,
            }),
            // an f64 slot has no spelling for null: a field's takes it
            // to the sidecar (`super::obj`), a bare node refuses it
            OPT if is_null(v) && self.u8(at + 1) == NUM => Err(Fail::Mismatch),
            OPT if is_null(v) => Ok(0),
            OPT => unsafe { self.cast(at + 1, v) },
            REF => unsafe { self.cast(self.u32(at + 1) as usize, v) },
            CELL if tag == Some(self.u16(at + 1)) => {
                unsafe { __torajs_rc_inc(as_void_ptr(v)) };
                Ok(v)
            }
            ARR if tag == Some(Tag::Arr as u16) => unsafe { self.arr(at, as_void_ptr(v)) },
            OBJ => unsafe { self.obj(at, v) },
            _ => Err(Fail::Mismatch),
        }
    }

    /// Release slot bits [`Self::cast`] answered for the node at `at`.
    ///
    /// # Safety
    /// `bits` came from `cast` on that node.
    pub(super) unsafe fn release(&self, at: usize, bits: u64) {
        let at = match self.u8(at) {
            OPT => at + 1,
            _ => at,
        };
        if matches!(self.u8(at), INT | NUM | BOOL | BOXED) {
            return;
        }
        if bits == 0 || bits == undef_cell_ptr() as u64 || is_str_undef(bits) {
            return;
        }
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(bits) };
    }

    /// An array node: the slot's own copy of the array `arr`. A
    /// scalar or string element is judged by `__torajs_arr_any_to_typed`
    /// (which records its own TypeError); an `any` element takes the
    /// array as it is; any other element is cast one by one.
    ///
    /// # Safety
    /// `at` is an ARR node; `arr` a live Arr cell.
    unsafe fn arr(&self, at: usize, arr: *mut c_void) -> Result<u64, Fail> {
        let kind = self.u8(at + 1);
        let elem = at + 2;
        match self.u8(elem) {
            ANY => {
                unsafe { __torajs_rc_inc(arr) };
                return Ok(arr as u64);
            }
            INT | NUM | BOOL | STR => {
                let out = unsafe { __torajs_arr_any_to_typed(arr.cast(), i64::from(kind)) };
                return match unsafe { __torajs_throw_check() } {
                    0 => Ok(out as u64),
                    _ => Err(Fail::Thrown),
                };
            }
            _ => {}
        }
        if let Some(copy) = self.copy_of(arr as AnyValue, at) {
            return Ok(copy);
        }
        let len = unsafe { arr.cast::<u8>().add(ARR_LEN_OFF).cast::<u64>().read() };
        // every element node left here holds a heap pointer; marked so
        // a release of a half-filled copy releases what it holds
        let out = unsafe { __torajs_arr_alloc(len) };
        unsafe { __torajs_arr_mark_kind(out.cast(), u64::from(ARR_KIND_HEAP)) };
        self.note_copy(arr as AnyValue, at, out as u64);
        for i in 0..len {
            let e = unsafe { __torajs_arr_get_any_boxed(arr, i) };
            match unsafe { self.cast(elem, e) } {
                Ok(bits) => {
                    let grown = unsafe { __torajs_arr_push(out, bits as i64) };
                    debug_assert_eq!(grown, out, "allocated with its full length");
                }
                Err(fail) => {
                    unsafe { self.release(at, out as u64) };
                    return Err(fail);
                }
            }
        }
        Ok(out as u64)
    }
}

/// `bits` is the Str slot's undefined sentinel.
fn is_str_undef(bits: u64) -> bool {
    bits == unsafe { __torajs_str_undef() } as u64
}

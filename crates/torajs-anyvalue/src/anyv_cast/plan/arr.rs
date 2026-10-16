//! The array node of a cast plan (`super`): the slot's own copy of
//! an `any` array, element by element.

use core::ffi::c_void;

use torajs_rc::ARR_KIND_HEAP;

use super::{ANY, BOOL, Fail, INT, NUM, Plan, STR};
use crate::nanbox::AnyValue;

const ARR_LEN_OFF: usize = 8;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
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

impl Plan {
    /// An array node: the slot's own copy of the array `arr`. A
    /// scalar or string element is judged by `__torajs_arr_any_to_typed`
    /// (which records its own TypeError); an `any` element takes the
    /// array as it is; any other element is cast one by one.
    ///
    /// # Safety
    /// `at` is an ARR node; `arr` a live Arr cell.
    pub(super) unsafe fn arr(&self, at: usize, arr: *mut c_void) -> Result<u64, Fail> {
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

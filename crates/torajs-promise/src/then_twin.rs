//! A typed `.then` / `.catch` handler handed a value its parameter
//! lane cannot hold runs its any twin instead.
//!
//! The settled cell's stamp is the only runtime record of what it
//! holds, and a promise the checker types `Promise<number>` may still
//! settle through the any lane (`Promise.resolve(x)` with `x: any`, an
//! executor's `resolve`, a handler returning `any`). §27.2.5.4 calls
//! the handler with the value as it is (§10.2.1.3 binds it unconverted),
//! so a `"str"` reaching `(v: number) => …` is a string inside the
//! body. A typed body cannot hold it; the handler's boxed dual entry
//! (`__boxed_<name>`, the uniform `(env, argv, argc) -> AnyValue`
//! adapter) can, and hands a slot it cannot cross to the all-`any`
//! twin of the body. So a value that fits the lane is unboxed into it
//! as before, and one that does not makes the reaction call the
//! adapter and settle the derived cell with its boxed answer.
//!
//! The adapter is found from the handler pointer: a closure cell keeps
//! it at `env + 32` when the link kept the closure's mint of it; a
//! closure whose mint the link elided, and the bits-ABI wrap of an
//! f64-faced handler (whose own slot is 0 and whose capture 0 is the
//! real handler), are looked up in the table the compiler lays out for
//! the module's `.then` / `.catch` handlers (`__torajs_then_twins`;
//! torajs-rc carries the empty default). The slot read is the raw one,
//! not the `__torajs_closure_boxed_entry` seam: the table roots exactly
//! the adapters of the handlers it names, and reading through the seam
//! would keep every closure's adapter in every program that settles a
//! promise.
//!
//! The table's first word reaches all of this code: it is the module's
//! forwarder to [`__torajs_then_twin_run`], and the reaction calls
//! through it. A module that hands `.then` / `.catch` no handler with a
//! twin links the default, whose first word is 0, so the reaction reads
//! the value into the lane as before and none of the code below is
//! linked into the program.
//!
//! A handler with no adapter keeps the nearest-value conversion it had:
//! a declared function handed over bare (its typed body has no all-`any`
//! twin), a face the adapter cannot box, more than eight parameters.

use core::ffi::c_void;

use crate::layout::{REPR_ANY, REPR_BOOL, REPR_F64, REPR_HEAP, REPR_I64, REPR_STR};
use crate::settled_form::{converts, other_width};
use crate::then_box::{box_settled, unbox_settled};

/// AnySlotTag indices, as `__torajs_anyv_unbox_tag` reports them.
const TAG_BOOL: i64 = 1;
const TAG_I64: i64 = 2;
const TAG_F64: i64 = 3;
const TAG_HEAP: i64 = 4;

/// The boxed adapters read up to 8 argument slots unconditionally —
/// mirror of torajs-core `ssa_lower_boxed_entry::MAX_BOXED_PARAMS`.
const MAX_BOXED_ARGS: usize = 8;

/// Closure-cell slots — mirror of torajs-core `ssa_lower.rs`
/// `CLOSURE_BOXED_ENTRY_OFF` / `CLOSURE_CAP_BASE_OFF`.
const BOXED_ENTRY_OFF: usize = 32;
const CAP0_OFF: usize = 48;

unsafe extern "C" {
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_str_fits(v: u64) -> i64;
    /// Relative words, each an offset from the table's own address:
    /// first the module's forwarder to [`__torajs_then_twin_run`] (0 in
    /// the default table, which has nothing else), then rows of two,
    /// `(handler, adapter)`, ended by a 0 handler word. An adapter word
    /// equal to the handler word marks a bits-ABI wrap, whose capture 0
    /// is the closure that has the twin. The link lays out the length;
    /// the table is read through a pointer from its address.
    static __torajs_then_twins: [i64; 0];
}

/// The table's first word.
#[inline(always)]
fn table() -> *const i64 {
    core::ptr::addr_of!(__torajs_then_twins) as *const i64
}

/// The runner a module's table names, as its forwarder is called.
type TwinRun = unsafe extern "C" fn(i64, i64, i64, *mut c_void, i64, *mut i64) -> i64;

/// The argument of a typed handler `cb` (a closure cell when
/// `closure`) whose lane `param` is not the settled form `src`: `Ok`
/// with `v` read into the lane, or `Err` with the OWNED boxed answer
/// of the handler's any twin, which ran in its place on a value the
/// lane cannot hold.
///
/// # Safety
/// `v` is a live value of form `src`; `cb` is a live handler.
#[cold]
#[inline(never)]
pub(crate) unsafe fn handler_arg(
    src: u8,
    param: u8,
    v: i64,
    cb: *mut c_void,
    closure: bool,
) -> Result<i64, i64> {
    unsafe {
        let base = table();
        let run = *base;
        if run == 0 {
            return Ok(crate::settled_form::convert(src, param, v));
        }
        let run: TwinRun = core::mem::transmute((base as usize).wrapping_add(run as usize));
        let mut out = 0;
        if run(
            i64::from(src),
            i64::from(param),
            v,
            cb,
            i64::from(closure),
            &mut out,
        ) != 0
        {
            Err(out)
        } else {
            Ok(out)
        }
    }
}

/// [`handler_arg`] for a module that lists handlers with twins: writes
/// the handler's argument to `out` and answers 0, or writes the twin's
/// OWNED boxed answer and answers 1.
///
/// # Safety
/// As [`handler_arg`]; `src` and `param` are repr bytes, `out` is
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_then_twin_run(
    src: i64,
    param: i64,
    v: i64,
    cb: *mut c_void,
    closure: i64,
    out: *mut i64,
) -> i64 {
    unsafe {
        let (src, param) = (src as u8, param as u8);
        let (word, twin) = match handler_convert(src, param, v) {
            Ok(v) => (v, false),
            Err(bx) => match twin_or_nearest(cb, closure != 0, param, bx) {
                Ok(av) => (av, true),
                Err(v) => (v, false),
            },
        };
        *out = word;
        i64::from(twin)
    }
}

/// A handler's value read for its declared lane: `Ok` with the value
/// in that lane, or `Err` with the box the handler's twin takes — the
/// settled value fits no form the lane holds. `src != param`, and the
/// handler declared a typed lane.
///
/// # Safety
/// `v` is a live value of form `src`.
unsafe fn handler_convert(src: u8, param: u8, v: i64) -> Result<i64, i64> {
    if !converts(src, param) {
        return Ok(v);
    }
    if matches!((src, param), (REPR_I64, REPR_F64) | (REPR_F64, REPR_I64)) {
        return Ok(other_width(src, v));
    }
    unsafe {
        let bx = if src == REPR_ANY {
            v
        } else {
            box_settled(src, v)
        };
        if lane_fit(param, bx) {
            Ok(unbox_settled(param, bx))
        } else {
            Err(bx)
        }
    }
}

/// Does the box `av` cross into lane `param` as it is? Only a value of
/// the lane's own kind does: a `number` lane takes a number, an integer
/// lane an integral one, a `boolean` lane a boolean, a `string` lane a
/// string and a pointer lane a cell other than a string (an object's
/// layout is compile-time knowledge, so any object cell passes; a
/// string has a lane of its own and never sits in an object's).
unsafe fn lane_fit(param: u8, av: i64) -> bool {
    unsafe {
        let tag = __torajs_anyv_unbox_tag(av as u64);
        match param {
            REPR_F64 => matches!(tag, TAG_I64 | TAG_F64),
            REPR_I64 => {
                tag == TAG_I64
                    || tag == TAG_F64 && {
                        let f = f64::from_bits(__torajs_anyv_unbox_value(av as u64) as u64);
                        f.fract() == 0.0
                            && f.abs() < 9.007_199_254_740_992e15
                            && !(f == 0.0 && f.is_sign_negative())
                    }
            }
            REPR_BOOL => tag == TAG_BOOL,
            REPR_STR => __torajs_anyv_str_fits(av as u64) != 0,
            REPR_HEAP => {
                tag == TAG_HEAP
                    && crate::unhandled::reason_is_cell_like(av)
                    && __torajs_anyv_str_fits(av as u64) == 0
            }
            _ => true,
        }
    }
}

/// What a twin-table row says about a handler pointer.
enum Row {
    /// The handler's adapter.
    Entry(usize),
    /// A bits-ABI wrap whose capture 0 is the real handler, a closure.
    Wrap,
}

/// The row naming code address `code`, if the module lists one.
unsafe fn row(code: usize) -> Option<Row> {
    unsafe {
        let base = table();
        let at = base as usize;
        let mut p = base.add(1);
        loop {
            let key = *p;
            if key == 0 {
                return None;
            }
            if at.wrapping_add(key as usize) == code {
                let twin = *p.add(1);
                return Some(if twin == key {
                    Row::Wrap
                } else {
                    Row::Entry(at.wrapping_add(twin as usize))
                });
            }
            p = p.add(2);
        }
    }
}

/// The env and adapter a handler's twin call takes: `cb` is a closure
/// cell when `closure`, else a bare fn.
unsafe fn twin_of(cb: *mut c_void, closure: bool) -> Option<(*mut c_void, usize)> {
    unsafe {
        if !closure {
            return match row(cb as usize)? {
                Row::Entry(e) => Some((core::ptr::null_mut(), e)),
                Row::Wrap => None,
            };
        }
        let at = (cb as *const u8).add(BOXED_ENTRY_OFF) as *const usize;
        if *at != 0 {
            return Some((cb, *at));
        }
        match row(crate::closure_abi::fn_ptr(cb) as usize)? {
            Row::Entry(e) => Some((cb, e)),
            Row::Wrap => twin_of(
                *((cb as *const u8).add(CAP0_OFF) as *const *mut c_void),
                true,
            ),
        }
    }
}

/// The argument of a handler handed the box `bx` (borrowed, as a
/// handler's argument slot borrows the settled value) its lane cannot
/// hold: `Ok` with the OWNED boxed answer of the handler's twin, which
/// ran on it, or — a handler with no adapter — `Err` with the nearest
/// value the lane holds, for the handler itself. §27.2.5.4.1 runs the
/// handler with an undefined `this`, which a receiver-first cell takes
/// in argv[0].
///
/// # Safety
/// `cb` is a live closure cell when `closure`, else a handler fn
/// pointer; `bx` is a live box.
unsafe fn twin_or_nearest(cb: *mut c_void, closure: bool, param: u8, bx: i64) -> Result<i64, i64> {
    unsafe {
        let Some((env, entry)) = twin_of(cb, closure) else {
            return Err(unbox_settled(param, bx));
        };
        let undef = crate::closure_abi::undefined_box() as u64;
        let mut argv = [undef; MAX_BOXED_ARGS];
        let argc = if !env.is_null() && crate::closure_abi::wants_receiver(env) {
            argv[1] = bx as u64;
            2
        } else {
            argv[0] = bx as u64;
            1
        };
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            core::mem::transmute(entry);
        Ok(call(env, argv.as_ptr(), argc) as i64)
    }
}

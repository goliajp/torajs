//! Any-param callback support for the typed `.then` / `.catch`
//! kernels (RFC 20260720-promise-any-cb knife 1).
//!
//! A `(v: any) => R` handler's body expects its argument as NaN-box
//! AnyValue bits, but the typed kernels move the settled value as raw
//! i64. The call site cannot box statically — the SSA layer's
//! `Type::Promise` is inner-T erased — so the kernel boxes at
//! dispatch time from the cell's `value_repr` stamp (the same truth
//! source the any-lane bridge reads). The lowering marks such
//! handlers by setting [`PARAM_ANY_FLAG`] on the kernels' `ret_repr`
//! parameter; low byte stays the return-repr code.
//!
//! An UNSTAMPED source (a mint family without repr wiring) refuses
//! LOUDLY at attach time — never a silent mis-box.

use core::ffi::c_void;
use core::sync::atomic::{AtomicI64, Ordering};

use crate::layout::{
    Promise, REPR_ANY, REPR_BOOL, REPR_F64, REPR_HEAP, REPR_I64, REPR_NULL, REPR_STR,
    REPR_UNSTAMPED, REPR_VOID, as_promise,
};

/// Bit 8 of the then/catch kernels' `ret_repr` parameter — set when
/// the handler's first parameter is `any` (dispatch boxes the settled
/// value per the source's repr stamp before the call). Low byte keeps
/// the return-repr code. Mirror:
/// `torajs-core/src/ssa_lower_promise_repr_mark.rs::PARAM_ANY_FLAG` —
/// must move in lockstep.
pub const PARAM_ANY_FLAG: i64 = 256;

/// Bits 16-23 of the same word — the handler's FIRST-PARAMETER repr,
/// for the mirror direction: a cell settled with an `any` handed to a
/// handler that declared a typed lane. Zero when the parameter is
/// `any` (see [`PARAM_ANY_FLAG`]) or a form with no repr code.
/// Mirror: `torajs-core/src/ssa_lower_promise_repr_mark.rs::
/// PARAM_REPR_SHIFT` — must move in lockstep.
pub const PARAM_REPR_SHIFT: i64 = 16;

/// AnySlotTag indices, as `__torajs_anyv_unbox_tag` reports them
/// (`torajs-anyvalue/src/nanbox_encode.rs`).
const TAG_BOOL: i64 = 1;
const TAG_I64: i64 = 2;
const TAG_F64: i64 = 3;
const TAG_HEAP: i64 = 4;

unsafe extern "C" {
    pub(crate) fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_box_str_slot(s: *mut c_void) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — the in-flight flag, read in place as user code
    /// reads it rather than through `__torajs_throw_check`
    static __torajs_throw_active: AtomicI64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    /// torajs-rc — is this pointer the immortal undefined cell?
    fn __torajs_is_undef_cell(p: *const u8) -> i64;
    /// torajs-anyvalue `promise_thenable` — a handler return that is a
    /// user thenable (RFC 20260913): settles `result` through the
    /// §27.2.1.3.2 job and answers 1; 0 = not a thenable, store it.
    fn __torajs_anyv_promise_settle_thenable(result: *mut c_void, repr: i64, value: i64) -> i64;
}

// The thenable probe of a handler's or an async body's Object return
// (§27.2.1.3.2 steps 8-9: Get(resolution, "then")), behind a seam whose
// default lives in `torajs-dispatch`. Only two things hand this file a
// return in object form: a call site whose static return form is an
// object or `any` (the compiler then emits a rooted user fn that calls
// `__torajs_promise_obj_return_mark`), and a handler's any twin
// (`__torajs_then_twin_run`). With both dead the link shadows the seam
// with a loud-reject stub; the probe behind it roots the generic value
// world. Unit tests carry no dispatch member, so they bridge directly.
#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_promise_settle_thenable_slow(result: *mut c_void, repr: i64, value: i64) -> i64;
}
#[cfg(test)]
unsafe fn __torajs_promise_settle_thenable_slow(result: *mut c_void, repr: i64, value: i64) -> i64 {
    unsafe { __torajs_anyv_promise_settle_thenable(result, repr, value) }
}

static OBJ_RETURN_SEEN: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// The compiler's evidence that some call site settles a promise with
/// an object-form return: a module with such a site emits a rooted
/// user fn calling this entry, so its text liveness is what the link
/// judges the thenable seam on. Nothing runs it; the store only keeps
/// the body its own (an empty one could be folded onto another).
#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_promise_obj_return_mark() {
    OBJ_RETURN_SEEN.store(1, Ordering::Relaxed);
}

/// The NaN-box gate of `then_adopt::returned_promise`: a cell pointer
/// has its top 16 bits and the other-type tag bit clear.
pub(crate) fn box_is_cell(bits: i64) -> bool {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    let b = bits as u64;
    b != 0 && b & TOP_16_MASK == 0 && b & TAG_BIT_TYPE_OTHER == 0
}

/// §27.2.2.1 PromiseReactionJob steps 8-9 — a handler that completes
/// ABRUPTLY rejects the derived promise with the thrown value; it does
/// not fulfill it with the (absent) return. Answers `true` when it
/// settled `result`, so the caller skips its resolve leg and goes
/// straight to teardown.
///
/// Until this existed the typed kernels read the return register
/// regardless and resolved with whatever garbage it held, so the throw
/// stayed pending in the TLS with no live frame to check it: a
/// `.then(() => { throw e })` chain silently turned into a fulfillment
/// and every downstream `.catch` was dead code. The any-lane bridge
/// (torajs-anyvalue `method_call_promise.rs::then_any_dispatch`) has
/// always done this — the two lanes disagreeing is what let async
/// test262 cases whose assertion throws inside a `.then` handler report
/// success.
///
/// Reason encoding mirrors that bridge exactly: box the (tag, value)
/// pair off the throw TLS and stamp the cell `REPR_ANY`, so a
/// downstream handler's [`handler_value`] converts it into whatever lane
/// that handler declared. `value_is_heap` marks the boxed stake as the
/// cell's to release; the take composition (tag peeked before value —
/// `take` clears `active` but leaves the tag slot) transfers the
/// throw's stake into the box.
pub(crate) unsafe fn reject_on_pending_throw(result: *mut c_void) -> bool {
    unsafe {
        if __torajs_throw_active.load(Ordering::Relaxed) == 0 {
            return false;
        }
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        crate::frame::reject_with_taken(result, tag, value);
        true
    }
}

/// A reaction handler's complete outcome protocol, in spec order: an
/// abrupt completion rejects (§27.2.2.1 steps 8-9), a returned promise
/// is ADOPTED rather than stored (§27.2.1.3.2), a returned user
/// thenable goes through the NewPromiseResolveThenableJob (the
/// anyvalue hook — only a box or a heap cell can carry one), and
/// anything else fulfills the derived promise verbatim under the call
/// site's static return repr.
pub(crate) unsafe fn settle_handler_return(result: *mut c_void, ret_repr: u8, value: i64) {
    unsafe {
        if reject_on_pending_throw(result) {
            return;
        }
        if let Some(inner) = crate::then_adopt::returned_promise(ret_repr, value) {
            crate::then_adopt::adopt_into(result, inner);
            return;
        }
        let object = ret_repr == REPR_HEAP || (ret_repr == REPR_ANY && box_is_cell(value));
        if object && __torajs_promise_settle_thenable_slow(result, ret_repr as i64, value) != 0 {
            return;
        }
        let pp = as_promise(result);
        (*pp).value_repr = ret_repr;
        // The return is OWNED (a callee hands back its own reference),
        // and the cell is where it now lives: mark the stake so the
        // cell's drop releases it. The mint marks the same forms
        // (`Promise.resolve(x)` over a Str / heap / any argument);
        // an immediate behind the any stamp passes the drop's NaN-box
        // gate untouched. Left unmarked, every array, string or
        // object a handler returned stayed allocated for the life of
        // the program.
        (*pp).value_is_heap = matches!(ret_repr, REPR_STR | REPR_HEAP | REPR_ANY) as u8;
        crate::state::__torajs_promise_resolve(result, value);
    }
}

/// Box the source's settled value per its repr stamp — mirror of the
/// any-lane bridge's `box_settled` (`torajs-anyvalue/src/
/// method_call_promise.rs`, must move in lockstep). Rc-neutral: the
/// box is a borrow of the cell's stake for the callback's argument
/// slot (the STR arm's helper materializes/incs internally, same
/// contract as the any-lane caller).
pub(crate) unsafe fn box_settled(repr: u8, value: i64) -> i64 {
    unsafe {
        if settled_is_undefined(repr, value) {
            return __torajs_anyv_box_from_pair(5, 0) as i64;
        }
        (match repr {
            REPR_I64 => __torajs_anyv_box_from_pair(2, value),
            REPR_F64 => __torajs_anyv_box_from_pair(3, value),
            REPR_BOOL => __torajs_anyv_box_from_pair(1, value),
            REPR_STR => __torajs_anyv_box_str_slot(value as *mut c_void),
            REPR_HEAP => __torajs_anyv_box_from_pair(4, value),
            REPR_ANY => value as u64,
            REPR_VOID => __torajs_anyv_box_from_pair(5, 0),
            REPR_NULL => __torajs_anyv_box_from_pair(0, 0),
            // unreachable — the attach-time gate refused UNSTAMPED.
            _ => __torajs_anyv_box_from_pair(5, 0),
        }) as i64
    }
}

/// A typed cell fulfilled with nothing carries `undefined` in its
/// slot's own spelling — the F64 sentinel in a `number` slot
/// (torajs-rc `undef_f64::F64_UNDEF_SENTINEL_BITS`, mirrored here in
/// lockstep), the immortal undefined cell in a pointer slot. Boxing
/// either as its slot's form loses the `undefined`: a NaN, an
/// `[object]` (643, `async-fallthrough-typed-001`).
pub(crate) unsafe fn settled_is_undefined(repr: u8, value: i64) -> bool {
    const F64_UNDEF_SENTINEL_BITS: u64 = 0x7FFC_0000_0000_000A;
    match repr {
        REPR_F64 => value as u64 == F64_UNDEF_SENTINEL_BITS,
        REPR_HEAP => value != 0 && unsafe { __torajs_is_undef_cell(value as *const u8) } != 0,
        _ => false,
    }
}

/// Unbox a settled AnyValue into the raw form a TYPED handler
/// parameter expects — the mirror of [`box_settled`], and rc-neutral
/// for the same reason: the argument slot borrows the cell's stake,
/// exactly as a same-repr cell's value does today.
///
/// Without this the typed kernels handed the box POINTER straight to
/// the handler, so `Promise.resolve(x as any)` read back through
/// `(v: number)` bitcast a pointer into an f64 (NaN) and through
/// `(v: string)` dereferenced it as a Str (SIGSEGV).
///
/// A tag that disagrees with the declared lane means the annotation
/// lied — TS `any` type-checks that and hands the value over
/// unconverted, which a typed lane cannot represent. A handler hands
/// such a value to its any twin instead ([`crate::then_twin`]); the
/// readers with no twin to go to (`await`, a combinator's result slot,
/// a handler without an adapter) take the nearest answer the lane can
/// hold (ToNumber-shaped for the number lane, truthiness for the
/// boolean one).
pub(crate) unsafe fn unbox_settled(param_repr: u8, av: i64) -> i64 {
    unsafe {
        // The target lane IS `any` — hand the box back whole, before
        // any unbox can touch it. The callers all route any-parameters
        // elsewhere, but a lane that unwraps what it was asked to
        // preserve is the exact defect this function exists to fix.
        if param_repr == REPR_ANY {
            return av;
        }
        let tag = __torajs_anyv_unbox_tag(av as u64);
        // A ShortStr reports tag Heap and `unbox_value` mints an owned
        // materialization; the scalar lanes below never consume it
        // (their Heap arms answer a constant), so they must not unbox
        // one — same nearest answers the heap-Str arm gives (NaN / 0 /
        // truthy). The STR/HEAP fallback lane DOES need the pointer:
        // there the materialization is the value itself, the handler
        // borrows it with no owner to reclaim it — the boxed-entry
        // Class B leg (546-02) owns that story.
        let heap_immediate = tag == TAG_HEAP && !crate::unhandled::reason_is_cell_like(av);
        let raw = if heap_immediate && matches!(param_repr, REPR_F64 | REPR_I64 | REPR_BOOL) {
            1
        } else {
            __torajs_anyv_unbox_value(av as u64)
        };
        match param_repr {
            REPR_F64 => match tag {
                TAG_F64 => raw,
                TAG_I64 | TAG_BOOL => (raw as f64).to_bits() as i64,
                _ => f64::NAN.to_bits() as i64,
            },
            REPR_I64 => match tag {
                TAG_I64 | TAG_BOOL => raw,
                TAG_F64 => crate::settled_form::exact_i64(f64::from_bits(raw as u64)),
                _ => 0,
            },
            REPR_BOOL => match tag {
                TAG_BOOL => raw,
                TAG_F64 => i64::from(f64::from_bits(raw as u64) != 0.0),
                _ => i64::from(raw != 0),
            },
            // A cell pointer is already the slot form both the STR and
            // HEAP lanes move values in (REPR_ANY returned early).
            _ => raw,
        }
    }
}

/// The value a handler's first parameter receives: boxed when it
/// declared `any`, otherwise read into the lane it declared the way
/// every other reader of a settled cell is. A typed pair that disagrees
/// reaches it too: `Promise.resolve(p)` where `p` is an `any` holding
/// a promise answers the INNER cell (§27.2.4.7 step 2), whose stamp
/// says i64, while the call site's erased `Promise<any>` let a
/// `(x: number)` handler declare f64. `Err` carries the OWNED boxed
/// answer of the handler's any twin, which ran in its place on a value
/// the declared lane cannot hold ([`crate::then_twin`]).
///
/// # Safety
/// `src` is a live settled cell; `cb` is the handler (a closure cell
/// when `closure`, else a fn pointer).
#[inline(always)]
pub(crate) unsafe fn handler_value(
    src: *const Promise,
    param_any: bool,
    param_repr: u8,
    cb: *mut c_void,
    closure: bool,
) -> Result<i64, i64> {
    unsafe {
        let (src_repr, value) = ((*src).value_repr, (*src).value);
        if param_any {
            Ok(box_settled(src_repr, value))
        } else if src_repr == param_repr {
            Ok(value)
        } else {
            crate::then_twin::handler_arg(src_repr, param_repr, value, cb, closure)
        }
    }
}

/// Attach-time gate for an any-param handler: an UNSTAMPED source has
/// no repr wiring, so dispatch could only mis-box. Throw before any
/// callback runs; the call site's throw-check propagates it. Returns
/// `true` when the attach must be refused.
pub(crate) unsafe fn refuse_unstamped(source: *mut c_void) -> bool {
    unsafe {
        if (*as_promise(source)).value_repr == REPR_UNSTAMPED {
            __torajs_throw_type_error(
                c"promise value form unknown to the any-param handler on this receiver".as_ptr(),
            );
            return true;
        }
    }
    false
}

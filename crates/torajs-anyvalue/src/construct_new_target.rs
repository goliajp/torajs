//! RFC 20260917-plain-fn-new-target knife 3 — the construct face of a
//! plain function that reads `new.target`.
//!
//! Such a function takes `(__this, __new_target, …user)`
//! (`torajs-core` `ast/new_target_param.rs`), but its value face — the
//! closure cell any runtime value of the function is — forwards
//! `undefined` for the slot, because every call through a value is a
//! [[Call]]. [[Construct]] through a value (`const K = F; new K()`,
//! `Reflect.construct(F, args, nt)`) has to reach the body with the
//! real new.target instead, so the compiler also mints a construct face
//! whose boxed adapter takes `[this, newTarget, …args]` and registers
//! it here, keyed by the value face's boxed entry. The construct kernel
//! asks this table first; a function that does not read `new.target`
//! has no entry and keeps the ordinary call channel.
//!
//! Closure flags are fully allocated, so the association lives in a
//! table rather than a bit. It only grows, one entry per function that
//! reads `new.target` and is used as a value; Atomics per the
//! multi-thread-ready substrate rule (design principles §6.2) with the
//! codebase's single-mutator Relaxed ordering.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::method_call_closure_dispatch::boxed_entry_of;
use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

/// (value face boxed entry, construct face boxed entry).
type Faces = Vec<(u64, u64)>;

static FACES: AtomicPtr<Faces> = AtomicPtr::new(core::ptr::null_mut());

/// Record `ctor_face` as the construct face of `value_face`. Both are
/// closure cells the module mints once at start-up; the table keeps
/// their boxed entries only and takes no reference.
///
/// # Safety
/// Both operands are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_new_target_register(value_face: AnyValue, ctor_face: AnyValue) {
    if !is_cell(value_face) || !is_cell(ctor_face) {
        return;
    }
    let (key, entry) = unsafe {
        (
            boxed_entry_of(as_void_ptr(value_face).cast::<u8>()),
            boxed_entry_of(as_void_ptr(ctor_face).cast::<u8>()),
        )
    };
    if key == 0 || entry == 0 {
        return;
    }
    record(key, entry);
}

/// Knife 2 — the same record from raw boxed entries: a function
/// expression's cell registers its construct face as it is minted.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_new_target_register_entries(value_entry: u64, ctor_entry: u64) {
    if value_entry != 0 && ctor_entry != 0 {
        record(value_entry, ctor_entry);
    }
}

fn record(key: u64, entry: u64) {
    let mut table = FACES.load(Ordering::Relaxed);
    if table.is_null() {
        table = Box::into_raw(Box::new(Faces::new()));
        FACES.store(table, Ordering::Relaxed);
    }
    // SAFETY: leaked for the process, single mutator.
    let table = unsafe { &mut *table };
    match table.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = entry,
        None => table.push((key, entry)),
    }
}

/// The construct face registered for a value face's boxed entry, 0 when
/// none.
pub(crate) fn ctor_face_entry(value_entry: u64) -> u64 {
    let table = FACES.load(Ordering::Relaxed);
    if table.is_null() {
        return 0;
    }
    // SAFETY: leaked for the process, single mutator.
    unsafe { &*table }
        .iter()
        .find(|(k, _)| *k == value_entry)
        .map_or(0, |(_, e)| *e)
}

/// Run a construct face: `[this, newTarget, …argv]` through its adapter,
/// with the constructed closure cell as env (a function expression's
/// face reads its captures there; a declaration's ignores it).
///
/// # Safety
/// `entry` is a registered construct face; `env` the live cell it was
/// registered for; `argv` holds `argc` live AnyValues; `this_av` and
/// `new_target` are live.
pub(crate) unsafe fn invoke_ctor_face(
    env: *mut c_void,
    entry: u64,
    this_av: AnyValue,
    new_target: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let n = argc.max(0) as usize;
    let mut full = Vec::with_capacity(n + 2);
    full.push(this_av);
    full.push(new_target);
    for i in 0..n {
        full.push(unsafe { *argv.add(i) });
    }
    unsafe {
        crate::method_call_closure_dispatch::invoke_boxed(env, entry, full.as_ptr(), (n + 2) as i64)
    }
}

//! Class-method reification — the own `C.prototype.<m>` function
//! object (RFC 20260717-class-first-class-value knife B cut 1).
//!
//! tr dispatches class methods through the nominal vtable + the
//! `.__class_methods_<i>` boxed-adapter table; the prototype dynobj
//! itself carried no own method entries, so
//! `Object.getOwnPropertyDescriptor(C.prototype, "m")` answered
//! undefined and `C.prototype.m()` missed. This module mints the
//! introspection-side function object, mirroring
//! [`crate::method_value`]'s builtin-method cell shape:
//!
//! - Layout is a capture-less closure env (universal header +
//!   fn_addr + drop_fn + props + boxed_entry + one capture slot
//!   holding the ADAPTER vaddr), so every existing callable probe
//!   (`typeof` → "function", strict-eq identity, expando reads)
//!   works unchanged.
//! - `FLAG_STATIC_LITERAL` — rc traffic no-ops, the cycle collector
//!   skips it, the cell never drops (the prototype entry that owns
//!   it is itself a process-lifetime singleton).
//! - `boxed_entry` points at [`class_bare_entry`] — a bare call
//!   (direct, HOF callback, any-call) is the ES `this = undefined`
//!   TypeError. The sentinel doubles as the recognizer:
//!   [`class_method_adapter`] answers the carried adapter vaddr only
//!   for cells whose boxed_entry IS the sentinel (the same
//!   discrimination scheme `builtin_method_mid` uses).
//! - Method-call sites that resolve a dynobj entry to one of these
//!   cells invoke the adapter through the uniform boxed ABI with the
//!   RECEIVER in the env slot (`adapter(this-as-env, argv, argc)`),
//!   the exact shape `struct_method` dispatch uses.

use core::ffi::c_void;

use torajs_rc::{FLAG_CLASS_METHOD_THIS_FREE, FLAG_STATIC_LITERAL, Tag};

use crate::nanbox::VALUE_UNDEFINED;

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

// Cell layout offsets — mirror of `method_value.rs` (itself a mirror
// of torajs-core's closure-env constants).
const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const CLOSURE_CAP_BASE_OFF: usize = 48;
/// Blade 3 (RFC 20260804-method-rebind-generic-body) — two extra
/// method-face slots after the adapter: the owning class's tag (the
/// receiver guard's expected value) and the `__cmany_` twin's boxed
/// adapter (0 = no twin; guard fail keeps the mono path — the
/// recorded super-route residue).
const CLS_CELL_TAG_OFF: usize = 56;
const CLS_CELL_TWIN_OFF: usize = 64;
const CELL_SIZE: usize = 72;

/// Boxed dual entry of every reified class-method cell — a bare call
/// is the ES `this = undefined` TypeError. Also the recognizer
/// sentinel for [`class_method_adapter`].
unsafe extern "C" fn class_bare_entry(_env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"class method called without a receiver (this is undefined)".as_ptr(),
        );
    }
    VALUE_UNDEFINED
}

/// Native entry — an any→typed fn-slot cast direct-calls this
/// instead of jumping to 0; the pending throw propagates at the
/// callee boundary.
unsafe extern "C" fn class_native_entry() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"class method called without a receiver (this is undefined)".as_ptr(),
        );
    }
    0
}

/// Mint one immortal class-method function object carrying `adapter`
/// (a `.__class_methods_<i>` boxed adapter vaddr). Called once per
/// (class, method) from `__torajs_anyv_class_register`'s wiring at
/// module init — the prototype's own entry keeps the sole reference,
/// so no interning table is needed for identity (`C.prototype.m ===
/// C.prototype.m` reads the same entry).
///
/// `this_free` (S2.38) — non-zero when the compiler proved the mono
/// body never reads its receiver: the cell carries
/// [`FLAG_CLASS_METHOD_THIS_FREE`] and `invoke_with_this` runs bare /
/// primitive-`this` calls with a null receiver instead of the
/// this-undefined TypeError (ES §10.2.1.2 — a this-free body runs
/// regardless of the thisArgument).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_class_method_cell_new(
    adapter: u64,
    this_free: u64,
    class_tag: u64,
    twin: u64,
) -> *mut u8 {
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL
            | if this_free != 0 {
                FLAG_CLASS_METHOD_THIS_FREE
            } else {
                0
            };
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = class_native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = class_bare_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = adapter;
        *(cell.add(CLS_CELL_TAG_OFF) as *mut u64) = class_tag;
        *(cell.add(CLS_CELL_TWIN_OFF) as *mut u64) = twin;
        cell
    }
}

/// Blade 3 — the face's receiver-guard pair `(class_tag, twin)`.
/// Only meaningful for a cell [`class_method_face_adapter`] already
/// recognized; a `class_tag` of 0 disarms the guard (runtime-define
/// mint sites carry no tag).
///
/// # Safety
/// `ptr` points at a live reified class-METHOD cell.
pub(crate) unsafe fn class_method_face_guard(ptr: *mut c_void) -> (u64, u64) {
    unsafe {
        (
            *(ptr.cast::<u8>().add(CLS_CELL_TAG_OFF) as *const u64),
            *(ptr.cast::<u8>().add(CLS_CELL_TWIN_OFF) as *const u64),
        )
    }
}

/// The fn-addr-registry lookup key of a closure cell (RFC
/// 20260719-fn-tostring-source B6c) — a class-method face keys on
/// its carried adapter vaddr (the compiler bakes one registry row
/// per dispatchable class method against the boxed adapter fn; the
/// face's own fn_addr is the throwing native entry, never in the
/// registry), every other cell on its own fn_addr. An accessor
/// face also answers its adapter here — no registry row exists for
/// accessor adapters, so lookups keep their miss behavior.
///
/// # Safety
/// `ptr` points at a live `Tag::Closure` heap cell.
pub(crate) unsafe fn registry_addr(ptr: *mut c_void) -> u64 {
    unsafe {
        if let Some(adapter) = class_method_adapter(ptr) {
            return adapter;
        }
        *(ptr.cast::<u8>().add(CLOSURE_FN_ADDR_OFF) as *const u64)
    }
}

/// The carried adapter vaddr when `ptr` is a reified class-method
/// cell (its boxed_entry is the [`class_bare_entry`] sentinel) or a
/// reified class-accessor face ([`class_accessor_bare_entry`]);
/// `None` for every other closure cell.
///
/// # Safety
/// `ptr` points at a live `Tag::Closure` heap cell.
/// METHOD-face-only twin of [`class_method_adapter`] — answers the
/// adapter for a reified class-method cell and `None` for an
/// accessor face (whose adapter is a getter body, never a callee):
/// `invoke_with_this` uses this to route a detached method value
/// back through the receiver-in-env adapter ABI without turning
/// accessor faces callable.
///
/// # Safety
/// `ptr` points at a live `Tag::Closure` heap cell.
pub(crate) unsafe fn class_method_face_adapter(ptr: *mut c_void) -> Option<u64> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == class_bare_entry as *const () as u64 {
            Some(*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64))
        } else {
            None
        }
    }
}

pub(crate) unsafe fn class_method_adapter(ptr: *mut c_void) -> Option<u64> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == class_bare_entry as *const () as u64
            || entry == class_accessor_bare_entry as *const () as u64
        {
            Some(*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64))
        } else {
            None
        }
    }
}

// ============================================================
// Class-accessor faces (RFC 20260718-accessor-reify 刀 2)
// ============================================================

/// Extended cell layout for accessor faces — two extra slots after
/// the adapter carry the reflection metadata a bare method cell
/// lacks: the interned `.name` Str ("get x" / "set x", §17 prefix
/// form) and the ES `length`.
const ACC_CELL_NAME_OFF: usize = 56;
const ACC_CELL_LEN_OFF: usize = 64;
const ACC_CELL_SIZE: usize = 72;

/// Boxed dual entry of a reified class-accessor face — its own
/// sentinel (distinct from [`class_bare_entry`]) so the reflection
/// probes know the extended slots exist; a bare call is the same
/// `this = undefined` TypeError.
unsafe extern "C" fn class_accessor_bare_entry(
    _env: *mut c_void,
    _argv: *const u64,
    _argc: i64,
) -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"class accessor called without a receiver (this is undefined)".as_ptr(),
        );
    }
    VALUE_UNDEFINED
}

/// Mint one immortal class-accessor face carrying `adapter` plus its
/// reflection metadata. `name` transfers — a fresh Str cell the face
/// keeps forever (the face itself never drops).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_accessor_cell_new(
    adapter: u64,
    name: *mut u8,
    length: u64,
) -> *mut u8 {
    // SAFETY: fresh ACC_CELL_SIZE allocation, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(ACC_CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = class_native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) =
            class_accessor_bare_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = adapter;
        *(cell.add(ACC_CELL_NAME_OFF) as *mut u64) = name as u64;
        *(cell.add(ACC_CELL_LEN_OFF) as *mut u64) = length;
        cell
    }
}

/// The `(name Str cell, length)` metadata when `ptr` is a reified
/// class-accessor face; `None` otherwise. The name cell is the
/// face's own immortal stake — callers rc_inc before handing out.
///
/// # Safety
/// `ptr` points at a live `Tag::Closure` heap cell.
pub(crate) unsafe fn class_accessor_meta(ptr: *mut c_void) -> Option<(*mut u8, u64)> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == class_accessor_bare_entry as *const () as u64 {
            Some((
                *(ptr.cast::<u8>().add(ACC_CELL_NAME_OFF) as *const u64) as *mut u8,
                *(ptr.cast::<u8>().add(ACC_CELL_LEN_OFF) as *const u64),
            ))
        } else {
            None
        }
    }
}

/// Staticlib face — the adapter a class face carries (`0` = not a
/// class face). torajs-dynobj's accessor-pair invoke probes here
/// before the kinds dispatch (a face's boxed dual entry is the
/// bare-receiver throw, not an invokable adapter).
///
/// # Safety
/// `p` is null or a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_face_adapter(p: *const c_void) -> u64 {
    if p.is_null() {
        return 0;
    }
    unsafe {
        if (p.cast::<u8>().add(4) as *const u16).read() != Tag::Closure as u16 {
            return 0;
        }
        class_method_adapter(p as *mut c_void).unwrap_or(0)
    }
}

/// Invoke a class-face adapter against a receiver — the accessor
/// twin of the method-call dispatch (`adapter(receiver-as-env,
/// argv, argc)`). A non-cell receiver is the bare-receiver
/// TypeError (a class body's `this` must be an instance).
///
/// # Safety
/// `recv` / `argv` carry valid AnyValue bit patterns; `argv` has
/// `argc` readable slots (null iff `argc == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_face_invoke(
    adapter: u64,
    recv: u64,
    argv: *const u64,
    argc: i64,
) -> u64 {
    unsafe {
        if !crate::nanbox::is_cell(recv) {
            return class_bare_entry(core::ptr::null_mut(), argv, argc);
        }
        let env = crate::nanbox::as_void_ptr(recv);
        crate::method_call::invoke_boxed(env, adapter, argv, argc)
    }
}

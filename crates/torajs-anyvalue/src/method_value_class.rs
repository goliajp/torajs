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

mod accessor;
mod ctor_method;

use accessor::class_accessor_bare_entry;
pub(crate) use accessor::class_accessor_meta;
pub(crate) use ctor_method::class_ctor_method;

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-value-drop — release one reference on a heap cell.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — scrub a cell from the cycle root buffer.
    fn __torajs_cycle_unbuffer(p: *mut c_void);
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
/// 564-01 — the §10.2.9 name a COMPUTED member's face carries,
/// derived from the key at definition time (0 for every face whose
/// name is the source spelling the fn-name registry already holds).
const CLS_CELL_NAME_OFF: usize = 72;
/// K2b — the evaluation's class object a UNIT face holds (RFC
/// 20260917-class-eval-unit), 0 on every immortal one-per-class face.
/// Non-zero is what makes the cell mortal and traceable.
const CLS_CELL_UNIT_OFF: usize = 80;
const CELL_SIZE: usize = 88;
/// The closure trace hook a mortal unit face installs (mirror of
/// torajs-cycle `CLOSURE_TRACE_FN_OFF`); 0 on every other face.
const CLOSURE_TRACE_FN_OFF: usize = 40;

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
    name: *mut u8,
) -> *mut u8 {
    // SAFETY: a spec whose `unit` is 0 — the immortal shape.
    unsafe {
        mint(MethodCellSpec {
            adapter,
            this_free,
            class_tag,
            twin,
            name,
            unit: 0,
        })
    }
}

/// K2b — one evaluation's face of a unit class (RFC
/// 20260917-class-eval-unit). §15.7.14 gives every evaluation its own
/// method function objects, so this face is minted per evaluation
/// rather than once per (class, row), and it is therefore MORTAL: no
/// `FLAG_STATIC_LITERAL`, a real [`method_face_drop`], and a
/// [`method_face_trace`] over the one child it holds — `unit`, the
/// evaluation's class object, whose prototype owns this very face.
/// That is a cycle by construction (class → prototype → face →
/// class), which is exactly why the trace hook is not optional.
///
/// `unit` is the class object's cell pointer, carrying a `+1` this
/// cell owns.
///
/// # Safety
/// `unit` is a live heap cell pointer (never 0 — a 0 unit would mint
/// a mortal cell with no owner to release, use the immortal twin).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_method_cell_new_unit(
    adapter: u64,
    this_free: u64,
    class_tag: u64,
    twin: u64,
    unit: u64,
) -> *mut u8 {
    // SAFETY: caller contract above.
    unsafe {
        mint(MethodCellSpec {
            adapter,
            this_free,
            class_tag,
            twin,
            name: core::ptr::null_mut(),
            unit,
        })
    }
}

/// What one reified method face is made of. A struct rather than six
/// positional arguments: `adapter`, `twin` and `unit` are all bare
/// `u64` words and swapping two of them compiles.
struct MethodCellSpec {
    adapter: u64,
    this_free: u64,
    class_tag: u64,
    twin: u64,
    name: *mut u8,
    unit: u64,
}

/// Mint one reified class-method function object. Immortal when
/// `spec.unit` is 0 (one per class row, the prototype entry outlives
/// the process), mortal when it is not (one per evaluation).
///
/// # Safety
/// `spec.name` is null or an owned Str cell; `spec.unit` is 0 or a
/// live heap cell this face takes a `+1` on.
unsafe fn mint(spec: MethodCellSpec) -> *mut u8 {
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = if spec.unit != 0 {
            0
        } else {
            FLAG_STATIC_LITERAL
        } | if spec.this_free != 0 {
            FLAG_CLASS_METHOD_THIS_FREE
        } else {
            0
        };
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = class_native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = if spec.unit != 0 {
            method_face_drop as *const () as u64
        } else {
            0
        };
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = class_bare_entry as *const () as u64;
        if spec.unit != 0 {
            *(cell.add(CLOSURE_TRACE_FN_OFF) as *mut u64) = method_face_trace as *const () as u64;
            torajs_rc::__torajs_rc_inc(spec.unit as *mut c_void);
        }
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = spec.adapter;
        *(cell.add(CLS_CELL_TAG_OFF) as *mut u64) = spec.class_tag;
        *(cell.add(CLS_CELL_TWIN_OFF) as *mut u64) = spec.twin;
        *(cell.add(CLS_CELL_NAME_OFF) as *mut u64) = spec.name as u64;
        *(cell.add(CLS_CELL_UNIT_OFF) as *mut u64) = spec.unit;
        cell
    }
}

/// drop_fn of a MORTAL unit face — release the expando bag and the
/// held class object, then the block (the
/// `proxy_revocable::revoker_drop` shape).
///
/// # Safety
/// `env` is a live unit-face cell at refcount 0.
unsafe extern "C" fn method_face_drop(env: *mut c_void) {
    unsafe {
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let props = *(cell.add(CLOSURE_PROPS_OFF) as *const u64);
        if props != 0 {
            __torajs_value_drop_heap(props as *mut c_void);
        }
        let unit = *(cell.add(CLS_CELL_UNIT_OFF) as *const u64);
        if unit != 0 {
            __torajs_value_drop_heap(unit as *mut c_void);
        }
        std::alloc::dealloc(
            cell,
            core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap(),
        );
    }
}

/// trace_fn of a MORTAL unit face — the held class object is the
/// cell's one extra child.
unsafe extern "C" fn method_face_trace(
    env: *mut c_void,
    visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    ctx: *mut c_void,
) {
    unsafe {
        let slot = env.cast::<u8>().add(CLS_CELL_UNIT_OFF);
        visit(
            0,
            *(slot as *const u64) as *mut c_void,
            slot as *mut c_void,
            ctx,
        );
    }
}

/// Blade 3 — the face's receiver-guard pair `(class_tag, twin)`.
/// A `class_tag` of 0 disarms the guard (runtime-define mint sites
/// carry no tag), and so does every cell that is not a reified
/// class-METHOD face.
///
/// The sentinel check is load-bearing, not defensive: an ACCESSOR
/// face's cell keeps its §17 name at `ACC_CELL_NAME_OFF` and its ES
/// length at `ACC_CELL_LEN_OFF` — the very words a METHOD face uses
/// for `(class_tag, twin)`. Read blind, every setter face (length 1)
/// answered `twin == 1`, and the two dynobj method-call arms below
/// duly `blr`'d to address 1: `Object.getOwnPropertyDescriptor(
/// C.prototype, "s").set()` was a SIGBUS. Getters (length 0) read as
/// "no twin" and took the correct mono path all along, which is why
/// only half the pair ever crashed.
///
/// # Safety
/// `ptr` points at a live `Tag::Closure` heap cell.
pub(crate) unsafe fn class_method_face_guard(ptr: *mut c_void) -> (u64, u64) {
    unsafe {
        if *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64)
            != class_bare_entry as *const () as u64
        {
            return (0, 0);
        }
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

/// The §10.2.9 name a reified class-method face carries when its
/// key was computed at runtime (564-01), `None` when the face's
/// name is the source spelling the fn-name registry holds. The Str
/// cell is the face's own immortal stake — callers rc_inc before
/// handing it out, exactly like [`class_accessor_meta`]'s.
///
/// # Safety
/// `ptr` points at a live `Tag::Closure` heap cell.
pub(crate) unsafe fn class_method_runtime_name(ptr: *mut c_void) -> Option<*mut u8> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry != class_bare_entry as *const () as u64 {
            return None;
        }
        let name = *(ptr.cast::<u8>().add(CLS_CELL_NAME_OFF) as *const u64) as *mut u8;
        (!name.is_null()).then_some(name)
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

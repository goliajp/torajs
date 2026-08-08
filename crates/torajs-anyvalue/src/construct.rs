//! S-NEW 刀 2 — `new <expr>(args…)`, where the constructor is a value
//! rather than a name.
//!
//! A `new C(…)` whose target is spelled out never reaches here: the
//! desugar binds it to the synthesized `__new_<C>` factory at compile
//! time and calls that directly. This is the other half — the callee
//! arrived as an `any` and only the running program knows what it is.
//!
//! Two pieces:
//!
//! - a registry filled once per class at module init, mapping the
//!   `__class_<C>` singleton to the boxed adapter of its `__new_<C>`
//!   factory. The adapter is the same `(env, argv, argc) -> AnyValue`
//!   dual entry every any-world call already dispatches through
//!   ([`crate::method_call_closure_dispatch::invoke_boxed`]); no new
//!   calling convention is introduced.
//! - [`__torajs_anyv_construct`], which checks IsConstructor and then
//!   calls that adapter.
//!
//! The registry is an open-addressed table keyed on the class cell
//! pointer, not an array indexed by class tag. Going the other way —
//! from a class value back to its tag — would mean scanning the tag
//! table on every construct, and `new` inside a loop cannot afford a
//! scan when the object it allocates costs tens of nanoseconds.
//! Slots are `AtomicU64` per the multi-thread-ready substrate rule
//! (design principles §6.2), which also matches how torajs-throw
//! holds its native-error factories.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{FLAG_FN_PROTO, HeapHeader, Tag};

use crate::method_call_closure_dispatch::{invoke_boxed, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_object_create_link_proto(obj: *mut c_void, proto: u64);
    fn __torajs_throw_check() -> i64;
}

/// Mirror of `method_value.rs`'s cell layout constant — the boxed
/// dual-entry slot of a closure cell.
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;

/// Power of two, and comfortably above the 256 class tags
/// `torajs-meta` allows, so the table never fills and probe chains
/// stay at one or two steps.
const CTOR_SLOTS: usize = 512;
const CTOR_MASK: u64 = CTOR_SLOTS as u64 - 1;

/// Class cell pointer bits; 0 means the slot was never claimed.
static CTOR_KEY: [AtomicU64; CTOR_SLOTS] = [const { AtomicU64::new(0) }; CTOR_SLOTS];
/// Address of that class's boxed factory adapter.
static CTOR_ENTRY: [AtomicU64; CTOR_SLOTS] = [const { AtomicU64::new(0) }; CTOR_SLOTS];
/// 1 when the class inherits `Array[@@species]`'s default getter
/// (§23.1.2.5 — an `extends Array` class object with no own
/// `@@species` answers ITSELF to the species read). Same slot index
/// as `CTOR_KEY`.
static CTOR_ARR_SPECIES: [AtomicU64; CTOR_SLOTS] = [const { AtomicU64::new(0) }; CTOR_SLOTS];

/// MurmurHash3 finalizer, same mixer `fnprops` uses on fn pointers.
/// Heap pointers are 8-aligned, so the low bits alone would collide
/// every eighth class.
#[inline]
fn mix(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    k ^ (k >> 33)
}

/// Record the boxed factory adapter for a class object.
///
/// Emitted by the lowerer right after `__torajs_anyv_class_register`,
/// with the same class value that call received — so the pointer is
/// the module-scope singleton and stays valid for the process. The
/// table holds the bits only; it takes no reference and releases
/// none.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_ctor_register(class_av: AnyValue, entry: *mut c_void) {
    if !is_cell(class_av) || entry.is_null() {
        return;
    }
    let key = as_void_ptr(class_av) as u64;
    let mut i = (mix(key) & CTOR_MASK) as usize;
    for _ in 0..CTOR_SLOTS {
        let held = CTOR_KEY[i].load(Ordering::Relaxed);
        if held == 0 || held == key {
            CTOR_KEY[i].store(key, Ordering::Relaxed);
            CTOR_ENTRY[i].store(entry as u64, Ordering::Relaxed);
            return;
        }
        i = (i + 1) & (CTOR_SLOTS - 1);
    }
}

/// Mark a class object as inheriting `Array[@@species]`'s default
/// getter (§23.1.2.5) — emitted at module init for `extends Array`
/// classes, right beside the factory-adapter registration. The
/// species read answers the class ITSELF where a plain object's
/// absent `@@species` defaults (a fn constructor's prototype chain
/// carries no species getter, so plain fns never mark).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_ctor_mark_arr_species(class_av: AnyValue) {
    if !is_cell(class_av) {
        return;
    }
    let key = as_void_ptr(class_av) as u64;
    let mut i = (mix(key) & CTOR_MASK) as usize;
    for _ in 0..CTOR_SLOTS {
        let held = CTOR_KEY[i].load(Ordering::Relaxed);
        if held == 0 || held == key {
            CTOR_KEY[i].store(key, Ordering::Relaxed);
            CTOR_ARR_SPECIES[i].store(1, Ordering::Relaxed);
            return;
        }
        i = (i + 1) & (CTOR_SLOTS - 1);
    }
}

/// Does this class object carry the inherited-`@@species` mark?
pub(crate) fn ctor_arr_species_self(key: u64) -> bool {
    let mut i = (mix(key) & CTOR_MASK) as usize;
    for _ in 0..CTOR_SLOTS {
        let held = CTOR_KEY[i].load(Ordering::Relaxed);
        if held == key {
            return CTOR_ARR_SPECIES[i].load(Ordering::Relaxed) != 0;
        }
        if held == 0 {
            return false;
        }
        i = (i + 1) & (CTOR_SLOTS - 1);
    }
    false
}

/// Look up a class object's factory adapter; 0 if it has none.
pub(crate) fn ctor_entry(key: u64) -> u64 {
    let mut i = (mix(key) & CTOR_MASK) as usize;
    for _ in 0..CTOR_SLOTS {
        let held = CTOR_KEY[i].load(Ordering::Relaxed);
        if held == key {
            return CTOR_ENTRY[i].load(Ordering::Relaxed);
        }
        if held == 0 {
            return 0;
        }
        i = (i + 1) & (CTOR_SLOTS - 1);
    }
    0
}

/// ES §7.2.4 IsConstructor, over the shapes tr can answer for today.
///
/// A class object is a dynobj carrying `FLAG_DYNOBJ_CLASS_CTOR` — the
/// same bit that makes `typeof C` answer `"function"`. An ordinary
/// function reaches any-world as a closure cell, and the compiler
/// stamps `FLAG_FN_PROTO` on exactly the declaration forms that own a
/// `.prototype` — which per §10.2.4 MakeConstructor are exactly the
/// forms with [[Construct]] (arrows and methods carry neither), so
/// that one bit answers both questions.
///
/// # Safety
/// `av` must be a live AnyValue.
pub(crate) unsafe fn is_constructor(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    let ptr = as_void_ptr(av);
    if ptr.is_null() {
        return false;
    }
    // SAFETY: a cell-encoded AnyValue points at a live heap block,
    // whose header is the first `HeapHeader` bytes.
    let hdr = unsafe { &*(ptr as *const HeapHeader) };
    (hdr.type_tag == Tag::DynObj as u16 && hdr.flags & torajs_rc::FLAG_DYNOBJ_CLASS_CTOR != 0)
        || (hdr.type_tag == Tag::Closure as u16 && hdr.flags & FLAG_FN_PROTO != 0)
}

/// ES §7.2.4 IsConstructor as a value-level predicate.
///
/// Exposed because test262's `isConstructor.js` needs exactly this
/// question answered without constructing anything — the stock harness
/// gets it from `Reflect.construct(function(){}, [], f)`, whose only
/// purpose is to probe `f`'s [[Construct]] without running `f`.
///
/// # Safety
/// `v` must be a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_is_constructor(v: AnyValue) -> bool {
    unsafe { is_constructor(v) }
}

/// `new callee(argv[0..argc])` where `callee` is a runtime value.
///
/// Answers the constructed object, owned by the caller. A callee that
/// is not a constructor raises a TypeError and answers undefined, per
/// §13.3.5.1 step 7.
///
/// # Safety
/// `argv` must point at `argc` readable AnyValue slots, borrowed for
/// the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_construct(
    callee: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    if !unsafe { is_constructor(callee) } {
        unsafe {
            __torajs_throw_type_error(c"value is not a constructor".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    // A plain function constructs per §10.2.2 (base kind): fresh
    // `this` from its `.prototype`, then the ordinary call channel.
    let cell = as_void_ptr(callee);
    // SAFETY: is_constructor above proved a live cell header.
    let hdr = unsafe { &*(cell as *const HeapHeader) };
    if hdr.type_tag == Tag::Closure as u16 {
        return unsafe { construct_plain_fn(cell, argv, argc) };
    }
    let entry = ctor_entry(as_void_ptr(callee) as u64);
    if entry == 0 {
        // The class is real but its factory has no boxed adapter —
        // an argument or return type outside the boxable set. Say so
        // rather than let the miss read as "not a constructor",
        // which would be a false statement about the program.
        unsafe {
            __torajs_throw_type_error(
                c"this constructor cannot be reached through a runtime value yet".as_ptr(),
            );
        }
        return VALUE_UNDEFINED;
    }
    unsafe { invoke_boxed(core::ptr::null_mut(), entry, argv, argc) }
}

/// §10.2.2 [[Construct]], base kind, over a plain-fn closure cell:
/// `this` is OrdinaryCreateFromConstructor(callee, "%Object.prototype%")
/// — a fresh dynobj linked to the callee's `.prototype` (minted on
/// first touch, `constructor` back-ref included) — the body runs
/// through the same receiver-routing dispatcher every any-world call
/// uses, and a non-object completion answers the fresh `this` (step
/// 10.b). `new.target` inside the body is a recorded approximation:
/// tr lowers it to undefined outside class ctors.
///
/// # Safety
/// `cell` is a live closure cell carrying `FLAG_FN_PROTO`; `argv`
/// points at `argc` readable AnyValue slots.
unsafe fn construct_plain_fn(cell: *mut c_void, argv: *const u64, argc: i64) -> AnyValue {
    unsafe {
        let Some((_ptag, pval)) = crate::closure_proto::fn_prototype_pair(cell) else {
            // Unreachable behind the is_constructor gate, but a loud
            // answer beats UB if the gate ever drifts.
            __torajs_throw_type_error(c"value is not a constructor".as_ptr());
            return VALUE_UNDEFINED;
        };
        let this_obj = __torajs_dynobj_alloc();
        __torajs_object_create_link_proto(this_obj, __torajs_anyv_box_pointer(pval as *mut c_void));
        let this_av = __torajs_anyv_box_pointer(this_obj);
        let entry = *((cell as *const u8).add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        let result = invoke_with_this(cell, entry, this_av, argv, argc);
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(this_av);
            return VALUE_UNDEFINED;
        }
        if is_heap_object(result) {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(this_av);
            return result;
        }
        if is_cell(result) && !as_void_ptr(result).is_null() {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(result);
        }
        this_av
    }
}

/// Is this completion value an Object in the spec sense — a heap cell
/// that is not one of the heap-shaped primitives (Str / Symbol /
/// BigInt)?
unsafe fn is_heap_object(av: AnyValue) -> bool {
    if !is_cell(av) {
        return false;
    }
    let ptr = as_void_ptr(av);
    if ptr.is_null() {
        return false;
    }
    // SAFETY: a cell-encoded AnyValue points at a live heap header.
    let hdr = unsafe { &*(ptr as *const HeapHeader) };
    hdr.type_tag != Tag::Str as u16
        && hdr.type_tag != Tag::Symbol as u16
        && hdr.type_tag != Tag::BigInt as u16
}

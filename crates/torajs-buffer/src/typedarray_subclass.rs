//! TypedArray-subclass instance allocation + `super(...)` semantics
//! (RFC 20260730-exotic-backed-class-instance, buffer-family blade).
//!
//! `class C extends Uint8Array` (any of the eleven §23.2 kinds) mints
//! a REAL TypedArray cell — the whole integer-indexed surface
//! (element reads/writes, `length`, the prototype methods, species)
//! rides the existing arms because the instance IS a typed array.
//! Class identity rides blade 0 (`FLAG_SUBCLASSED` + torajs-meta side
//! table), scrubbed by `__torajs_typedarray_drop`.
//!
//! The mint runs BEFORE the ctor body and answers the kind's
//! zero-length view over a fresh private buffer (§23.2.5.1's
//! no-argument answer). `super(...)` then applies the FULL §23.2.5.1
//! constructor semantics to the already-minted cell: the three
//! borrowed slots run through `__torajs_typedarray_create` (length /
//! buffer+offset+length / typed-array / iterable / array-like — every
//! form, including its coercion order), and the product's innards
//! TRANSPLANT into the mint. Exact, because this-TDZ means nothing
//! observed the default: the product cell never escapes, its buffer
//! reference moves into the mint, and the mint's private zero-buffer
//! is released.
//!
//! `super(...)` answers a fresh OWNED reference on the instance (the
//! wrapper-kernel contract: `super(v)` sits in statement position and
//! the lowerer releases the discarded any value). A pending throw
//! from the constructor semantics answers the borrowed box un-inc'd
//! for the caller's throw-check to divert past.

use core::ffi::c_void;

use torajs_anyvalue::nanbox::{AnyValue, as_void_ptr};
use torajs_rc::FLAG_SUBCLASSED;

use crate::typedarray::{ARRAY_LEN_OFF, BUFFER_OFF, BYTE_OFFSET_OFF, CELL_SIZE, Kind, kind_of};
use crate::typedarray_ctor::{__torajs_typedarray_create, create_same_type};

unsafe extern "C" {
    /// torajs-meta — record the fresh instance's class identity
    /// (blade 0). Takes no reference on the proto cell.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    /// torajs-meta classmeta — the class's registered `__proto_<C>`
    /// AnyValue immediate (0 when unregistered).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_rc_dec(v: AnyValue);
}

/// Mint a TypedArray-subclass instance: the `kind`'s zero-length
/// view (§23.2.5.1's no-argument default), flagged + registered.
/// `kind` is the [`Kind`] discriminant the lowering resolved from
/// the `extends` parent's name.
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_subclass_alloc(class_tag: i64, kind: i64) -> AnyValue {
    unsafe {
        // Zero elements: the byte count is 0 for every kind, so the
        // allocation cannot fail and no throw-check is needed.
        let v = create_same_type(Kind::from_repr(kind as u8), 0);
        let p = as_void_ptr(v).cast::<u8>();
        *(p.add(6) as *mut u16) |= FLAG_SUBCLASSED;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(p as *mut c_void, class_tag, proto_cell);
        v
    }
}

/// `super(...)` inside a `class C extends <TypedArray>` ctor — the
/// builtin's [[Construct]] semantics (§23.2.5.1, every argument
/// form) applied to the already-minted subclass cell. The three
/// slots are BORROWED; a missing one is `undefined`, which is
/// argument-count-exact for this family (ToIndex(undefined) is 0 and
/// an undefined explicit length means "to the end of the buffer").
///
/// # Safety
/// `this_av` is the live minted subclass cell; the slots are live
/// borrowed AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_subclass_super(
    this_av: AnyValue,
    a0: AnyValue,
    a1: AnyValue,
    a2: AnyValue,
) -> AnyValue {
    unsafe {
        let tp = as_void_ptr(this_av).cast::<u8>();
        let kind = kind_of(tp as *mut c_void);
        let product = __torajs_typedarray_create(kind as i64, a0, a1, a2);
        if __torajs_throw_check() != 0 {
            return this_av;
        }
        // Transplant: the product cell never escaped (rc 1, no
        // expando props), so its buffer reference MOVES into the
        // mint and the shell frees without a drop walk.
        let pp = as_void_ptr(product).cast::<u8>();
        let old_buf = (tp.add(BUFFER_OFF) as *const u64).read();
        (tp.add(BUFFER_OFF) as *mut u64).write((pp.add(BUFFER_OFF) as *const u64).read());
        (tp.add(BYTE_OFFSET_OFF) as *mut i64).write((pp.add(BYTE_OFFSET_OFF) as *const i64).read());
        (tp.add(ARRAY_LEN_OFF) as *mut i64).write((pp.add(ARRAY_LEN_OFF) as *const i64).read());
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(pp, layout);
        // The mint's private zero-buffer — released only after the
        // new innards are in place.
        __torajs_anyv_rc_dec(old_buf);
        torajs_rc::__torajs_rc_inc(tp as *mut c_void);
        this_av
    }
}

//! `Iterator` global identity helpers (RFC 20260730-iterator-global
//! 刀 1).
//!
//! Two runtime faces the lowering emits against:
//!
//! - [`__torajs_proto_chain_builtin`] — the `__torajs_genfn_chain`
//!   shape generalized to any builtin-proto tag: chains a declared
//!   class's `__proto_<C>` dynobj to a builtin prototype singleton
//!   (writes the internal PROTO_SLOT_KEY entry the
//!   `get_proto_of_any` dynobj arm reads). Emitted after class
//!   registration for `class C extends Iterator {}` (the stripped
//!   SUBCLASSABLE_BUILTINS path — the instance stays an ordinary
//!   `Tag::Obj`, only the prototype chain gains the builtin link).
//!
//! - [`__torajs_instanceof_builtin_proto`] — §7.3.22
//!   OrdinaryHasInstance against a builtin prototype singleton: walk
//!   the operand's prototype chain and answer whether the singleton
//!   appears on it. This is how `v instanceof Iterator` must behave
//!   (the Iterator "class" has no per-instance heap tag — generator
//!   objects, MapIter/ArrIter cells and `extends Iterator` subclass
//!   instances all qualify purely through their chains).

use core::ffi::c_void;

use core::ffi::c_char;

use crate::reflect::{
    ANY_HEAP, PROTO_SLOT_ATTRS, PROTO_SLOT_KEY, VALUE_UNDEFINED_IMM, alloc_str_key, is_cell_imm,
};
use crate::reflect_proto::__torajs_anyv_get_proto_of_any;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_rc_dec(p: *mut c_void);
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// §27.1.3.1 — the Iterator constructor is abstract: `new Iterator()`
/// with Iterator itself as NewTarget throws a TypeError (only a
/// subclass's `super()` passes through, and tr's stripped-heritage
/// shape never routes that here). Records the pending throw and
/// answers undefined; the lowering's `emit_throw_check` propagates.
///
/// # Safety
/// No preconditions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iterator_ctor_throw() -> u64 {
    unsafe {
        __torajs_throw_type_error(c"Abstract class Iterator not directly constructable".as_ptr())
    };
    VALUE_UNDEFINED_IMM
}

/// Prototype-chain walk bound. Real chains are ≤ 5 links; the cap is
/// a termination guarantee for the walk (dynobj `__proto__` writes
/// can in principle build long chains; spec cycles are rejected at
/// set time, the cap makes the loop total regardless).
const MAX_PROTO_WALK: u32 = 64;

/// Chain `proto_anyv` (a class's `__proto_<C>` dynobj, passed as the
/// AnyValue its module-scope binding holds) to the builtin prototype
/// singleton of `proto_tag`. Non-cell input is a no-op (misordered
/// toolchain resilience, mirrors `__torajs_genfn_chain`).
///
/// # Safety
/// `proto_anyv` carries a valid AnyValue bit pattern; cell case must
/// point to a valid dynobj. `proto_tag` is a valid builtin-proto tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proto_chain_builtin(proto_anyv: u64, proto_tag: i64) -> u64 {
    if !is_cell_imm(proto_anyv) {
        return 0;
    }
    let target = unsafe { __torajs_get_builtin_prototype(proto_tag) };
    if target.is_null() {
        return 0;
    }
    // define consumes one rc of an ANY_HEAP value — the singleton is
    // immortal, the extra inc keeps the link alive by design
    // (mirrors genfn's leaked-singleton convention).
    unsafe { __torajs_rc_inc(target) };
    let mut slot = proto_anyv as *mut c_void;
    let k = unsafe { alloc_str_key(PROTO_SLOT_KEY) };
    unsafe {
        __torajs_dynobj_define_plain(
            &mut slot,
            k,
            ANY_HEAP as u64,
            target as u64,
            PROTO_SLOT_ATTRS,
        )
    };
    unsafe { __torajs_str_drop(k) };
    0
}

/// §7.3.22 OrdinaryHasInstance with the builtin prototype singleton
/// of `proto_tag` as the target: true iff the singleton appears on
/// `v`'s prototype chain.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_instanceof_builtin_proto(v: u64, proto_tag: i64) -> bool {
    let target = unsafe { __torajs_get_builtin_prototype(proto_tag) };
    if target.is_null() {
        return false;
    }
    // get_proto_of_any answers an owned AnyValue (rc_inc'd cell) or
    // a non-cell immediate (VALUE_NULL_IMM = chain end); every cell
    // link we step past gets its inc returned. The refs we drop are
    // the walk's own — chain members are alive through their owners
    // (class globals / singletons), so no dec here can reach zero.
    let mut cur = unsafe { __torajs_anyv_get_proto_of_any(v) };
    let mut depth = 0u32;
    while is_cell_imm(cur) && depth < MAX_PROTO_WALK {
        if cur as usize == target as usize {
            unsafe { __torajs_rc_dec(cur as *mut c_void) };
            return true;
        }
        let next = unsafe { __torajs_anyv_get_proto_of_any(cur) };
        unsafe { __torajs_rc_dec(cur as *mut c_void) };
        cur = next;
        depth += 1;
    }
    if is_cell_imm(cur) {
        unsafe { __torajs_rc_dec(cur as *mut c_void) };
    }
    false
}

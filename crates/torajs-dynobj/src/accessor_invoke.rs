//! Accessor-pair invoke paths — `__torajs_accessor_invoke_getter` /
//! `__torajs_accessor_invoke_setter` (split from `accessor.rs` by the
//! 500-line file discipline; RFC 20260718-accessor-reify 刀 1 grew the
//! parent past the cap with the builtin-face re-route).
//!
//! The pair's face closures span several ABI shapes — the packed
//! `kinds` word (see `accessor.rs`) selects the fn-pointer transmute;
//! an interned builtin-method face (probed by mid) re-routes through
//! the any-method dispatcher instead of its bare-receiver throw.

use core::ffi::c_void;

use crate::accessor::{
    ACC_GET_OFF, ACC_KIND_BOOL, ACC_KIND_BOXED, ACC_KIND_F64, ACC_KIND_I64, ACC_KIND_NAKED,
    ACC_KIND_PTR, ACC_KIND_RECV, ACC_KIND_VOID, ACC_KINDS_OFF, ACC_SET_OFF, CLOSURE_FN_ADDR_OFF,
    VALUE_UNDEFINED,
};

unsafe extern "C" {
    /// torajs-rc — the one boxed-entry reader (link-judged; see
    /// `torajs_rc::closure_entry`).
    fn __torajs_closure_boxed_entry(cell: *const u8) -> u64;
    /// torajs-anyvalue — NaN-box a `(tag, value)` pair into an
    /// `AnyValue`. `tag`: 0=Null 1=Bool 2=I64 3=F64(bits) 4=Heap(ptr).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-anyvalue — extract the payload (per-tag value) from a
    /// NaN-box `AnyValue` (F64 returns the bits, Heap the pointer).
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-anyvalue — the interned builtin-method mid a face
    /// carries, `-1` for ordinary closures (RFC
    /// 20260718-accessor-reify 刀 1).
    fn __torajs_builtin_method_face_mid(p: *const c_void) -> i64;
    /// torajs-anyvalue — invoke a builtin mid against a receiver
    /// (the pair-invoke twin of the `.call` re-dispatch).
    fn __torajs_builtin_method_face_dispatch(
        recv: u64,
        mid: i64,
        argv: *const u64,
        argc: i64,
    ) -> u64;
    /// torajs-anyvalue — the adapter a reified class face carries
    /// (`0` = not a class face; RFC 20260718-accessor-reify 刀 2).
    fn __torajs_class_face_adapter(p: *const c_void) -> u64;
    /// torajs-anyvalue — invoke a class-face adapter with the
    /// receiver in the env slot.
    fn __torajs_class_face_invoke(adapter: u64, recv: u64, argv: *const u64, argc: i64) -> u64;
}

/// `__torajs_accessor_invoke_getter(pair, recv_anyv)` — call the
/// getter closure and return its result as an `AnyValue`. The getter
/// is invoked with the env-first closure ABI and **no** user argument
/// (a getter takes none). Its raw return lands in the register bank
/// dictated by its native SSA return type, so the packed getter ret
/// kind selects the matching fn-pointer transmute (an `F64` getter
/// leaves its value in `d0`, an `I64` one in `x0`) and the matching
/// box. A pair with no getter yields `undefined`.
///
/// `this` binding: `recv_anyv` is the NaN-boxed property-read
/// receiver, BORROWED into the call. An [`ACC_KIND_RECV`] face (a
/// fn-expr accessor whose body says `this`) rides the boxed dual
/// entry with the receiver in argv[0]; every other face ignores it
/// (the closure-capture accessor idiom carries no `this`).
///
/// # Safety
/// `pair` is null or a live `AccessorPair`; its `get_closure` (when
/// present) is a live closure whose fn at `+8` has the env-first ABI
/// and the return type encoded by the getter ret kind; `recv_anyv` is
/// a live receiver or `undefined`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_invoke_getter(
    pair: *const c_void,
    recv_anyv: u64,
) -> u64 {
    if pair.is_null() {
        return VALUE_UNDEFINED;
    }
    let getter = unsafe { *((pair as *const u8).add(ACC_GET_OFF) as *const u64) } as *mut c_void;
    if getter.is_null() {
        return VALUE_UNDEFINED;
    }
    // A builtin-method face (an interned mid cell — e.g. the Annex B
    // `get __proto__`) carries the bare-receiver throw as its boxed
    // dual entry; re-route through the mid dispatcher instead.
    let mid = unsafe { __torajs_builtin_method_face_mid(getter) };
    if mid >= 0 {
        return unsafe {
            __torajs_builtin_method_face_dispatch(recv_anyv, mid, core::ptr::null(), 0)
        };
    }
    // A reified class-accessor face (RFC 20260718-accessor-reify
    // 刀 2) — invoke its boxed adapter with the receiver in the env
    // slot (the class-method dispatch shape).
    let cls_adapter = unsafe { __torajs_class_face_adapter(getter) };
    if cls_adapter != 0 {
        return unsafe { __torajs_class_face_invoke(cls_adapter, recv_anyv, core::ptr::null(), 0) };
    }
    let kinds = unsafe { *((pair as *const u8).add(ACC_KINDS_OFF) as *const u64) };
    let raw_ret = (kinds & 0xff) as u8;
    if raw_ret & ACC_KIND_RECV != 0 || unsafe { crate::accessor::closure_recv_first(getter) } {
        // Receiver-first fn-expr face — boxed dual entry, argv[0] is
        // the receiver the body reads as `this`. The header-flag OR
        // covers routes that never stamped the kind byte (alias /
        // any-boxed faces, knife 2W).
        let entry = unsafe { __torajs_closure_boxed_entry(getter as *const u8) };
        if entry == 0 {
            return VALUE_UNDEFINED;
        }
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            unsafe { core::mem::transmute(entry as usize) };
        let mut buf = [VALUE_UNDEFINED; 8];
        buf[0] = recv_anyv;
        return unsafe { call(getter, buf.as_ptr(), 1) };
    }
    let ret_kind = raw_ret & !ACC_KIND_NAKED;
    // A NAKED getter's env argument is spurious either way (the fn
    // reads no params) — the env-first transmutes below stay correct
    // for both shapes, so the flag only matters for the setter.
    if ret_kind == ACC_KIND_BOXED {
        let entry = unsafe { __torajs_closure_boxed_entry(getter as *const u8) };
        if entry == 0 {
            return VALUE_UNDEFINED;
        }
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            unsafe { core::mem::transmute(entry as usize) };
        // Zero-arg call still hands the adapter a live argv buffer —
        // it reads its declared param slots unconditionally.
        let buf = [VALUE_UNDEFINED; 8];
        return unsafe { call(getter, buf.as_ptr(), 0) };
    }
    let fn_addr = unsafe { *((getter as *const u8).add(CLOSURE_FN_ADDR_OFF) as *const usize) };
    // S1 (RFC 20260810-indirect-argc-abi) — env-first entries carry
    // the hidden i64 argc after the env; a getter passes 0 user
    // arguments. A NAKED getter reads neither register, so the shape
    // stays correct for both (see the note above).
    unsafe {
        match ret_kind {
            ACC_KIND_F64 => {
                let f: unsafe extern "C" fn(*mut c_void, i64) -> f64 =
                    core::mem::transmute(fn_addr);
                __torajs_anyv_box_from_pair(3, f(getter, 0).to_bits() as i64)
            }
            ACC_KIND_BOOL => {
                let f: unsafe extern "C" fn(*mut c_void, i64) -> i64 =
                    core::mem::transmute(fn_addr);
                __torajs_anyv_box_from_pair(1, i64::from(f(getter, 0) != 0))
            }
            ACC_KIND_I64 => {
                let f: unsafe extern "C" fn(*mut c_void, i64) -> i64 =
                    core::mem::transmute(fn_addr);
                __torajs_anyv_box_from_pair(2, f(getter, 0))
            }
            ACC_KIND_PTR => {
                let f: unsafe extern "C" fn(*mut c_void, i64) -> u64 =
                    core::mem::transmute(fn_addr);
                __torajs_anyv_box_from_pair(4, f(getter, 0) as i64)
            }
            ACC_KIND_VOID => {
                let f: unsafe extern "C" fn(*mut c_void, i64) = core::mem::transmute(fn_addr);
                f(getter, 0);
                VALUE_UNDEFINED
            }
            // ACC_KIND_ANY (and any unknown): the getter already
            // returns a NaN-box AnyValue — pass it through verbatim.
            _ => {
                let f: unsafe extern "C" fn(*mut c_void, i64) -> u64 =
                    core::mem::transmute(fn_addr);
                f(getter, 0)
            }
        }
    }
}

/// `__torajs_accessor_invoke_setter(pair, recv_anyv, value_anyv)` —
/// call the setter closure with the assigned value, returning `1` when
/// a setter ran and `0` when the accessor has no setter (the caller
/// raises the "only a getter" assignment error). The setter is invoked
/// env-first with the value unboxed per the stored setter param kind
/// (an `F64` setter takes its argument in `d0`, an `I64` one in `x0`);
/// an `Any` setter receives the NaN-box `AnyValue` verbatim.
///
/// `this` binding: `recv_anyv` is the NaN-boxed assignment receiver,
/// BORROWED into the call. An [`ACC_KIND_RECV`] face rides the boxed
/// dual entry with the receiver in argv[0] and the value at argv[1];
/// every other face ignores it.
///
/// # Safety
/// `pair` is null or a live `AccessorPair`; its `set_closure` (when
/// present) is a live closure whose fn at `+8` has the env-first ABI
/// and the param type encoded by the setter param kind; `recv_anyv` is
/// a live receiver or `undefined`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_invoke_setter(
    pair: *const c_void,
    recv_anyv: u64,
    value_anyv: u64,
) -> i32 {
    if pair.is_null() {
        return 0;
    }
    let setter = unsafe { *((pair as *const u8).add(ACC_SET_OFF) as *const u64) } as *mut c_void;
    if setter.is_null() {
        return 0;
    }
    // Builtin-method face — see the getter twin above.
    let mid = unsafe { __torajs_builtin_method_face_mid(setter) };
    if mid >= 0 {
        let buf = [value_anyv];
        unsafe { __torajs_builtin_method_face_dispatch(recv_anyv, mid, buf.as_ptr(), 1) };
        return 1;
    }
    // Class-accessor face — see the getter twin above.
    let cls_adapter = unsafe { __torajs_class_face_adapter(setter) };
    if cls_adapter != 0 {
        let buf = [value_anyv];
        unsafe { __torajs_class_face_invoke(cls_adapter, recv_anyv, buf.as_ptr(), 1) };
        return 1;
    }
    let kinds = unsafe { *((pair as *const u8).add(ACC_KINDS_OFF) as *const u64) };
    let raw_kind = ((kinds >> 8) & 0xff) as u8;
    if raw_kind & ACC_KIND_RECV != 0 || unsafe { crate::accessor::closure_recv_first(setter) } {
        // Receiver-first fn-expr face — boxed dual entry, argv[0] is
        // the receiver the body reads as `this`, argv[1] the value.
        // Header-flag OR: see the getter twin.
        let entry = unsafe { __torajs_closure_boxed_entry(setter as *const u8) };
        if entry == 0 {
            return 1;
        }
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            unsafe { core::mem::transmute(entry as usize) };
        let mut buf = [VALUE_UNDEFINED; 8];
        buf[0] = recv_anyv;
        buf[1] = value_anyv;
        unsafe { call(setter, buf.as_ptr(), 2) };
        return 1;
    }
    let naked = raw_kind & ACC_KIND_NAKED != 0;
    let param_kind = raw_kind & !ACC_KIND_NAKED;
    if naked {
        // Named top-level fn — no leading env param; the value is
        // its FIRST argument.
        let fn_addr = unsafe { *((setter as *const u8).add(CLOSURE_FN_ADDR_OFF) as *const usize) };
        unsafe {
            match param_kind {
                ACC_KIND_F64 => {
                    let v = f64::from_bits(__torajs_anyv_unbox_value(value_anyv) as u64);
                    let f: unsafe extern "C" fn(f64) = core::mem::transmute(fn_addr);
                    f(v);
                }
                ACC_KIND_BOOL | ACC_KIND_I64 => {
                    let f: unsafe extern "C" fn(i64) = core::mem::transmute(fn_addr);
                    f(__torajs_anyv_unbox_value(value_anyv));
                }
                ACC_KIND_PTR => {
                    let f: unsafe extern "C" fn(*mut c_void) = core::mem::transmute(fn_addr);
                    f(__torajs_anyv_unbox_value(value_anyv) as *mut c_void);
                }
                _ => {
                    let f: unsafe extern "C" fn(u64) = core::mem::transmute(fn_addr);
                    f(value_anyv);
                }
            }
        }
        return 1;
    }
    if param_kind == ACC_KIND_BOXED {
        let entry = unsafe { __torajs_closure_boxed_entry(setter as *const u8) };
        if entry == 0 {
            return 1;
        }
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            unsafe { core::mem::transmute(entry as usize) };
        let mut buf = [VALUE_UNDEFINED; 8];
        buf[0] = value_anyv;
        unsafe { call(setter, buf.as_ptr(), 1) };
        return 1;
    }
    let fn_addr = unsafe { *((setter as *const u8).add(CLOSURE_FN_ADDR_OFF) as *const usize) };
    // S1 — env-first setter takes the hidden i64 argc after the env
    // (one user argument: the assigned value).
    unsafe {
        match param_kind {
            ACC_KIND_F64 => {
                let v = f64::from_bits(__torajs_anyv_unbox_value(value_anyv) as u64);
                let f: unsafe extern "C" fn(*mut c_void, i64, f64) = core::mem::transmute(fn_addr);
                f(setter, 1, v);
            }
            ACC_KIND_BOOL | ACC_KIND_I64 => {
                let v = __torajs_anyv_unbox_value(value_anyv);
                let f: unsafe extern "C" fn(*mut c_void, i64, i64) = core::mem::transmute(fn_addr);
                f(setter, 1, v);
            }
            ACC_KIND_PTR => {
                let v = __torajs_anyv_unbox_value(value_anyv) as *mut c_void;
                let f: unsafe extern "C" fn(*mut c_void, i64, *mut c_void) =
                    core::mem::transmute(fn_addr);
                f(setter, 1, v);
            }
            // ACC_KIND_ANY (and any unknown): the setter takes an
            // AnyValue — pass the NaN-box through verbatim.
            _ => {
                let f: unsafe extern "C" fn(*mut c_void, i64, u64) = core::mem::transmute(fn_addr);
                f(setter, 1, value_anyv);
            }
        }
    }
    1
}

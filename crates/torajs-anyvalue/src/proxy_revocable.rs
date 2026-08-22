//! `Proxy.revocable(target, handler)` — §28.2.2.1
//! (RFC 20260823-proxy-substrate 刀 3).
//!
//! Answers `{ proxy, revoke }`. The revoke function is a native
//! closure cell in the [`crate::promise_with_resolvers`] shape: a
//! `Tag::Closure` with a boxed entry, one capture slot, and the
//! §20.2.3 `name` / `length` reflection entries pre-seeded.
//!
//! §10.5.4.1 says revocation stores **null** into `[[ProxyTarget]]`
//! and `[[ProxyHandler]]`, and §28.2.2.1.1 says the revoker drops
//! its own `[[RevocableProxy]]` to null too — so calling `revoke()`
//! twice is a no-op by construction, without a flag anywhere. Both
//! of those are literal here: the capture slot is cleared and the
//! proxy's two slots are nulled, and every internal method already
//! reads `is_null(handler)` as "revoked".

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, VALUE_NULL, VALUE_UNDEFINED, as_void_ptr};
use crate::nanbox_encode::__torajs_anyv_box_pointer;
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::proxy::{HANDLER_OFF, TARGET_OFF};

// ---- closure cell layout (ssa_lower closure-env mirror) ----
const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const CLOSURE_TRACE_FN_OFF: usize = 40;
/// `[[RevocableProxy]]` — the proxy cell the revoker holds (+1),
/// nulled by the first call.
const REVOKE_PROXY_OFF: usize = 48;
const CELL_SIZE: usize = 56;

/// dynobj bucket tag for a heap payload (torajs-dynobj layout).
const ANY_HEAP: u64 = 4;
const ANY_I64: u64 = 2;
/// §20.2.3 fn `name` / `length` entry flags — mirror of
/// `promise_with_resolvers::REFLECT_ENTRY_FLAGS`.
const REFLECT_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags: u64,
    );
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
    /// torajs-value-drop — universal NaN-box-safe heap release.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — drop the block out of the root buffer first.
    fn __torajs_cycle_unbuffer(p: *mut c_void);
}

/// §28.2.2.1. `target` / `handler` are borrowed; the result is an
/// owned dynobj boxed as `any`. A rejected argument leaves the
/// §10.5.14 TypeError pending and answers undefined.
///
/// # Safety
/// Both arguments are valid AnyValues alive across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proxy_revocable(target: AnyValue, handler: AnyValue) -> AnyValue {
    unsafe {
        let p = crate::proxy::__torajs_proxy_create(target, handler);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let cell = as_void_ptr(p);
        let revoke = mint_revoker(cell);
        let mut obj = __torajs_dynobj_alloc();
        store(&mut obj, b"proxy", p);
        store(
            &mut obj,
            b"revoke",
            __torajs_anyv_box_pointer(revoke as *mut c_void),
        );
        obj as u64
    }
}

/// One `{ key: heap-value }` entry; the value's reference transfers.
unsafe fn store(obj: &mut *mut c_void, key: &[u8], value: AnyValue) {
    unsafe {
        let k = __torajs_str_alloc(key.as_ptr(), key.len() as i64);
        __torajs_dynobj_set(obj, k as *mut c_void, ANY_HEAP, value);
        __torajs_str_drop(k as *mut c_void);
    }
}

/// The revoker cell — holds the proxy at [`REVOKE_PROXY_OFF`] with
/// its own `+1`, so revoking a proxy nobody else still names is
/// still well-defined.
unsafe fn mint_revoker(proxy_cell: *mut c_void) -> *mut u8 {
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) =
            crate::method_value::native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = revoker_drop as *const () as u64;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = revoke_entry as *const () as u64;
        *(cell.add(CLOSURE_TRACE_FN_OFF) as *mut u64) = revoker_trace as *const () as u64;
        torajs_rc::__torajs_rc_inc(proxy_cell);
        *(cell.add(REVOKE_PROXY_OFF) as *mut u64) = proxy_cell as u64;

        let props_slot = cell.add(CLOSURE_PROPS_OFF) as *mut *mut c_void;
        *props_slot = __torajs_dynobj_alloc();
        let name_key = __torajs_str_alloc(c"name".as_ptr() as *const u8, 4);
        let empty = __torajs_str_alloc(c"".as_ptr() as *const u8, 0);
        __torajs_dynobj_define(
            props_slot,
            name_key as *mut c_void,
            ANY_HEAP,
            empty as u64,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_str_drop(name_key as *mut c_void);
        let len_key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
        __torajs_dynobj_define(
            props_slot,
            len_key as *mut c_void,
            ANY_I64,
            0,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_str_drop(len_key as *mut c_void);
        cell
    }
}

/// §28.2.2.1.1 — the revoker's [[Call]]. Ignores its arguments,
/// answers undefined, and is a no-op after the first call.
unsafe extern "C" fn revoke_entry(env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    unsafe {
        let slot = env.cast::<u8>().add(REVOKE_PROXY_OFF) as *mut u64;
        let held = *slot as *mut c_void;
        if held.is_null() {
            return VALUE_UNDEFINED;
        }
        *slot = 0;
        let p = held.cast::<u8>();
        let (t, h) = (
            (p.add(TARGET_OFF) as *const u64).read(),
            (p.add(HANDLER_OFF) as *const u64).read(),
        );
        *(p.add(TARGET_OFF) as *mut u64) = VALUE_NULL;
        *(p.add(HANDLER_OFF) as *mut u64) = VALUE_NULL;
        __torajs_anyv_rc_dec(t);
        __torajs_anyv_rc_dec(h);
        __torajs_value_drop_heap(held);
        VALUE_UNDEFINED
    }
}

/// drop_fn — release the props bag and the held proxy, then the
/// block itself (the `promise_with_resolvers::resolver_drop` shape).
unsafe extern "C" fn revoker_drop(env: *mut c_void) {
    unsafe {
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let props = *(cell.add(CLOSURE_PROPS_OFF) as *const u64);
        if props != 0 {
            __torajs_value_drop_heap(props as *mut c_void);
        }
        let held = *(cell.add(REVOKE_PROXY_OFF) as *const u64);
        if held != 0 {
            __torajs_value_drop_heap(held as *mut c_void);
        }
        std::alloc::dealloc(
            cell,
            core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap(),
        );
    }
}

/// trace_fn — the held proxy is the cell's one extra child.
unsafe extern "C" fn revoker_trace(
    env: *mut c_void,
    visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    ctx: *mut c_void,
) {
    unsafe {
        let slot = env.cast::<u8>().add(REVOKE_PROXY_OFF);
        visit(
            0,
            *(slot as *const u64) as *mut c_void,
            slot as *mut c_void,
            ctx,
        );
    }
}

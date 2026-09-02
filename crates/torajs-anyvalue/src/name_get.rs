//! `__torajs_any_name_get` — `recv.name` where the receiver is an
//! `any` value (chunk 716). OWNED return protocol, the exact mirror
//! of `index_any.rs::__torajs_any_length_get`: the returned box
//! holds its own reference, the caller's ownership ledger drops it.
//!
//! ssa-lower's member fallback routes the compile-time literal name
//! `"name"` here (third special case after `length` / `size`), so
//! the borrow-shaped member-get pair never sees the key. Dispatch:
//!
//! - `null` / `undefined` → catchable TypeError.
//! - `Tag::Closure` → the fn-metadata read, in own-property order:
//!   an expando `f.name = …` answers first (recorded divergence: ES
//!   makes `name` non-writable so bun ignores the write; tr's
//!   expando shadow wins); a reified builtin method cell answers
//!   its interned immortal name Str (chunk 715 metadata, rc no-op
//!   under the owned protocol); an ordinary closure walks the
//!   fn-addr registry — hit answers the row's immortal name cell,
//!   miss answers the ES anonymous-function name `""` (arrow / anon closures
//!   skip the registry; the binding-name NamedEvaluation face is a
//!   recorded follow-up).
//! - `Tag::DynObj` → own-property probe for the literal key
//!   `"name"` (a user `{ name: "x" }` answers its value; absence
//!   answers `undefined`).
//! - everything else (primitives, other heap tags) → `undefined`.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-str — fresh rc=1 Str from raw bytes.
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — key-presence probe (disambiguates a stored
    /// undefined from an absent entry).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-fnname — registry walk (chunk 716); the name's static Str
    /// cell, NULL = miss.
    fn __torajs_fn_name_lookup(fn_addr: u64, out_len: *mut u32, out_arity: *mut u32) -> *const u8;
    /// torajs-fnname — toString kernels (RFC 20260719 B5).
    fn __torajs_fn_source_str(fn_addr: u64) -> *mut u8;
    fn __torajs_fn_native_form_str(name_ptr: *const u8, name_len: u32) -> *mut u8;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Closure-cell layout mirror (`member_get.rs` / torajs-core
/// `ssa_lower.rs` constants).
const CLOSURE_PROPS_OFF: usize = 24;

/// Shared `""` cell — the ES anonymous-function name for registry
/// misses.
static EMPTY_NAME_CELL: AtomicU64 = AtomicU64::new(0);

/// The IMMORTAL name cell of a closure for the borrow-shaped
/// dynamic-key probe (`member_get`'s virtual pair, chunk D):
/// method cells reuse their chunk-715 interned name; registry fns
/// intern per fn_addr; a registry miss answers the shared `""`.
/// Bound cells answer `None` — their `"bound "` concat has no
/// stable intern key (recorded boundary; the static `.name` member
/// read covers them through the owned channel).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
pub(crate) unsafe fn closure_virtual_name_cell(ptr: *mut c_void) -> Option<*mut u8> {
    unsafe {
        if let Some(cell) = crate::method_value::builtin_method_name_cell_of(ptr) {
            return Some(cell);
        }
        // Reified class-accessor face (RFC 20260718-accessor-reify
        // 刀 2) — the carried "get <p>" / "set <p>" Str, owned out.
        if let Some((name, _)) = crate::method_value_class::class_accessor_meta(ptr) {
            torajs_rc::__torajs_rc_inc(name as *mut core::ffi::c_void);
            return Some(name);
        }
        // 564-01 — a COMPUTED member's name is its runtime key
        // (§10.2.9), carried by the face; the registry row under it
        // holds the compiler's `__ccm_<n>__` sentinel instead.
        if let Some(name) = crate::method_value_class::class_method_runtime_name(ptr) {
            torajs_rc::__torajs_rc_inc(name as *mut core::ffi::c_void);
            return Some(name);
        }
        if crate::method_bind::bound_cell_meta(ptr).is_some() {
            return None;
        }
        // B6c — a class-method face resolves its adapter's registry
        // row (the user-visible method name).
        let fn_addr = crate::method_value_class::registry_addr(ptr);
        let mut name_len: u32 = 0;
        let mut arity: u32 = 0;
        let name_ptr = __torajs_fn_name_lookup(fn_addr, &mut name_len, &mut arity);
        if name_ptr.is_null() {
            let p = EMPTY_NAME_CELL.load(Ordering::Relaxed);
            if p != 0 {
                return Some(p as *mut u8);
            }
            let cell = crate::method_value::mint_immortal_str(b"");
            EMPTY_NAME_CELL.store(cell as u64, Ordering::Relaxed);
            return Some(cell);
        }
        // The registry row points at the name's immortal static Str
        // cell — the borrow-shaped answer itself, no interning needed.
        Some(name_ptr as *mut u8)
    }
}

/// See module doc.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_name_get(recv: AnyValue) -> AnyValue {
    // Proxy twin of the `.length` arm — see `len_get.rs`.
    if crate::proxy::is_proxy(recv) {
        return unsafe { crate::proxy::get_named(recv, b"name") };
    }
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    unsafe {
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Closure as u16 {
            return closure_name(ptr);
        }
        if tag == Tag::DynObj as u16 {
            // Own-property probe for the literal key "name" — the
            // `__torajs_any_length_get` dynobj-arm shape.
            let key = __torajs_str_alloc(c"name".as_ptr() as *const u8, 4);
            let dtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
            let dval = __torajs_dynobj_get_value(ptr, key as *const c_void);
            let has_own = __torajs_dynobj_has(ptr, key as *const c_void);
            __torajs_str_drop(key as *mut c_void);
            if dtag != 5 || has_own != 0 {
                // Present entry (a stored undefined shadows the
                // chain). The probe pair is a borrow — the returned
                // box owns its own reference.
                crate::payload_rc_inc(dtag as i64, dval as i64);
                return crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag as i64, dval as i64);
            }
            // §10.1.8.1 OrdinaryGet — the user [[Prototype]] chain
            // answers before undefined (RFC 20260721 刀 5 R-F:
            // `Object.create(parent)` shapes inherit a defined
            // `name`). The chain root (implicit %Object.prototype%)
            // carries no `name` — plain undefined below.
            if let Some(parent) = crate::member_get_own::user_proto_cell(ptr) {
                return __torajs_any_name_get(crate::nanbox_encode::__torajs_anyv_box_from_pair(
                    4,
                    parent as i64,
                ));
            }
            // Function.prototype's virtual own `name` (§20.2.3,
            // RFC 20260722 刀 3) — immortal "" Str, ledger-free.
            let key = __torajs_str_alloc(c"name".as_ptr() as *const u8, 4);
            let meta =
                crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key as *const c_void);
            __torajs_str_drop(key as *mut c_void);
            if let Some((mtag, mval)) = meta {
                return crate::nanbox_encode::__torajs_anyv_box_from_pair(mtag as i64, mval as i64);
            }
            return VALUE_UNDEFINED;
        }
    }
    VALUE_UNDEFINED
}

/// The `Tag::Closure` arm — expando shadow, then method-cell
/// metadata, then the fn-addr registry.
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
unsafe fn closure_name(ptr: *mut c_void) -> AnyValue {
    unsafe {
        let props = *(ptr.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) as *const c_void;
        if !props.is_null() {
            let key = __torajs_str_alloc(c"name".as_ptr() as *const u8, 4);
            let dtag = __torajs_dynobj_get_tag(props, key as *const c_void);
            let dval = __torajs_dynobj_get_value(props, key as *const c_void);
            let present = dtag != 5 || __torajs_dynobj_has(props, key as *const c_void) != 0;
            __torajs_str_drop(key as *mut c_void);
            if present {
                // Shared with the `.length` face: a data pair boxes
                // owned, an accessor entry runs its getter (pre-fix
                // the raw accessor pair boxed as data — a
                // `defineProperty(f, "name", {get})` read answered
                // the pair, not the getter). A heap cell's pointer
                // bits ARE its box encoding, so the receiver
                // reconstructs from `ptr`.
                return crate::len_get::box_probe_pair(dtag, dval, ptr as u64);
            }
        }
        // chunk C — a tombstoned virtual `name` with no expando
        // recreate (probed above) continues along [[Prototype]]:
        // %Function.prototype% carries an own `name` "" (§20.2.3),
        // matching bun on `delete f.name; f.name`.
        if crate::member_get::header_flag(ptr, torajs_rc::FLAG_FN_NAME_DELETED) {
            let empty = __torajs_str_alloc(c"".as_ptr() as *const u8, 0);
            return crate::nanbox_encode::__torajs_anyv_box_from_pair(4, empty as i64);
        }
        let cell = closure_name_str(ptr);
        crate::nanbox_encode::__torajs_anyv_box_from_pair(4, cell as i64)
    }
}

/// `.name` read on a `Type::Closure` receiver (chunk 798 — the
/// typed-tier registry rewire). Thin extern face over
/// [`closure_name_str`] so the SSA member arm shares the full
/// metadata chain (builtin method cells, `bound `-prefixed bound
/// cells, the fn-addr registry). The Any-tier expando shadow is
/// deliberately NOT consulted: ES makes `name` non-writable, so on
/// the typed tier the registry answer matches bun exactly (the
/// Any-tier expando-first order stays the recorded divergence).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_name_str(ptr: *mut c_void) -> *mut u8 {
    unsafe { closure_name_str(ptr) }
}

/// `toString` source of a closure-tagged cell (RFC
/// 20260719-fn-tostring-source B5) — the typed-tier `String(c)` /
/// template lane twin of the any-lane method arm: a reified builtin
/// method cell mints the named JSC native form, everything else
/// routes the cell's fn_addr through the registry erased-source
/// kernel (bound wrappers land there on their `bound `-marked row
/// and answer the target-named native form).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_source_str(ptr: *mut c_void) -> *mut u8 {
    unsafe {
        if let Some(name) = crate::method_value::builtin_method_name(ptr) {
            return __torajs_fn_native_form_str(name.as_ptr(), name.len() as u32);
        }
        // B6c — a class-method face resolves its adapter's registry
        // row (the erased method-shorthand source).
        __torajs_fn_source_str(crate::method_value_class::registry_addr(ptr))
    }
}

/// The name Str cell of a closure-tagged cell, owned protocol:
/// method cells answer their interned immortal cell (drop no-ops on
/// the static flag), everything else answers a fresh rc=1 Str.
/// Recursive over bound-cell targets (chunk 719 — ES §20.2.3.2
/// SetFunctionName: `"bound " + targetName`, nesting concatenates
/// outside-in). An expando `name` shadow on a bound TARGET is not
/// consulted here (recorded edge; the receiving cell's own expando
/// shadows in `closure_name` before this runs).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
unsafe fn closure_name_str(ptr: *mut c_void) -> *mut u8 {
    unsafe {
        if let Some(cell) = crate::method_value::builtin_method_name_cell_of(ptr) {
            return cell;
        }
        // Class-accessor face — see the virtual-name twin above.
        if let Some((name, _)) = crate::method_value_class::class_accessor_meta(ptr) {
            torajs_rc::__torajs_rc_inc(name as *mut core::ffi::c_void);
            return name;
        }
        // 564-01 — see the virtual-name twin above.
        if let Some(name) = crate::method_value_class::class_method_runtime_name(ptr) {
            torajs_rc::__torajs_rc_inc(name as *mut core::ffi::c_void);
            return name;
        }
        if let Some((kind, target, _)) = crate::method_bind::bound_cell_meta(ptr) {
            let tname: *mut u8 = if kind == 0 {
                let (mid, _) = crate::method_bind::unpack_builtin_target(target);
                let name = torajs_rc::any_method_meta(mid).map_or("", |(n, _)| n);
                __torajs_str_alloc(name.as_ptr(), name.len() as i64)
            } else {
                closure_name_str(target as *mut c_void)
            };
            let prefix = __torajs_str_alloc(c"bound ".as_ptr() as *const u8, 6);
            let joined = crate::__torajs_str_concat(prefix as *const u8, tname as *const u8);
            __torajs_str_drop(prefix as *mut c_void);
            // Fresh target names drop; an interned method-name cell
            // no-ops on the static flag.
            __torajs_str_drop(tname as *mut c_void);
            return joined as *mut u8;
        }
        // B6c — a class-method face resolves its adapter's registry
        // row (the user-visible method name).
        let fn_addr = crate::method_value_class::registry_addr(ptr);
        let mut name_len: u32 = 0;
        let mut arity: u32 = 0;
        let name_ptr = __torajs_fn_name_lookup(fn_addr, &mut name_len, &mut arity);
        if name_ptr.is_null() {
            // Registry miss — the ES anonymous-function name is the
            // empty string (arrow / anon closures never register;
            // the binding-name NamedEvaluation face is a recorded
            // follow-up).
            return __torajs_str_alloc(c"".as_ptr() as *const u8, 0);
        }
        // The name's immortal static Str cell, owned out (its drop is
        // a no-op under the static flag).
        name_ptr as *mut u8
    }
}

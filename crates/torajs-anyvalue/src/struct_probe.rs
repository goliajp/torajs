//! The `Tag::Obj` own-property surface for the `any` lane: the
//! class-layout FIELD probe (chunk 744) and — RFC
//! 20260714-objlit-accessor blade 5 — the ACCESSOR [[Get]] behind it.
//! Carved out of `member_get.rs`, which owns the tag/value dispatch and
//! was over the 500-line file limit with both halves in it.
//!
//! Blades 1-4 taught the reflection surface (keys / gOPD / hasOwnProperty
//! / JSON) that an accessor is an own property, and the TYPED member
//! read has invoked getters since blade 2. The `any` lane still could
//! not: `(o as any).v` answered `undefined` and a class getter through
//! `any` did the same (probe at `cd0f3caf` vs bun's `2` / `999`).
//!
//! Two representations, one property. Both resolve from the plain name:
//!
//! * **object literal** — the getter closure is a layout FIELD named
//!   `__getter_v`. Its lifted body is `(__env, __this, ...)`, so the
//!   boxed dual entry the closure cell already carries takes the
//!   receiver as argv[0].
//! * **class** — the accessor is prototype-level and has no layout
//!   field. `__cm_<C>__v_get`'s boxed adapter rides in the class's
//!   dispatch table under the same `__getter_v` spelling (emit side:
//!   `ssa_lower_module_metadata::collect_own_class_methods`), and takes
//!   the receiver in the ENV slot with an empty argv — the shape
//!   `struct_method` already invokes plain methods with.
//!
//! **Called exactly once per read.** The `(tag, value)` probe pair is
//! two kernel calls, so it must not invoke anything: the tag channel
//! answers the [`ANY_ACCESSOR_TAG`] sentinel and the emitted accessor
//! arm (`ssa_lower_accessor::emit_any_get_result`) does the single
//! [[Get]] through [`__torajs_any_accessor_get`]. A getter with side
//! effects runs once, like ES §10.1.8 says.
//!
//! Ownership: the result is OWNED (the boxed adapter's return carries
//! its own ref), matching the dynobj accessor arm the emit joins with.
//! The receiver rides argv/env as a BORROW — the caller outlives the
//! call.

use core::ffi::c_void;

use torajs_rc::Tag;

mod field_read;
pub(crate) use field_read::{struct_field_pair, struct_field_pair_bytes};

use crate::method_call::invoke_boxed;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, box_void_ptr, is_cell};

unsafe extern "C" {
    /// torajs-structmeta — layout + accessor resolution.
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_accessor_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        kind: u8,
    ) -> u32;
    fn __torajs_struct_accessor_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        kind: u8,
    ) -> *const c_void;
    /// torajs-structmeta RFC 20260815 刀 5 — the flags-aware variant;
    /// writes the record's flags word on a hit so a twin-primary row
    /// (a GENERIC class's accessor) invokes recv-first.
    fn __torajs_struct_accessor_method_find_flags(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        kind: u8,
        out_flags: *mut u32,
    ) -> *const c_void;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    /// torajs-structmeta — does a name spell an accessor SLOT
    /// (`__getter_v`)? 255 = a plain property name.
    fn __torajs_accessor_name_kind(name: *const u8, name_len: u32) -> u8;
    /// torajs-dynobj — the AccessorPair lane (unchanged).
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Mirror of `torajs-structmeta::FieldInfo` (`member_get.rs` twin).
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// `class_tag` u32 offset inside a `Tag::Obj` instance.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// `kind` bytes of `__torajs_struct_accessor_find` (torajs-structmeta
/// `AccessorKind::from_raw`).
pub(crate) const KIND_GETTER: u8 = 0;
pub(crate) const KIND_SETTER: u8 = 1;

/// The dynobj probe's accessor sentinel — mirrors
/// `torajs_dynobj::layout::ANY_ACCESSOR`. A struct accessor answers the
/// same tag with a ZERO value channel: there is no AccessorPair cell,
/// and that zero is what routes [`__torajs_any_accessor_get`] into the
/// struct lane.
pub(crate) const ANY_ACCESSOR_TAG: u64 = 6;

/// MethodMeta flags bit 1 — mirror of torajs-structmeta
/// `METHOD_FLAG_TWIN_PRIMARY` (`method_call_dynobj/tail.rs` carries
/// the same twin): the record's adapter is the receiver-polymorphic
/// `__cmany_` twin, recv-first calling convention.
const METHOD_FLAG_TWIN_PRIMARY: u32 = 2;

/// How a struct's accessor half is reached.
enum StructAccessor {
    /// Object literal — the closure env cell out of the layout slot.
    Closure(*mut c_void),
    /// Class — the boxed adapter out of the dispatch table.
    Adapter(*const c_void),
    /// GENERIC class (RFC 20260815 刀 5) — the row's adapter is the
    /// `__cmany_` twin: the receiver box rides argv[0] and the env
    /// argument is dropped (the mono adapter's env-slot convention
    /// would hand the twin a bare struct ptr it decodes as a NaN
    /// box — the rotation-413 REVERT's TypeError).
    TwinAdapter(*const c_void),
}

/// Resolve one half of a struct's accessor for `prop`.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell.
unsafe fn resolve(ptr: *mut c_void, prop: &[u8], kind: u8) -> Option<StructAccessor> {
    unsafe {
        let class_tag = ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read();
        let layout = __torajs_struct_layout_lookup(class_tag);
        if layout.is_null() {
            return None;
        }
        let idx = __torajs_struct_accessor_find(layout, prop.as_ptr(), prop.len() as u32, kind);
        if idx != u32::MAX {
            let info = __torajs_struct_field_info(layout, idx);
            let env = ptr
                .cast::<u8>()
                .add(info.field_byte_offset as usize)
                .cast::<*mut c_void>()
                .read();
            if env.is_null() {
                return None;
            }
            return Some(StructAccessor::Closure(env));
        }
        let mut mflags: u32 = 0;
        let adapter = __torajs_struct_accessor_method_find_flags(
            layout,
            prop.as_ptr(),
            prop.len() as u32,
            kind,
            &mut mflags,
        );
        if adapter.is_null() {
            return None;
        }
        if mflags & METHOD_FLAG_TWIN_PRIMARY != 0 {
            return Some(StructAccessor::TwinAdapter(adapter));
        }
        Some(StructAccessor::Adapter(adapter))
    }
}

/// Does `ptr` carry an accessor property named `prop`? EITHER half
/// makes the property present: reading a set-only property answers
/// `undefined` (ES §10.1.8 — an accessor with an undefined `[[Get]]`),
/// which is a definite value, not an absent key.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell.
pub(crate) unsafe fn struct_accessor_present(ptr: *mut c_void, prop: &[u8]) -> bool {
    unsafe {
        resolve(ptr, prop, KIND_GETTER).is_some() || resolve(ptr, prop, KIND_SETTER).is_some()
    }
}

/// Invoke a resolved getter half, borrowing `recv` into the call.
///
/// # Safety
/// `recv` is the live `Tag::Obj` cell the accessor was resolved from.
unsafe fn invoke_getter(recv: *mut c_void, acc: StructAccessor) -> AnyValue {
    unsafe {
        match acc {
            // The lifted body is `(__env, __this)`: the closure cell's
            // boxed adapter feeds argv[0] into `__this`.
            StructAccessor::Closure(env) => {
                let entry = crate::method_call_closure_dispatch::boxed_entry_of(env.cast::<u8>());
                if entry == 0 {
                    return VALUE_UNDEFINED;
                }
                // Borrowed into the call — the receiver outlives it, so
                // the box takes no ref (the adapter's argv slots are
                // borrows by the boxed-entry contract).
                let argv = [box_void_ptr(recv)];
                invoke_boxed(env, entry, argv.as_ptr(), 1)
            }
            // `__cm_<C>__<p>_get` takes `__this` as its first param,
            // which the adapter reads out of the env slot — empty argv.
            StructAccessor::Adapter(adapter) => {
                invoke_boxed(recv, adapter as u64, core::ptr::null(), 0)
            }
            // `__cmany_` twin — recv-first: the receiver box rides
            // argv[0], the env argument is dropped (the
            // `method_call_dynobj/tail.rs` twin-primary shape).
            StructAccessor::TwinAdapter(adapter) => {
                let argv = [box_void_ptr(recv)];
                invoke_boxed(recv, adapter as u64, argv.as_ptr(), 1)
            }
        }
    }
}

/// A struct accessor's [[Set]] (RFC 20260714-objlit-accessor blade 7 —
/// the write mirror of blade 5's read). `value` is BORROWED into the
/// call, like every boxed-entry argv slot; the caller keeps its stake.
///
/// Answers `true` when the property is an accessor and the write is
/// resolved:
///
/// * a setter runs with the value as its argument;
/// * a GET-ONLY property throws (ES §10.1.9 / §6.2.5.6 — an assignment
///   whose [[Set]] is undefined fails, and a module is strict, so the
///   failure is a TypeError, not a silent no-op; bun agrees).
///
/// `false` = not an accessor at all — the caller keeps its own answer
/// for a data field (a typed struct through `any` cannot grow a
/// property; that is RFC 20260714-struct-dynamic-props).
///
/// # Safety
/// `obj` is a live `Tag::Obj` cell; `name` points at `name_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_accessor_set(
    obj: *mut c_void,
    name: *const u8,
    name_len: u32,
    value: AnyValue,
) -> bool {
    unsafe {
        if obj.is_null() || name.is_null() {
            return false;
        }
        let prop = core::slice::from_raw_parts(name, name_len as usize);
        if let Some(acc) = resolve(obj, prop, KIND_SETTER) {
            invoke_setter(obj, acc, value);
            return true;
        }
        if resolve(obj, prop, KIND_GETTER).is_some() {
            __torajs_throw_type_error(
                c"Attempted to assign to readonly property.".as_ptr() as *const core::ffi::c_char
            );
            return true;
        }
        false
    }
}

/// Invoke a resolved setter half with `value` — the receiver rides the
/// same slot the getter's does, and the value follows it.
///
/// # Safety
/// `recv` is the live `Tag::Obj` cell the accessor was resolved from.
unsafe fn invoke_setter(recv: *mut c_void, acc: StructAccessor, value: AnyValue) {
    unsafe {
        match acc {
            // The lifted body is `(__env, __this, v)`.
            StructAccessor::Closure(env) => {
                let entry = crate::method_call_closure_dispatch::boxed_entry_of(env.cast::<u8>());
                if entry == 0 {
                    return;
                }
                let argv = [box_void_ptr(recv), value];
                invoke_boxed(env, entry, argv.as_ptr(), 2);
            }
            // `__cm_<C>__<p>_set(__this, v)` — `__this` is the env.
            StructAccessor::Adapter(adapter) => {
                let argv = [value];
                invoke_boxed(recv, adapter as u64, argv.as_ptr(), 1);
            }
            // `__cmany_` twin — recv-first: `(recv box, v)`.
            StructAccessor::TwinAdapter(adapter) => {
                let argv = [box_void_ptr(recv), value];
                invoke_boxed(recv, adapter as u64, argv.as_ptr(), 2);
            }
        }
    }
}

/// A struct accessor's [[Get]], keyed by raw name bytes — the shape
/// the reflection walkers need (`torajs-meta`'s `Object.values` /
/// `Object.entries` over a struct cell enumerate layout slot names and
/// have no Str cell to spend an allocation on per key).
///
/// Answers the getter's result (OWNED), or `undefined` for a set-only
/// property — ES §10.1.8, an accessor with an undefined [[Get]]. An
/// absent property answers `undefined` too; the walkers only ask about
/// slots they just read out of the layout.
///
/// # Safety
/// `obj` is a live `Tag::Obj` cell; `name` points at `name_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_accessor_get(
    obj: *mut c_void,
    name: *const u8,
    name_len: u32,
) -> AnyValue {
    unsafe {
        if obj.is_null() || name.is_null() {
            return VALUE_UNDEFINED;
        }
        let prop = core::slice::from_raw_parts(name, name_len as usize);
        match resolve(obj, prop, KIND_GETTER) {
            Some(acc) => invoke_getter(obj, acc),
            None => VALUE_UNDEFINED,
        }
    }
}

/// The single [[Get]] behind an accessor member read on an `any`
/// receiver — see module doc. `pair_bits` is the value channel of the
/// probe that answered [`ANY_ACCESSOR_TAG`]:
///
/// * NON-ZERO — a dynobj `AccessorPair` cell; the existing lane invokes
///   it (unchanged, receiver-independent).
/// * ZERO — a struct accessor; resolve it against the receiver's
///   layout / dispatch table and invoke it WITH the receiver, so
///   `get v() { return this.a + 10 }` sees its `this`.
///
/// The result is owned. A getter that throws records a pending throw
/// the emitted `emit_throw_check` routes right after this call.
///
/// # Safety
/// `recv` is a live receiver (the probe just walked it); `key` is a
/// live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_accessor_get(
    recv: AnyValue,
    key: *const c_void,
    pair_bits: u64,
) -> AnyValue {
    unsafe {
        if pair_bits != 0 {
            return __torajs_accessor_invoke_getter(pair_bits as *const c_void, recv);
        }
        if !is_cell(recv) {
            return VALUE_UNDEFINED;
        }
        let ptr = as_void_ptr(recv);
        let cell_tag = ptr.cast::<u8>().add(4).cast::<u16>().read();
        // §10.5.8 — a Proxy receiver answers this same sentinel from
        // the pure probe pair, and this is the one place the `get`
        // trap runs (RFC 20260823-proxy-substrate 刀 1). It shares
        // the accessor route because it answers the same question:
        // the value is computed by invoking something, given the
        // receiver and the key.
        if cell_tag == Tag::Proxy as u16 {
            return crate::proxy::get(ptr, key, recv);
        }
        if cell_tag != Tag::Obj as u16 {
            return VALUE_UNDEFINED;
        }
        let k = crate::key_wtf8::KeyWtf8::of(key);
        // Set-only property: present, but its [[Get]] is undefined.
        __torajs_struct_accessor_get(ptr, k.as_ptr(), k.len())
    }
}

/// Blade 5 — is `key` an accessor property of the struct cell `ptr`?
/// The value channel answers 0 for one (there is no AccessorPair cell),
/// which is exactly what routes the invoke into the struct lane.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `key` a live Str cell.
pub(crate) unsafe fn struct_accessor_key(ptr: *mut c_void, key: *const c_void) -> bool {
    let k = unsafe { crate::key_wtf8::KeyWtf8::of(key) };
    unsafe { struct_accessor_present(ptr, k.as_bytes()) }
}

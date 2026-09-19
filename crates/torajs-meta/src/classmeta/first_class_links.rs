//! The first-class function-object links a class object gets at
//! register time (RFC 20260717-class-first-class-value knife A):
//! the class-ctor mark, `C.[[Prototype]]` per §15.7.14 / §10.2.3, and
//! `__proto_<C>.constructor = C`. Split out of the registry module
//! (file-size); the body is verbatim, and the private registry
//! tables and externs it reads stay in the parent, reached through
//! `super::`.

use core::ffi::c_void;

use super::{
    __torajs_builtin_ctor_value, __torajs_dynobj_define_plain, __torajs_dynobj_mark_class_ctor,
    __torajs_get_builtin_prototype, __torajs_rc_inc, __torajs_str_drop, ANY_HEAP,
    CLASSES_BY_TAG_IMM, DEFINE_CTOR_FLAGS, FUNCTION_PROTO_TAG, PROTOS_BY_TAG_IMM, TAG_DYNOBJ,
    alloc_str_key, heap_type_tag, in_range, is_cell_imm, reify,
};

/// RFC 20260717-class-first-class-value knife A — the first-class
/// function-object links, wired once per class at register time:
///
/// 1. mark the class dynobj `FLAG_DYNOBJ_CLASS_CTOR` so `typeof C`
///    answers `"function"` (ES: a class constructor IS a function
///    object; tr models it as a dynobj whose tag alone reads
///    "object"),
/// 2. link `C.[[Prototype]]` → %Function.prototype% (§10.2.3
///    MakeConstructor / §15.7.14: constructor functions inherit from
///    %Function.prototype%),
/// 3. define `__proto_<C>.constructor = C` with `{writable: true,
///    enumerable: false, configurable: true}` (§10.2.3 step 4).
///
/// The `constructor ↔ prototype` reference cycle is between two
/// module-scope singletons whose lifetime spans the process — no
/// leak surface.
///
/// # Safety
/// `class_anyv` is a cell-encoded AnyValue pointing at a live heap
/// object; `PROTOS_BY_TAG_IMM[tag]` was registered by the emit
/// sequence just before this call (class_globals.rs emit order).
pub(super) unsafe fn wire_first_class_links(
    tag: i64,
    class_anyv: u64,
    parent_tag: i64,
    builtin_parent_tag: i64,
) {
    unsafe {
        let class_cell = class_anyv as *mut c_void;
        __torajs_dynobj_mark_class_ctor(class_cell);
        // Step 2 — §15.7.14 class heritage: a derived class ctor's
        // [[Prototype]] IS the parent ctor (`getPrototypeOf(Sub) ===
        // Super`; §20.5.6.2 NativeError.[[Prototype]] = Error is the
        // same rule). A root class — or a parent that never
        // registered (dropout) — links %Function.prototype% per
        // §10.2.3. Registration runs in source order, so the parent
        // slot is filled before any subclass wires (extends is
        // TDZ-gated upstream). Both link targets outlive the entry
        // (registry slots are process-lifetime, the singleton is
        // immortal); ordinary_set_prototype_of rc_incs the stored
        // link itself.
        let parent = if in_range(parent_tag) {
            CLASSES_BY_TAG_IMM[parent_tag as usize]
        } else {
            0
        };
        // A class object fresh from its register is extensible and
        // its heritage chain is acyclic by construction — the link
        // cannot be refused, so the answer carries nothing.
        let flags = class_cell.cast::<u8>().add(6).cast::<u16>().read();
        if is_cell_imm(parent) && heap_type_tag(parent as *const c_void) == TAG_DYNOBJ {
            let _ = crate::reflect_proto_set::ordinary_set_prototype_of(class_cell, parent, flags);
        } else if builtin_parent_tag >= 0 {
            // A stripped BUILTIN parent (`class CP extends Promise`) —
            // §15.7.14 still applies: `getPrototypeOf(CP) === Promise`,
            // so static inheritance (`CP.resolve`) resolves through
            // the ordinary member-get chain walk onto the interned
            // builtin ctor cell (immortal; the set rc_incs the link).
            let ctor = __torajs_builtin_ctor_value(builtin_parent_tag);
            let _ = crate::reflect_proto_set::ordinary_set_prototype_of(class_cell, ctor, flags);
        } else {
            let func_proto = __torajs_get_builtin_prototype(FUNCTION_PROTO_TAG);
            if !func_proto.is_null() {
                let _ = crate::reflect_proto_set::ordinary_set_prototype_of(
                    class_cell,
                    func_proto as u64,
                    flags,
                );
            }
        }
        // Step 3 — define transfers the value stake, so the entry owns
        // one reference to the class object.
        let proto = PROTOS_BY_TAG_IMM[tag as usize];
        if is_cell_imm(proto) && heap_type_tag(proto as *const c_void) == TAG_DYNOBJ {
            let key = alloc_str_key(b"constructor");
            __torajs_rc_inc(class_cell);
            let mut slot = proto as *mut c_void;
            __torajs_dynobj_define_plain(
                &mut slot,
                key,
                ANY_HEAP as u64,
                class_anyv,
                DEFINE_CTOR_FLAGS,
            );
            __torajs_str_drop(key);
            // rotation 186 — dynobj define may RESIZE (fresh block +
            // free old); every table read after a define must see
            // the moved cell or it dereferences freed memory. Same
            // writeback on every class/proto define site below.
            PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
            reify::reify_prototype_methods(tag, slot);
        }
    }
}

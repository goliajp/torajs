//! The prototype's reified own-method faces — split from the
//! parent when the 404-01 twin-primary skip pushed it past the
//! 500-line hard limit. The parent answers "how a class's cells and
//! links register"; this answers "how one method lands on the
//! prototype as a callable face". Body verbatim; a child module
//! reaches the parent's private externs and statics directly.

use super::*;

/// Knife B cut 1 — the prototype's own method entries. Walks the
/// class's `.__class_methods_<i>` boxed-adapter table (the same
/// records `struct_method` dispatch resolves) and defines one
/// reified function object per method onto `__proto_<C>`, with the
/// §10.2.10 method attribute set `{writable: true, enumerable:
/// false, configurable: true}` — the same flags_byte the
/// `constructor` link uses. Accessor slots (`__getter_<p>` /
/// `__setter_<p>` synthetic names) are skipped — their surface is
/// the AccessorPair face, not an own method entry.
///
/// The table merges parent chains, so a subclass prototype lists
/// inherited methods too — a recorded boundary vs bun (spec lists
/// only own methods per prototype level).
///
/// # Safety
/// `proto` is a live dynobj heap pointer.
pub(super) unsafe fn reify_prototype_methods(tag: i64, proto: *mut c_void) {
    unsafe {
        let layout = __torajs_struct_layout_lookup(tag as u32);
        if layout.is_null() {
            return;
        }
        let n = __torajs_struct_method_count(layout);
        // rotation 186 — thread ONE slot through the whole define
        // loop and write the table back at the end: any define may
        // resize (fresh block + free old), and both the next
        // iteration and every later table read must see the move.
        let mut slot = proto;
        for i in 0..n {
            let mut name_ptr: *const u8 = core::ptr::null();
            let mut name_len: u32 = 0;
            let adapter = __torajs_struct_method_at(layout, i, &mut name_ptr, &mut name_len);
            if adapter.is_null() || name_ptr.is_null() || name_len == 0 {
                continue;
            }
            let name = core::slice::from_raw_parts(name_ptr, name_len as usize);
            if name.starts_with(b"__getter_") || name.starts_with(b"__setter_") {
                continue;
            }
            // RFC 20260802 刀 2 — a runtime computed member's `__ccm_`
            // sentinel is not a property name; its own entry lands via
            // the class-decl-position computed define instead.
            if name.starts_with(b"__ccm_") {
                continue;
            }
            // S2.38 — bit 0 of the MethodMeta flags word marks a
            // receiver-free body; the face runs bare calls with a
            // null receiver instead of the this-undefined TypeError.
            let flags = __torajs_struct_method_flags_at(layout, i);
            let this_free = u64::from(flags & 1);
            // Blade 3 — the face carries its owning class tag + the
            // `__cmany_` twin adapter so a re-bound receiver routes
            // through the any-lane body instead of the mono's baked
            // offsets (invoke_with_this's guard).
            //
            // 405-04 — bit 1 marks a twin-primary record (GENERIC
            // class row): its adapter IS the receiver-polymorphic
            // `__cmany_` twin (recv-first calling convention — the
            // receiver rides argv[0], the env argument is dropped).
            // Mint the face with the STATIC-face encoding `(tag 0,
            // twin = adapter)`: every dispatch consumer routes that
            // pair through invoke_boxed_recv_first and never the env
            // channel, so the face is sound under every receiver.
            // (Pre-405-04 these rows were skipped — the env-channel
            // mint would have fed `this` off the first argument.)
            let (cell_tag, twin) = if flags & 2 != 0 {
                (0u64, adapter as u64)
            } else {
                (tag as u64, __torajs_struct_method_twin_at(layout, i) as u64)
            };
            let cell = __torajs_class_method_cell_new(adapter as u64, this_free, cell_tag, twin);
            let key = alloc_str_key(name);
            // The minted cell is FLAG_STATIC_LITERAL (rc no-op) — the
            // define's transferred stake is the entry's sole handle.
            __torajs_dynobj_define(
                &mut slot,
                key,
                ANY_HEAP as u64,
                cell as u64,
                DEFINE_CTOR_FLAGS,
            );
            __torajs_str_drop(key);
        }
        PROTOS_BY_TAG_IMM[tag as usize] = slot as u64;
    }
}

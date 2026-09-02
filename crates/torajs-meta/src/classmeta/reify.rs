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
/// `constructor` link uses.
///
/// Rotation 562 — an accessor slot (`__getter_<p>` / `__setter_<p>`)
/// defines its AccessorPair HERE too, from the same table rows, so
/// the property lands at its DECLARATION position. The emitted
/// `__torajs_class_accessor_reify` statement still runs afterwards
/// and redefines the same key, which keeps the position and the
/// value; before this, a prototype's own entries came out
/// methods-then-accessors, and `class Y { get g() {} m() {} }`
/// answered `["constructor", "m", "g"]` where bun answers
/// declaration order (`console.log(new Y())` printed the two rows
/// in that same wrong order).
///
/// The table merges parent chains — it is the dispatch resolution,
/// answering "which body does `c.m()` reach" in one lookup — so only
/// the rows flagged as DECLARED by this class become own entries
/// here. Each prototype level lists its own methods, and the rest are
/// found by the walk, per spec.
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
            // 508-03 — bit 2 marks a row this class DECLARES. An
            // inherited row is already reachable one hop up the
            // prototype chain, so copying it here would only add a
            // name `hasOwnProperty` / `getOwnPropertyNames` must not
            // see, and a shadow that outlives re-linking the chain
            // (`setPrototypeOf(D.prototype, standin)` left the copy in
            // front of `standin`'s method). Read before the accessor
            // arm: an inherited accessor is an inherited row too.
            let flags = __torajs_struct_method_flags_at(layout, i);
            if flags & 4 == 0 {
                continue;
            }
            if let Some(prop) = accessor_slot_prop(name) {
                // A computed accessor's slot names a `__ccm_`
                // sentinel, not a property; its own entry lands under
                // the runtime key via the class-decl-position computed
                // define — the carve-out the plain-method arm below
                // makes too. Otherwise the pair defines once, at the
                // FIRST of its two rows: that row's index IS the
                // declaration position, and both halves come off the
                // table.
                if !prop.starts_with(b"__ccm_") && !accessor_row_before(layout, i, prop) {
                    let (get_adapter, set_adapter) = accessor_adapters(layout, n, prop);
                    let key = alloc_str_key(prop);
                    super::define::define_accessor_pair(
                        &mut slot,
                        key,
                        prop,
                        get_adapter,
                        set_adapter,
                        DEFINE_ACCESSOR_FLAGS,
                    );
                    __torajs_str_drop(key);
                }
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
            __torajs_dynobj_define_plain(
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

/// The property an accessor slot names (`__getter_x` / `__setter_x`
/// → `x`), or `None` for a plain method row.
unsafe fn accessor_slot_prop(name: &[u8]) -> Option<&[u8]> {
    name.strip_prefix(b"__getter_".as_slice())
        .or_else(|| name.strip_prefix(b"__setter_".as_slice()))
}

/// The get / set boxed adapters the table holds for property `prop`
/// (0 for a half the class does not declare).
unsafe fn accessor_adapters(layout: *const c_void, n: u32, prop: &[u8]) -> (u64, u64) {
    let (mut get, mut set) = (0u64, 0u64);
    for j in 0..n {
        let mut np: *const u8 = core::ptr::null();
        let mut nl: u32 = 0;
        let adapter = unsafe { __torajs_struct_method_at(layout, j, &mut np, &mut nl) };
        if adapter.is_null() || np.is_null() || nl == 0 {
            continue;
        }
        // Only a row this class DECLARES becomes an own entry (508-03).
        if unsafe { __torajs_struct_method_flags_at(layout, j) } & 4 == 0 {
            continue;
        }
        let name = unsafe { core::slice::from_raw_parts(np, nl as usize) };
        if let Some(rest) = name.strip_prefix(b"__getter_".as_slice())
            && rest == prop
        {
            get = adapter as u64;
        } else if let Some(rest) = name.strip_prefix(b"__setter_".as_slice())
            && rest == prop
        {
            set = adapter as u64;
        }
    }
    (get, set)
}

/// Whether an accessor row for `prop` appears in rows `[0, upto)` —
/// the two halves of a get/set pair define once, at the first.
unsafe fn accessor_row_before(layout: *const c_void, upto: u32, prop: &[u8]) -> bool {
    for j in 0..upto {
        let mut np: *const u8 = core::ptr::null();
        let mut nl: u32 = 0;
        let adapter = unsafe { __torajs_struct_method_at(layout, j, &mut np, &mut nl) };
        if adapter.is_null() || np.is_null() || nl == 0 {
            continue;
        }
        let name = unsafe { core::slice::from_raw_parts(np, nl as usize) };
        if unsafe { accessor_slot_prop(name) } == Some(prop) {
            return true;
        }
    }
    false
}

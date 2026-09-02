//! 405-05 — the prototype entries of the struct inspect walker.
//! bun's default inspect lists a class instance's prototype methods
//! after its own properties (`NG { k: 6, get: [Function: get] }`)
//! and renders prototype accessors as `[Getter]` / `[Setter]` /
//! `[Getter/Setter]`; tr's `struct_print` walked only
//! `field_metadata`, so every method-carrying class printed bare.
//!
//! Rotation 562 — the walk reads the class's REIFIED prototype
//! (`__proto_<C>`, the object `Object.getOwnPropertyNames` answers
//! from) instead of the compile-time `.__class_methods_<i>` table.
//! The table names a row by the symbol the desugar minted, and for a
//! computed member that symbol is a `__ccm_<n>__` sentinel — a name
//! no property has, printed straight into the user's output
//! (`class Y { get [k]() {} }` printed `__ccm_1__: [Getter]` where
//! bun prints `k1: [Getter]`, and the `__getter_`-prefixed spelling
//! walked past the sentinel filter that was meant to catch it). The
//! prototype carries the runtime key, so the printed face and the
//! reflected face now read one object.
//!
//! Shape follows bun's `forEachProperty` (`bindings.cpp`): up to
//! five [[Prototype]] hops, `constructor` always hidden, the fast /
//! slow walk decided per hop (see anyvalue's `key_hidden`), and a
//! key a nearer prototype already carries is that prototype's — the
//! override the merged table used to express with its `seen` set.

use core::ffi::c_void;

use crate::reflect::{
    __torajs_anyv_unbox_tag, __torajs_anyv_unbox_value, __torajs_dynobj_get_tag,
    __torajs_dynobj_get_value, __torajs_dynobj_has, __torajs_str_drop, ANY_HEAP, PROTO_SLOT_KEY,
    TAG_DYNOBJ, alloc_str_key, heap_type_tag,
};

unsafe extern "C" {
    fn __torajs_io_putc_out(c: i32) -> i32;
    // torajs-anyvalue::inspect — the shared key writer / width /
    // hidden-key predicate, and the nested-context value printer
    // (a closure renders `[Function: m]`, an AccessorPair
    // `[Getter]`).
    fn __torajs_print_str_cell_as_key(cell: *const c_void);
    fn __torajs_key_cell_print_len(cell: *const c_void) -> u32;
    fn __torajs_key_cell_inspect_hidden(key: *const c_void, flags: u64, slow: i32) -> i32;
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);
    fn __torajs_inspect_line_add(n: u32);
    // torajs-dynobj — own-entry walk of one prototype.
    fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64;
    fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void;
    fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64;
    fn __torajs_dynobj_iter_print_order(obj: *const c_void, out: *mut u64, cap: u64) -> u64;
    fn __torajs_dynobj_iter_slow_mode(obj: *const c_void) -> i32;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// bun walks at most five prototypes (`prototypeCount < 5`).
const MAX_HOPS: usize = 5;

#[inline]
unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { __torajs_io_putc_out(b as i32) };
    }
}

/// An AnyValue's dynobj cell, or null when it is not one.
unsafe fn dynobj_of_anyv(anyv: u64) -> *const c_void {
    if anyv == 0 {
        return core::ptr::null();
    }
    let tag = unsafe { __torajs_anyv_unbox_tag(anyv) };
    let val = unsafe { __torajs_anyv_unbox_value(anyv) };
    if tag != ANY_HEAP || val == 0 {
        return core::ptr::null();
    }
    let cell = val as *const c_void;
    if unsafe { heap_type_tag(cell) } != TAG_DYNOBJ {
        return core::ptr::null();
    }
    cell
}

/// The prototype chain of `class_tag`, nearest first — the class's
/// own `__proto_<C>` and the user prototypes above it.
unsafe fn proto_chain(class_tag: u32) -> ([*const c_void; MAX_HOPS], usize) {
    let mut chain = [core::ptr::null(); MAX_HOPS];
    let mut n = 0usize;
    let mut cur =
        unsafe { dynobj_of_anyv(crate::classmeta::proto_anyv_borrowed(class_tag as i64)) };
    let key = unsafe { alloc_str_key(PROTO_SLOT_KEY) };
    while !cur.is_null() && n < MAX_HOPS {
        chain[n] = cur;
        n += 1;
        cur = if unsafe { __torajs_dynobj_has(cur, key) } {
            let t = unsafe { __torajs_dynobj_get_tag(cur, key) } as i64;
            let v = unsafe { __torajs_dynobj_get_value(cur, key) };
            unsafe { dynobj_of_anyv(__torajs_anyv_box_from_pair(t, v as i64)) }
        } else {
            core::ptr::null()
        };
    }
    unsafe { __torajs_str_drop(key) };
    (chain, n)
}

/// Run `f` over every prototype entry the inspect face prints, in
/// bun's order (each hop's own entries, nearest prototype first).
unsafe fn for_each_proto_entry(class_tag: u32, mut f: impl FnMut(*mut c_void, u64)) {
    let (chain, n_chain) = unsafe { proto_chain(class_tag) };
    for h in 0..n_chain {
        let proto = chain[h];
        let len = unsafe { __torajs_dynobj_iter_len(proto) };
        if len == 0 {
            continue;
        }
        let mut order = vec![0u64; len as usize];
        let n = unsafe { __torajs_dynobj_iter_print_order(proto, order.as_mut_ptr(), len) };
        order.truncate(n as usize);
        // bun's fast / slow walk, decided per hop (`fast` is
        // recomputed at each `restart`): the shape rules the fast
        // walk out, or the fast walk hits nothing at all — a
        // prototype carrying only `constructor` and a
        // `@@toStringTag` is the second case, and its assigned
        // (enumerable) tag is what the slow walk then prints.
        let mut slow = unsafe { __torajs_dynobj_iter_slow_mode(proto) };
        if slow == 0 {
            let fast_hit = order.iter().any(|&i| unsafe {
                let key = __torajs_dynobj_iter_key(proto, i);
                !key.is_null()
                    && __torajs_key_cell_inspect_hidden(
                        key,
                        __torajs_dynobj_iter_flags(proto, i),
                        0,
                    ) == 0
            });
            if !fast_hit {
                slow = 1;
            }
        }
        for &i in &order {
            let key = unsafe { __torajs_dynobj_iter_key(proto, i) };
            if key.is_null() {
                continue;
            }
            let flags = unsafe { __torajs_dynobj_iter_flags(proto, i) };
            if unsafe { __torajs_key_cell_inspect_hidden(key, flags, slow) } != 0 {
                continue;
            }
            // A nearer prototype's entry of the same key overrides
            // this one, and printed already.
            if (0..h).any(|j| unsafe { __torajs_dynobj_has(chain[j], key.cast::<u8>()) }) {
                continue;
            }
            // The RAW stored value (an accessor entry is its
            // `AccessorPair`, which the value printer renders
            // `[Getter]`; the keyed GET would resolve it instead).
            f(key, unsafe { __torajs_dynobj_iter_value(proto, i) });
        }
    }
}

/// True when the class has at least one printable prototype entry —
/// feeds the caller's empty-form (`Name {}`) early return.
pub(crate) unsafe fn has_visible_methods(class_tag: u32) -> bool {
    let mut any = false;
    unsafe { for_each_proto_entry(class_tag, |_, _| any = true) };
    any
}

/// Emit the prototype entries at `indent + 2`, continuing the
/// caller's separator protocol (`emitted > 0` → leading `,\n`).
pub(crate) unsafe fn print_proto_methods(class_tag: u32, indent: u32, emitted: &mut u32) {
    unsafe {
        for_each_proto_entry(class_tag, |key, anyv| {
            if *emitted > 0 {
                put_bytes(b",\n");
                __torajs_inspect_line_add(1);
            }
            let klen = __torajs_key_cell_print_len(key);
            for _ in 0..indent + 2 {
                __torajs_io_putc_out(b' ' as i32);
            }
            __torajs_print_str_cell_as_key(key);
            put_bytes(b": ");
            __torajs_inspect_line_add(klen + 2);
            __torajs_print_anyv_inline_at(anyv, indent + 2);
            *emitted += 1;
        })
    };
}

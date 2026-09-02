//! `console.log(obj)` for arbitrary dynobj (plain object literals,
//! `Object.entries` rows, etc) — nested-print substrate trunk
//! Commit 3.
//!
//! Walks a dynobj's dense entry array via
//! `__torajs_dynobj_iter_{len,key,value}` in bun's print order
//! (`iter_print_order`; CPython-3.7 compact-dict shape —
//! `torajs-dynobj` since session 85 `88c9778`), skips tombstones
//! (`key == null`), and emits each `<key>: <value>` pair through
//! `__torajs_print_anyv_inline` (Commit 1 substrate). Every own
//! property prints, enumerable or not — bun's inspect walks the own
//! keys without the enumerable filter (`Object.keys` / for-in /
//! `JSON.stringify` keep excluding them).
//!
//! Output shape (bun-parity for plain object literals):
//! - empty (no own props) → `{}` (no '\n')
//! - non-empty → `{\n  k: v,\n  k2: v2,\n}` (no trailing '\n', but
//!   keeps the final `}` on its own line per bun's pretty form;
//!   trailing comma on the last entry mirrors bun exactly)
//!
//! Key emission: keys are borrowed Str cell pointers (returned by
//! `__torajs_dynobj_iter_key`). They are reinterpreted as a NaN-box
//! AnyValue (raw cell ptr is a valid NaN-box cell encoding —
//! `nanbox::box_void_ptr`) and dispatched through
//! `__torajs_print_anyv_inline` which routes them via its Tag::Str
//! / Tag::Substr arm. Result: keys print **unquoted** (`a` not
//! `"a"`) matching bun's obj-literal form (Map / Set quote their
//! keys differently — that's a later commit's concern).
//!
//! ## Newline policy
//!
//! Commit 3 emits no trailing '\n' after the closing `}`. The
//! Commit 4 `__torajs_print_anyv` Tag::Obj arm + SSA dispatcher
//! obj-elem case append '\n' when the outer console.log call
//! terminates.

use core::ffi::c_void;

use crate::iter::{
    __torajs_dynobj_iter_flags, __torajs_dynobj_iter_key, __torajs_dynobj_iter_len,
    __torajs_dynobj_iter_value,
};
use crate::iter_print_order::__torajs_dynobj_iter_print_order;
use crate::iter_slow_mode::__torajs_dynobj_iter_slow_mode;
use crate::layout::{DYNOBJ_HDR_FLAG_NULL_PROTO, TAG_DYNOBJ, TAG_OBJ};
use crate::proto_chain::{__torajs_dynobj_proto_next, MAX_PROTO_HOPS};

/// Universal heap-header `type_tag u16 @4` / `flags u16 @6`.
const HDR_TYPE_OFF: usize = 4;
const HDR_FLAGS_OFF: usize = 6;

unsafe extern "C" {
    /// mmalloc Layer-1 sized API (same pair resize.rs uses) — the
    /// ES-order visit buffer is a per-print cold-path allocation.
    #[link_name = "__torajs_calloc"]
    fn calloc(size: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn free(p: *mut c_void, size: usize);
    /// Indent-threaded inline AnyValue printer from
    /// `torajs-anyvalue::inspect` (inspect indent trunk).
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);
    /// torajs-anyvalue — the own keys inspect leaves out
    /// (`constructor`, a non-enumerable `__proto__`, `@@toStringTag`).
    fn __torajs_key_cell_inspect_hidden(key: *const c_void, flags: u64, slow: i32) -> i32;
    /// torajs-anyvalue — bun's name prefix for an ordinary object
    /// (`constructor` name / `@@toStringTag`), 1 when one printed.
    fn __torajs_inspect_obj_name_prefix(obj: *const c_void) -> i32;
    /// Line-width estimate primitives (inspect wrap trunk) — mirror
    /// of bun's `estimated_line_length` accounting, hosted in
    /// `torajs-anyvalue::inspect::formatters`.
    fn __torajs_inspect_line_reset(cols: u32);
    fn __torajs_inspect_line_add(n: u32);
    /// Per-byte stdout writer (`libtorajs_io.a`). Same buffer
    /// shared with IR-emitted `print_*` and Str / Arr printers.
    fn __torajs_io_putc_out(c: i32) -> i32;
    /// Emit a Str / Substr cell as an object key — bare when the
    /// key is an ASCII identifier, JSON-quoted otherwise
    /// (`{ a: 1 }` but `{ "a-b": 1 }`, bun `isLatin1Identifier`
    /// rule). Inspect escape trunk helper in
    /// `torajs-anyvalue::inspect`.
    fn __torajs_print_str_cell_as_key(cell: *const c_void);
    /// Own-entry probe — the shadow rule of the prototype walk.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-meta — the rows a `Tag::Obj` struct cell (a typed
    /// object literal, a class instance) contributes to this block
    /// (562-10). `any_emitted` carries the `{`-and-separator
    /// protocol across the crate seam.
    /// torajs-meta — `[class Z]` for a class object (562-06); 1 when
    /// it printed, 0 for any other cell.
    fn __torajs_class_object_print(cell: *const c_void) -> i32;
    fn __torajs_struct_put_own_rows_at(
        cell: *const c_void,
        indent: u32,
        nearer: *const *const c_void,
        nearer_len: usize,
        any_emitted: *mut i32,
    );
}

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_out(b as i32) };
}

#[inline]
unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

/// `console.log(obj)` recursive walker for dynobj. Emits the bun
/// multi-line `{\n  k: v,\n}` pretty form with no trailing newline.
///
/// # Safety
///
/// `obj` must be either NULL or a valid dynobj heap object whose
/// header / entry array / index follow the layout asserted by
/// `torajs-dynobj::layout`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_print_any(obj: *const c_void) {
    // Top-level entry (SSA dispatcher / any.rs top-level arm) — a
    // fresh console.log line starts at column 0.
    unsafe { __torajs_inspect_line_reset(0) };
    unsafe { obj_print_any_at(obj, 0) }
}

/// Indent-threaded export of the walker (inspect indent trunk) —
/// `indent` is this object's own indent column: fields pad at
/// `indent + 2`, the closing `}` at `indent` (bun's uniform depth*2
/// model). Values thread `indent + 2` as their own indent so nested
/// composites keep descending.
///
/// # Safety
///
/// Same contract as [`__torajs_obj_print_any`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_print_any_at(obj: *const c_void, indent: u32) {
    unsafe { obj_print_any_at(obj, indent) }
}

unsafe fn obj_print_any_at(obj: *const c_void, indent: u32) {
    unsafe {
        if obj.is_null() {
            put_bytes(b"null");
            return;
        }
        // 562-06 — a class object is a dynobj carrying `name` /
        // `length` / `prototype`, but bun prints it as `[class Z]`
        // with no block at all, at every depth.
        if __torajs_class_object_print(obj) != 0 {
            return;
        }
        // Name prefix — the `constructor`'s name or the
        // `@@toStringTag` (bun `get_object_name`, see anyvalue's
        // `inspect/obj_name`); when neither names it, the
        // `Object.create(null)` form (regex `.groups`, module
        // namespaces) still prefixes at every depth.
        let hdr_flags = *((obj as *const u8).add(HDR_FLAGS_OFF) as *const u16);
        if __torajs_inspect_obj_name_prefix(obj) == 0 && hdr_flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0
        {
            put_bytes(b"[Object: null prototype] ");
        }
        let mut any_emitted = false;
        put_own_rows(obj, &[], indent, &mut any_emitted);
        // The prototypes above it — bun walks up to five and prints
        // what it finds (`Object.create(base)` shows `base`'s
        // properties), a key a nearer object carries belonging to
        // that object.
        let mut nearer: [*const c_void; MAX_PROTO_HOPS + 1] = [core::ptr::null(); 6];
        nearer[0] = obj;
        let mut n_nearer = 1usize;
        let mut cur = __torajs_dynobj_proto_next(obj);
        while !cur.is_null() && n_nearer <= MAX_PROTO_HOPS {
            put_proto_rows(cur, &nearer[..n_nearer], indent, &mut any_emitted);
            nearer[n_nearer] = cur;
            n_nearer += 1;
            cur = __torajs_dynobj_proto_next(cur);
        }
        if !any_emitted {
            put_bytes(b"{}");
        } else {
            put_bytes(b",\n");
            __torajs_inspect_line_add(1);
            put_indent(indent);
            put_byte(b'}');
        }
    }
}

/// The rows one prototype contributes, by the cell shape that holds
/// them: a dynobj's are here, a typed object literal's (a `Tag::Obj`
/// struct cell) live in `torajs-meta` behind the 562-10 seam. Any
/// other tag contributes nothing — `proto_next` returns only these
/// two.
///
/// # Safety
/// `src` is a live heap cell; `nearer` holds live dynobj cells.
unsafe fn put_proto_rows(
    src: *const c_void,
    nearer: &[*const c_void],
    indent: u32,
    any_emitted: &mut bool,
) {
    unsafe {
        match *((src as *const u8).add(HDR_TYPE_OFF) as *const u16) {
            TAG_DYNOBJ => put_own_rows(src, nearer, indent, any_emitted),
            TAG_OBJ => {
                let mut any = i32::from(*any_emitted);
                __torajs_struct_put_own_rows_at(
                    src,
                    indent,
                    nearer.as_ptr(),
                    nearer.len(),
                    &mut any,
                );
                *any_emitted = any != 0;
            }
            _ => {}
        }
    }
}

#[inline]
unsafe fn put_indent(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}

/// Emit the visible own rows of `src` at `indent`, skipping any key
/// one of `nearer` (the objects already walked, own object first)
/// carries — bun's `visitedProperties`. `any_emitted` carries the
/// caller's `{`-and-separator protocol across the chain.
///
/// # Safety
/// `src` is a live dynobj; `nearer` holds live cells.
unsafe fn put_own_rows(
    src: *const c_void,
    nearer: &[*const c_void],
    indent: u32,
    any_emitted: &mut bool,
) {
    unsafe {
        let len = __torajs_dynobj_iter_len(src);
        if len == 0 {
            return;
        }
        // bun's print order (see `iter_print_order`) — index keys
        // ascending first, then insertion order, symbol keys in
        // place unless an index key pushed them last; holes
        // pre-excluded so no NULL-key check in the loop.
        let order = calloc(len as usize * 8) as *mut u64;
        let n = __torajs_dynobj_iter_print_order(src, order, len);
        // bun's fast / slow walk (`key_hidden` module doc): the slow
        // walk when the shape rules the fast one out, or when the
        // fast walk would print nothing (bun's `anyHits` restart).
        let mut slow = __torajs_dynobj_iter_slow_mode(src);
        if slow == 0 {
            let fast_hit = (0..n).any(|j| {
                let i = *order.add(j as usize);
                let key = __torajs_dynobj_iter_key(src, i);
                __torajs_key_cell_inspect_hidden(key, __torajs_dynobj_iter_flags(src, i), 0) == 0
            });
            if !fast_hit && n > 0 {
                slow = 1;
            }
        }
        for j in 0..n {
            let i = *order.add(j as usize);
            let key = __torajs_dynobj_iter_key(src, i);
            if __torajs_key_cell_inspect_hidden(key, __torajs_dynobj_iter_flags(src, i), slow) != 0
            {
                continue;
            }
            if nearer.iter().any(|&o| __torajs_dynobj_has(o, key) != 0) {
                continue;
            }
            if !*any_emitted {
                put_bytes(b"{\n");
                // bun handleFirstProperty (ConsoleObject.zig:1893):
                // the estimate is OVERWRITTEN to parent-indent + 1 on
                // the first property; later property rows do NOT
                // reset (bun's deliberate accumulation — the estimate
                // only gates nested array wrap decisions).
                __torajs_inspect_line_reset(indent + 1);
                *any_emitted = true;
            } else {
                put_bytes(b",\n");
                __torajs_inspect_line_add(1);
            }
            put_indent(indent + 2);
            // Key — borrowed Str or Symbol cell ptr. A Str is bare
            // when it's an ASCII identifier, JSON-quoted otherwise
            // (bun's isLatin1Identifier rule); a Symbol prints as
            // `[Symbol(desc)]`.
            __torajs_print_str_cell_as_key(key);
            put_bytes(b": ");
            __torajs_inspect_line_add(2);
            // Value — already a NaN-box AnyValue per iter_value's
            // u64 return contract. Its own indent is this field
            // row's column (indent + 2).
            __torajs_print_anyv_inline_at(__torajs_dynobj_iter_value(src, i), indent + 2);
        }
        free(order as *mut c_void, len as usize * 8);
    }
}

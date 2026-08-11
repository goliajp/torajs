//! Top-level `console.log(v)` / `typeof v` entries — NaN-box
//! [`AnyValue`] dispatch with the trailing newline owned by this
//! module. Composite tags delegate to typed walkers
//! (`__torajs_arr_print_any`, `__torajs_map_print`, etc) declared in
//! [`super::formatters`]; nested cells inside a composite come back
//! through [`super::tag_dispatch::__torajs_print_anyv_inline`].

use core::ffi::c_void;

use super::formatters::{
    __torajs_anyv_struct_print_inline, __torajs_arr_print_any, __torajs_bigint_print_inline,
    __torajs_fn_print_inline, __torajs_inspect_line_reset, __torajs_io_putc_out,
    __torajs_map_print, __torajs_obj_print_any, __torajs_promise_print,
    __torajs_regex_print_inline, __torajs_set_print, __torajs_str_print, __torajs_substr_print,
    __torajs_symbol_print_inline, SUBSTR_VIEW_FLAG, alloc_literal, heap_flags, heap_type_tag,
    print_bool, print_f64, print_i64, put_bytes, put_closure_fn_name, put_date_inline,
    put_f64_inline, put_i64_inline, put_str_cell_inline, put_str_cell_inline_esc,
    put_substr_cell_inline, put_substr_cell_inline_esc, str_cell_is_bare_key, write_line,
};
use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined, short_str_bytes, short_str_len,
};
use torajs_rc::Tag;

/// SSA dispatcher entry for `console.log(fn_addr: Type::FnSig)` —
/// emits the bun `[Function: <name>]` form plus '\n'. Phase 2 wire
/// (fn-name registry Step 5) — delegates the table lookup to
/// `__torajs_fn_print_inline` (torajs-fnname) which walks the rodata
/// `__torajs_fn_name_table[]` set up by torajs-link's
/// 3b.3 / 3b.4-5 chain-fixup pipeline.
///
/// # Safety
///
/// `fn_addr` is the raw code-section pointer
/// `InstKind::FnAddr` lowers to; it's compared (not dereferenced)
/// against the table's `fn_addr` slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_print_outer(fn_addr: u64) {
    unsafe { __torajs_fn_print_inline(fn_addr) };
    unsafe { __torajs_io_putc_out(b'\n' as i32) };
}

/// `typeof v` per ES §13.5.3 — NaN-box [`AnyValue`] entry point.
/// Returns a fresh Str. Dispatches on the immediate NaN-box
/// predicates (no heap struct read).
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object
/// (only the `HeapHeader::type_tag` at +4 is read).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_typeof(v: AnyValue) -> *mut u8 {
    if is_null(v) {
        return alloc_literal(b"object");
    }
    if is_undefined(v) {
        return alloc_literal(b"undefined");
    }
    if is_bool(v) {
        return alloc_literal(b"boolean");
    }
    if is_int32(v) || is_double(v) {
        return alloc_literal(b"number");
    }
    // Step 8c — ShortStr is a string at the JS surface even though
    // its bits live inline in the AnyValue immediate; report
    // `typeof` as `"string"` BEFORE the cell-pointer branch (which
    // would mis-dispatch to `"object"` via the fall-through arm).
    if is_short_str(v) {
        return alloc_literal(b"string");
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: cell pointer is non-null per is_cell guarantee +
        // caller invariant says it points to a live heap object.
        let tag = unsafe { heap_type_tag(child) };
        let kind: &[u8] = if tag == Tag::Str as u16 {
            b"string"
        } else if tag == Tag::Closure as u16 {
            b"function"
        } else if tag == Tag::DynObj as u16
            && unsafe { heap_flags(child) } & torajs_rc::FLAG_DYNOBJ_CLASS_CTOR != 0
        {
            // A `__class_<C>` class-constructor dynobj — ES models
            // class constructors as function objects (RFC
            // 20260717-class-first-class-value knife A).
            b"function"
        } else if tag == Tag::Symbol as u16 {
            b"symbol"
        } else if tag == Tag::BigInt as u16 {
            b"bigint"
        } else {
            // OBJ / ARR / REGEX / DATE / WEAK* / DYNOBJ / MAP* /
            // ARR_ITER → "object"
            b"object"
        };
        return alloc_literal(kind);
    }
    // Defensive — uninitialized slot (v == 0) reads as "object"
    // (matches `typeof null` per spec).
    alloc_literal(b"object")
}

/// `console.log(v)` single-arg dispatch — NaN-box [`AnyValue`]
/// entry point. Dispatches on the immediate NaN-box predicates.
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_anyv(v: AnyValue) {
    // Fresh console.log line — reset the wrap estimate to column 0
    // (inspect wrap trunk; bun starts each log with a fresh
    // estimated_line_length).
    __torajs_inspect_line_reset(0);
    if is_null(v) {
        write_line(b"null\n");
        return;
    }
    if is_undefined(v) {
        write_line(b"undefined\n");
        return;
    }
    if is_bool(v) {
        // SAFETY: extern fn callable from this no_std-ish module.
        unsafe { print_bool(as_bool(v)) };
        return;
    }
    if is_int32(v) {
        let n = as_int32(v) as i64;
        // SAFETY: as above.
        unsafe { print_i64(n) };
        return;
    }
    if is_double(v) {
        let d = as_double(v);
        // SAFETY: as above.
        unsafe { print_f64(d) };
        return;
    }
    // Step 8c — ShortStr inline-print path. No heap alloc: read the
    // 8-bit length + 5-byte payload off the immediate and dump
    // bytes through `putchar`. Mirrors how `__torajs_str_print`
    // emits Heap+Str bytes, but skips the materialize round-trip
    // entirely (Heap+Str path goes through __torajs_str_print
    // below).
    if is_short_str(v) {
        let len = short_str_len(v) as usize;
        let bytes = short_str_bytes(v);
        for &b in &bytes[..len] {
            // SAFETY: __torajs_io_putc_out takes any i32 byte value.
            unsafe { __torajs_io_putc_out(b as i32) };
        }
        unsafe { __torajs_io_putc_out(b'\n' as i32) };
        return;
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: live heap ptr per caller invariant.
        let tag = unsafe { heap_type_tag(child) };
        if tag == Tag::Str as u16 {
            // Tag::Str covers both real Str (`{len@+8, data@+16}`)
            // AND Substr view (`{len@+8, parent@+16, offset@+24}`).
            // Disambiguate via `FLAG_SUBSTR_VIEW`: set on every Substr
            // (standalone via `__torajs_substr_create`, inline via
            // split-tail emit) so `__torajs_str_print`'s
            // "read inline bytes @ +16" walker doesn't garble the
            // parent-ptr field. The substr-aware printer reads parent
            // + offset and prints the parent's payload slice.
            let flags = unsafe { heap_flags(child) };
            if flags & SUBSTR_VIEW_FLAG != 0 {
                // SAFETY: Substr layout per torajs-str::substr.
                // substr_print writes its own trailing newline.
                unsafe { __torajs_substr_print(child as *const u8) };
            } else {
                // SAFETY: Tag::Str header layout — print walker reads
                // len@+8 and bytes@+16.
                unsafe { __torajs_str_print(child as *const u8) };
            }
        } else if tag == Tag::Arr as u16 {
            // Nested-print substrate trunk Commit 4 — Tag::Arr
            // (typed Arr<T> heap cell) renders as bun's
            // `[ a, b, c ]` form via the Commit 2 walker. Closes
            // `Any-print arr fallback` wedge (`const a:any = [1,2,3]`).
            // SAFETY: Arr heap layout per torajs-arr::layout.
            unsafe { __torajs_arr_print_any(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::DynObj as u16 {
            // Nested-print substrate trunk Commit 4 — Tag::DynObj
            // (object literal / Object.entries row) renders as
            // bun's `{\n  k: v,\n}` form via the Commit 3 walker.
            // Closes `Any-print obj fallback` wedge
            // (`const o:any = {a:1}`). Tag::Obj (static-layout
            // class instance) keeps the `[object]` fallback —
            // class-instance pretty-print requires struct_layouts
            // metadata and is a separate substrate trunk (W-J).
            // SAFETY: dynobj layout per torajs-dynobj::layout.
            unsafe { __torajs_obj_print_any(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::Date as u16 {
            // Commit 5 — Date wire. `put_date_inline` prints the
            // ISO-8601 form unquoted (bun: `1970-01-01T00:00:00.000Z`
            // not `"1970-..."`), or `Invalid Date` for the invalid
            // sentinel.
            // SAFETY: Date layout per torajs-date::layout.
            unsafe { put_date_inline(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::RegExp as u16 {
            // Commit 6 — RegExp wire. Bun prints RegExp values as
            // `/source/flags` (unquoted, both top-level and nested,
            // unlike Str which gains `"..."` inside arr / obj).
            // SAFETY: RegExp layout per torajs-regex::regex.
            unsafe { __torajs_regex_print_inline(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::Promise as u16 {
            // Commit 8 — Promise wire. Emits the bun minimal form
            // `Promise { <pending|resolved|rejected> }` — bun
            // deliberately doesn't surface value / reason in the
            // default console.log inspect.
            // SAFETY: Promise layout per torajs-promise::layout.
            unsafe { __torajs_promise_print(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::Map as u16 {
            // Runtime Tag::Set substrate — Type::Any `console.log(m)`
            // routes here once Tag::Set (=19) split Set from Map.
            // SAFETY: Map layout per torajs-collections::layout.
            unsafe { __torajs_map_print(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::Set as u16 {
            // SAFETY: Set shares the Map layout (same struct, just
            // TAG_SET stamped on the heap header by __torajs_set_create).
            unsafe { __torajs_set_print(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::Closure as u16 {
            // Phase 2 wire (fn-name registry Step 5) — top-level
            // closure print: fn-addr registry lookup, or the
            // interned method name for a reified method cell
            // (chunk 715), + '\n'.
            unsafe { put_closure_fn_name(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::Obj as u16 {
            // W-J Phase D — Tag::Obj struct-cell pretty print. The
            // walker reads class_tag@+8, looks up the link-emitted
            // `__torajs_class_name_table` (W-J A3c chunk 2 substrate)
            // for the `Name {…}` prefix, then walks declared fields
            // via the W-J A4 readers (`__torajs_struct_field_*`).
            // Anonymous / A1-anon-stamp-miss cells fall through to
            // the no-prefix `{…}` form so empty layouts degrade
            // cleanly instead of stalling on `[object]`.
            unsafe { __torajs_anyv_struct_print_inline(v as u64) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::WeakMap as u16 {
            // WeakMap / WeakSet are non-enumerable per spec
            // (§24.4 / §24.5 — no `forEach`, no iterators), so
            // bun's default inspect prints the fixed `WeakMap {}` /
            // `WeakSet {}` form regardless of entry count.
            write_line(b"WeakMap {}\n");
        } else if tag == Tag::WeakSet as u16 {
            write_line(b"WeakSet {}\n");
        } else if tag == Tag::Symbol as u16 {
            // Symbol cell via Any — bun prints `Symbol(<desc>)`.
            // Same inline body as `__torajs_symbol_print` modulo
            // the trailing '\n' (which this top-level dispatcher
            // appends).
            unsafe { __torajs_symbol_print_inline(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::BigInt as u16 {
            // BigInt cell via Any — bun prints `<decimal>n`. The
            // inline helper allocates a temporary decimal Str via
            // `__torajs_bigint_to_string`, writes its bytes,
            // appends `n` and rc_decs the Str.
            unsafe { __torajs_bigint_print_inline(child) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if tag == Tag::SymbolWrapper as u16 {
            // Object(sym) — fixed multi-line block + '\n'
            // (rotation 184).
            unsafe { crate::inspect::wrapper_block::put_symbol_wrapper_at(child, 0) };
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else if unsafe { crate::inspect::formatters::put_wrapper_inline(child, tag) } {
            // Primitive wrapper — `[String: "…"]` form + '\n'.
            unsafe { __torajs_io_putc_out(b'\n' as i32) };
        } else {
            write_line(b"[object]\n");
        }
        return;
    }
    write_line(b"[unknown-any-tag]\n");
}

/// `console.log(...)` multi-arg joiner entry — same NaN-box dispatch
/// as [`__torajs_print_anyv`] (top-level: strings unquoted, matches
/// bun's `console.log("hi")` → `hi`) but emits NO trailing newline.
/// The ssa-lower multi-arg path calls this once per Type::Any arg
/// (and emits the ' ' separator + final '\n' itself); typed Arr<T>
/// args route to `__torajs_arr_print_<T>_inline` (torajs-arr's
/// no-\n typed-walker family) since this entry's `Tag::Arr` arm
/// reads slots as NaN-box AnyValues which only matches Arr<Any>.
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object
/// whose `HeapHeader::type_tag` matches its layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_anyv_inline_top(v: AnyValue) {
    // Per-arg reset (multi-arg joiner). Approximation: bun keeps one
    // estimate across all args of a single console.log; tr resets per
    // arg. Divergence only matters when an earlier arg pushes a later
    // composite arg over the 80-column wrap edge — probe-calibrate if
    // a fixture ever hits it.
    __torajs_inspect_line_reset(0);
    if is_null(v) {
        unsafe { put_bytes(b"null") };
        return;
    }
    if is_undefined(v) {
        unsafe { put_bytes(b"undefined") };
        return;
    }
    if is_bool(v) {
        unsafe { put_bytes(if as_bool(v) { b"true" } else { b"false" }) };
        return;
    }
    if is_int32(v) {
        unsafe { put_i64_inline(as_int32(v) as i64) };
        return;
    }
    if is_double(v) {
        unsafe { put_f64_inline(as_double(v)) };
        return;
    }
    if is_short_str(v) {
        let len = short_str_len(v) as usize;
        let bytes = short_str_bytes(v);
        for &b in &bytes[..len] {
            unsafe { __torajs_io_putc_out(b as i32) };
        }
        return;
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: live heap ptr per caller invariant.
        let tag = unsafe { heap_type_tag(child) };
        if tag == Tag::Str as u16 {
            let flags = unsafe { heap_flags(child) };
            if flags & SUBSTR_VIEW_FLAG != 0 {
                unsafe { put_substr_cell_inline(child) };
            } else {
                unsafe { put_str_cell_inline(child) };
            }
        } else if tag == Tag::Arr as u16 {
            // Arr<Any> only — typed Arr<i64/f64/bool/str/substr> reach
            // this entry only when boxed by the ssa-lower multi-arg
            // path's Any fallback (current path dispatches typed-Arr
            // to torajs-arr's no-\n typed family before boxing).
            unsafe { __torajs_arr_print_any(child) };
        } else if tag == Tag::DynObj as u16 {
            unsafe { __torajs_obj_print_any(child) };
        } else if tag == Tag::Date as u16 {
            unsafe { put_date_inline(child) };
        } else if tag == Tag::RegExp as u16 {
            unsafe { __torajs_regex_print_inline(child) };
        } else if tag == Tag::Promise as u16 {
            unsafe { __torajs_promise_print(child) };
        } else if tag == Tag::Map as u16 {
            unsafe { __torajs_map_print(child) };
        } else if tag == Tag::Set as u16 {
            unsafe { __torajs_set_print(child) };
        } else if tag == Tag::Closure as u16 {
            unsafe { put_closure_fn_name(child) };
        } else if tag == Tag::Obj as u16 {
            unsafe { __torajs_anyv_struct_print_inline(v as u64) };
        } else if tag == Tag::WeakMap as u16 {
            unsafe { put_bytes(b"WeakMap {}") };
        } else if tag == Tag::WeakSet as u16 {
            unsafe { put_bytes(b"WeakSet {}") };
        } else if tag == Tag::Symbol as u16 {
            unsafe { __torajs_symbol_print_inline(child) };
        } else if tag == Tag::BigInt as u16 {
            unsafe { __torajs_bigint_print_inline(child) };
        } else if tag == Tag::SymbolWrapper as u16 {
            // Object(sym) — fixed multi-line block, indent 0 (this
            // dispatcher has no indent thread; rotation 184).
            unsafe { crate::inspect::wrapper_block::put_symbol_wrapper_at(child, 0) };
        } else if unsafe { crate::inspect::formatters::put_wrapper_inline(child, tag) } {
            // Primitive wrapper — bytes emitted by the helper.
        } else {
            unsafe { put_bytes(b"[object]") };
        }
        return;
    }
    unsafe { put_bytes(b"[unknown-any-tag]") };
}

/// Emit the bytes of a Str / Substr heap cell **unquoted** —
/// nested-print substrate trunk Commit 4 helper for dynobj-key /
/// Map-key / Set-elem-string callers that need raw key bytes without
/// the `"..."` wrapper [`super::tag_dispatch::__torajs_print_anyv_inline`]
/// adds for nested-context string values.
///
/// `cell` must point to a Tag::Str heap object (real Str layout
/// `{len@+8, data@+16}` OR Substr view `{len@+8, parent@+16,
/// offset@+24}`); the inspect path picks via `FLAG_SUBSTR_VIEW`.
/// Non-Str tags emit nothing (callers feed Str ptrs from
/// `__torajs_dynobj_iter_key` / Map key slots / etc by contract).
///
/// # Safety
///
/// `cell` must point to a valid Tag::Str heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_str_cell_unquoted(cell: *const c_void) {
    unsafe {
        if cell.is_null() {
            return;
        }
        let tag = heap_type_tag(cell);
        if tag != Tag::Str as u16 {
            return;
        }
        let flags = heap_flags(cell);
        if flags & SUBSTR_VIEW_FLAG != 0 {
            put_substr_cell_inline(cell);
        } else {
            put_str_cell_inline(cell);
        }
    }
}

/// Emit a Str / Substr cell as a quoted, JSON-escaped inspect
/// string — `"..."` with bun's writeJSONString escapes (inspect
/// escape trunk). Quote bytes stay out of the width estimate (bun
/// parity); the payload length is accounted by the cell walkers.
/// Cross-staticlib export for the typed-array printer family
/// (`torajs-arr::print_typed`).
///
/// # Safety
///
/// `cell` must be NULL (no-op) or point to a valid Tag::Str heap
/// object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_str_cell_quoted(cell: *const c_void) {
    unsafe {
        if cell.is_null() {
            return;
        }
        if heap_type_tag(cell) != Tag::Str as u16 {
            return;
        }
        __torajs_io_putc_out(b'"' as i32);
        if heap_flags(cell) & SUBSTR_VIEW_FLAG != 0 {
            put_substr_cell_inline_esc(cell, true);
        } else {
            put_str_cell_inline_esc(cell, true);
        }
        __torajs_io_putc_out(b'"' as i32);
    }
}

/// Emit a Str / Substr cell as an object key — bare when its code
/// units form an ASCII identifier (`[A-Za-z$_][A-Za-z0-9$_]*`, bun
/// `isLatin1Identifier`), otherwise JSON-quoted (`"a-b": 1`,
/// `"0": 2`, `"with space": 5`). Cross-staticlib export for the
/// dynobj walker.
///
/// # Safety
///
/// Same contract as [`__torajs_print_str_cell_quoted`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_str_cell_as_key(cell: *const c_void) {
    unsafe {
        if cell.is_null() {
            return;
        }
        if heap_type_tag(cell) != Tag::Str as u16 {
            return;
        }
        if str_cell_is_bare_key(cell) {
            __torajs_print_str_cell_unquoted(cell);
        } else {
            __torajs_print_str_cell_quoted(cell);
        }
    }
}

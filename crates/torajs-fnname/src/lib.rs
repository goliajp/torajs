//! torajs-fnname — runtime fn-name registry helper.
//!
//! Reads the `__torajs_fn_name_table[]` + `__torajs_fn_name_table_count`
//! rodata globals torajs-link emits at link time (see
//! `crates/torajs-link/src/fn_name_table_layout.rs`) and prints
//! `[Function: <name>]` (no trailing newline) for the input fn body
//! vaddr. The outer caller (`inspect.rs::__torajs_fn_print_outer`,
//! Tag::Closure print paths) appends `\n` / nested-separator bytes.
//!
//! ## Lookup
//!
//! The table is a contiguous `[FnNameTableEntry; N]` array followed
//! by the `u64` count. Lookup is linear: N is typically a few dozen
//! (one entry per top-level `function <name>()` decl), and the table
//! is in `__DATA_CONST` cache-warm immediately after a touched
//! Closure inspect. Binary-search becomes worthwhile only at much
//! larger N — deferred to the same phase that compute_fn_name_table_layout
//! gains a "sort entries by `fn_addr` at link time" pass.
//!
//! ## Anonymous fn
//!
//! When `fn_addr` doesn't appear in the table, the helper emits
//! `[Function]` (bun's anonymous spelling — chunk 797). Top-level
//! `function foo()` decls always land an entry; anonymous arrow /
//! non-binding closures (`console.log(() => {})`) flow to this path
//! because ssa_lower skips them at Pass 2 fn-decl walk.
//!
//! ## Symbol ABI
//!
//! Externs the runtime imports at staticlib link time:
//! - `__torajs_io_putc_out(c: i32) -> i32` from `torajs-io`
//! - `__torajs_fn_name_table` / `__torajs_fn_name_table_count`
//!   from torajs-link's emit pass (registered via
//!   `apply_fn_name_table_overrides`).
//!
//! ## Why `std` (not `no_std`)
//!
//! Same reason `torajs-rc` / `torajs-anyvalue` chose `std`: Layer-1+
//! staticlibs each declaring their own `#[panic_handler]` would
//! conflict on the lang item, and `std` is the cheapest way to
//! satisfy rustc's panic-handler requirement on the rlib build that
//! cargo workspace tests / `cargo build` produce. The user binary
//! never pulls the panic infrastructure because the staticlib has
//! no panic sites; `torajs-panic-runtime` provides the final binary's
//! `#[panic_handler]` instead.

/// One row of the `__torajs_fn_name_table[]` rodata array.
///
/// Mirror of `crate::fn_name_table_layout::FnNameTableEntryLayout`'s
/// emit bytes — see that doc for offsets / chain-fixup semantics.
#[repr(C)]
pub struct FnNameTableEntry {
    /// Final vaddr of the user fn body. Chain-fixup target — dyld
    /// rebases this at load time so it matches the runtime
    /// `fn_addr` argument exactly (both observe the same ASLR slide).
    pub fn_addr: u64,
    /// Final vaddr of the raw name bytes (no leading 16-byte Str
    /// header — RawBytes flavour, see
    /// `crates/torajs-link/src/exec.rs::UserStringEntry`).
    pub name_ptr: *const u8,
    /// ECMA `String.length` of the name (UTF-16 code units, or
    /// Latin-1 bytes per the `StringLiteral::encode_from_str`
    /// branch). For ASCII fn names this also equals `name_ptr`'s
    /// byte length.
    pub name_len: u32,
    /// ES-spec `Function.length` (chunk 716) — the former alignment
    /// pad slot; `build_fn_name_table_payload` writes the arity the
    /// SSA fn-decl walk computed (leading params before the first
    /// default / rest).
    pub arity: u32,
    /// RFC 20260719-fn-tostring-source B3b — final vaddr of the
    /// type-erased fn source text (RawBytes flavour, same channel
    /// as `name_ptr`), or NULL when no user-written source was
    /// recorded for this row (synthesized forwarders / bound
    /// wrappers). Chain-fixup target when non-NULL.
    pub src_ptr: *const u8,
    /// ES `String.length` of the erased source; 0 when `src_ptr`
    /// is NULL.
    pub src_len: u32,
    /// Alignment pad — keeps the entry at 40 bytes (8-aligned so
    /// the array stride matches the emit side's ENTRY_SIZE).
    pub _pad: u32,
}

// `*const u8` makes the entry struct non-`Send`/`Sync`, but the
// table lives in immutable rodata and we never construct one in
// Rust — the chain-fixup pipeline writes the final vaddrs in place.
unsafe impl Sync for FnNameTableEntry {}

unsafe extern "C" {
    /// First entry of the `__torajs_fn_name_table[]` array. The
    /// emit pass writes `count` entries back-to-back starting at
    /// this address (see `fn_name_table_layout::build_fn_name_table_payload`).
    static __torajs_fn_name_table: FnNameTableEntry;
    /// `u64` entry count placed immediately after the entries.
    static __torajs_fn_name_table_count: u64;
    fn __torajs_io_putc_out(c: i32) -> i32;
    // torajs-str — RFC 20260710 C2a: a Nullable fn-typed slot holds
    // NULL (JS null) or the immortal undefined sentinel alongside
    // real fn addresses; print/ToString must not render those as
    // [Function].
    fn __torajs_str_is_undef(p: *const u8) -> i64;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_undef() -> *mut u8;
}

const STR_DATA_OFF: usize = 16;

const PREFIX: &[u8] = b"[Function: ";
const SUFFIX: &[u8] = b"]";
// Chunk 797 — bun prints unregistered fns as `[Function]` (node
// spells `[Function (anonymous)]`; bun is the parity oracle).
const ANON: &[u8] = b"[Function]";
// Chunk 798 — the bind-desugar wrapper registers the ES
// SetFunctionName form `bound <fn>` (§20.2.3.2) so `.name` answers
// it, but bun's inspect prints a bound fn by its TARGET name
// (`console.log(g.bind(null))` → `[Function: g]`). The print faces
// strip the marker; fn idents can't contain spaces, so the prefix
// is unambiguous (registry-synthesized only).
const BOUND_MARK: &[u8] = b"bound ";

/// Strip the `bound ` SetFunctionName marker for the print faces.
///
/// # Safety
/// `name_ptr` points at `name_len` readable bytes.
unsafe fn print_face_name(name_ptr: *const u8, name_len: u32) -> (*const u8, u32) {
    if (name_len as usize) > BOUND_MARK.len() {
        let mut is_bound = true;
        for (j, &b) in BOUND_MARK.iter().enumerate() {
            // Safety: j < BOUND_MARK.len() < name_len.
            if unsafe { *name_ptr.add(j) } != b {
                is_bound = false;
                break;
            }
        }
        if is_bound {
            // Safety: BOUND_MARK.len() < name_len keeps the tail
            // pointer in-bounds with a nonzero remaining length.
            return unsafe {
                (
                    name_ptr.add(BOUND_MARK.len()),
                    name_len - BOUND_MARK.len() as u32,
                )
            };
        }
    }
    (name_ptr, name_len)
}

/// Look up `fn_addr` in `__torajs_fn_name_table[]` and emit either
/// `[Function: <name>]` or `[Function]` to stdout.
/// Does NOT emit a trailing newline — callers
/// (`__torajs_fn_print_outer` for top-level prints, nested arr/obj
/// walkers for inline emit) append their own separator.
///
/// # Safety
///
/// Caller-side invariant: `fn_addr` is a u64 value previously
/// captured at the SSA dispatcher for a `Type::FnSig(_)` operand
/// (top-level fn ref) or a closure cell's `fn_addr@+8` slot. Either
/// way it's an in-image vaddr that, post-dyld-rebase, matches
/// the chain-fixed `fn_addr` in some entry IFF that fn was
/// registered at Pass 2 of ssa_lower. Reads from immutable rodata
/// only.
/// Look up `fn_addr` in `__torajs_fn_name_table[]` — the shared
/// walk behind the print helper and the `.name` / `.length`
/// reflection reads (chunk 716). Hit: returns the raw name bytes
/// pointer and stores the code-unit length / ES-spec arity through
/// the out params. Miss: returns NULL (out params untouched).
///
/// # Safety
/// `fn_addr` is compared, never dereferenced; `out_len` / `out_arity`
/// are valid writable slots. Reads from immutable rodata only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_name_lookup(
    fn_addr: u64,
    out_len: *mut u32,
    out_arity: *mut u32,
) -> *const u8 {
    // Safety: `__torajs_fn_name_table_count` lives in __DATA_CONST
    // rodata after the chain-fixup walk completes.
    let count = unsafe { __torajs_fn_name_table_count };
    let entries_base: *const FnNameTableEntry = &raw const __torajs_fn_name_table;
    let mut i: u64 = 0;
    while i < count {
        // Safety: entries[0..count] live back-to-back in __DATA_CONST
        // per `compute_fn_name_table_layout` so `.add(i)` stays in-bounds.
        let entry = unsafe { &*entries_base.add(i as usize) };
        if entry.fn_addr == fn_addr {
            unsafe {
                *out_len = entry.name_len;
                *out_arity = entry.arity;
            }
            return entry.name_ptr;
        }
        i += 1;
    }
    core::ptr::null()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_print_inline(fn_addr: u64) {
    // RFC 20260710 C2a — a Nullable fn-typed slot's nullish reprs
    // print as their JS values, not as [Function].
    if fn_addr == 0 {
        emit_bytes(b"null");
        return;
    }
    if unsafe { __torajs_str_is_undef(fn_addr as *const u8) } != 0 {
        emit_bytes(b"undefined");
        return;
    }
    let mut name_len: u32 = 0;
    let mut arity: u32 = 0;
    let name_ptr = unsafe { __torajs_fn_name_lookup(fn_addr, &mut name_len, &mut arity) };
    // B3c — anonymous rows (empty name, registered so `src_ptr` /
    // arity can answer) print the same `[Function]` form as a miss.
    let hit = if name_ptr.is_null() || name_len == 0 {
        None
    } else {
        Some((name_ptr, name_len))
    };
    match hit {
        Some((name_ptr, name_len)) => {
            let (name_ptr, name_len) = unsafe { print_face_name(name_ptr, name_len) };
            emit_bytes(PREFIX);
            // Safety: `name_ptr` resolves to `__torajs_str_dyn_<sid>`'s
            // payload byte range; `name_len` is the ECMA String.length
            // (Latin-1 = byte count; UTF-16 = code units = name_len * 2
            // payload bytes — TODO when test262 first lands a non-ASCII
            // fn name; for ASCII it's a byte count and works directly).
            for j in 0..name_len {
                let b = unsafe { *name_ptr.add(j as usize) };
                emit_byte(b);
            }
            emit_bytes(SUFFIX);
        }
        None => {
            emit_bytes(ANON);
        }
    }
}

/// ToString for a fn-typed slot value (RFC 20260710 C2a — the
/// console multi-arg join path). Returns an owned Str:
///
/// - `0` (JS null) → a fresh pooled "null".
/// - the undefined sentinel → the sentinel cell itself (Str-shaped,
///   payload "undefined", `FLAG_STATIC_LITERAL` → the caller's drop
///   is a no-op).
/// - a real fn address → fresh `[Function: <name>]` /
///   `[Function (anonymous)]` per the name-table lookup.
///
/// # Safety
/// `fn_addr` is a fn-typed slot value: 0, the sentinel address, or
/// an in-image code vaddr (compared, never called).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fnsig_to_str(fn_addr: u64) -> *mut u8 {
    if fn_addr == 0 {
        return unsafe { alloc_str(b"null") };
    }
    if unsafe { __torajs_str_is_undef(fn_addr as *const u8) } != 0 {
        return unsafe { __torajs_str_undef() };
    }
    let mut name_len: u32 = 0;
    let mut arity: u32 = 0;
    let name_ptr = unsafe { __torajs_fn_name_lookup(fn_addr, &mut name_len, &mut arity) };
    // B3c — empty-name rows keep the anonymous ToString form.
    if name_ptr.is_null() || name_len == 0 {
        return unsafe { alloc_str(ANON) };
    }
    let (name_ptr, name_len) = unsafe { print_face_name(name_ptr, name_len) };
    let total = PREFIX.len() + name_len as usize + SUFFIX.len();
    let s = unsafe { __torajs_str_alloc_pooled(total as u64) };
    let mut dst = unsafe { s.add(STR_DATA_OFF) };
    for &b in PREFIX {
        unsafe {
            *dst = b;
            dst = dst.add(1);
        }
    }
    for j in 0..name_len {
        unsafe {
            *dst = *name_ptr.add(j as usize);
            dst = dst.add(1);
        }
    }
    for &b in SUFFIX {
        unsafe {
            *dst = b;
            dst = dst.add(1);
        }
    }
    s
}

/// `.name` read on a `Type::FnSig` receiver (chunk 798 — the
/// typed-tier registry rewire). The receiver is a raw fn body vaddr
/// (no closure cell to consult), so the answer is exactly the
/// registry row: hit mints a fresh owned Str with the name bytes,
/// miss answers the ES anonymous-function name `""`. `0` / the
/// undefined sentinel never match a table row and fall to the same
/// empty-Str miss path.
///
/// # Safety
/// `fn_addr` is compared, never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_name_str(fn_addr: u64) -> *mut u8 {
    let mut name_len: u32 = 0;
    let mut arity: u32 = 0;
    let name_ptr = unsafe { __torajs_fn_name_lookup(fn_addr, &mut name_len, &mut arity) };
    if name_ptr.is_null() {
        return unsafe { alloc_str(b"") };
    }
    // Safety: hit rows point at `name_len` readable rodata bytes
    // (ASCII byte count; the non-ASCII UTF-16 payload face shares
    // the fn-print helper's recorded TODO).
    unsafe { alloc_str(core::slice::from_raw_parts(name_ptr, name_len as usize)) }
}

// RFC 20260719-fn-tostring-source B4 — the native-form fallback for
// registry rows with no recorded source (synthesized forwarders /
// bound wrappers) and for unregistered addresses. Matches bun's
// JSC form: 4-space indent, no name on the anonymous shape.
const NATIVE_PREFIX: &[u8] = b"function ";
const NATIVE_SUFFIX: &[u8] = b"() {\n    [native code]\n}";

/// Walk `__torajs_fn_name_table[]` for `fn_addr`'s row and answer
/// the raw erased-source bytes pointer (storing the code-unit length
/// through `out_len`), or NULL when the row is absent or carries no
/// recorded source.
///
/// # Safety
/// `fn_addr` is compared, never dereferenced; `out_len` is a valid
/// writable slot. Reads from immutable rodata only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_source_lookup(fn_addr: u64, out_len: *mut u32) -> *const u8 {
    let count = unsafe { __torajs_fn_name_table_count };
    let entries_base: *const FnNameTableEntry = &raw const __torajs_fn_name_table;
    let mut i: u64 = 0;
    while i < count {
        // Safety: entries[0..count] live back-to-back in __DATA_CONST
        // per `compute_fn_name_table_layout` so `.add(i)` stays in-bounds.
        let entry = unsafe { &*entries_base.add(i as usize) };
        if entry.fn_addr == fn_addr {
            if entry.src_ptr.is_null() {
                return core::ptr::null();
            }
            unsafe { *out_len = entry.src_len };
            return entry.src_ptr;
        }
        i += 1;
    }
    core::ptr::null()
}

/// `Function.prototype.toString` kernel — mint an owned Str holding
/// the row's type-erased source text, or the JSC native form
/// `function <name>() {\n    [native code]\n}` when no source is
/// recorded (name comes from the same row; anonymous/absent rows
/// leave it empty per bun's anonymous native shape).
///
/// # Safety
/// `fn_addr` is compared, never dereferenced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_source_str(fn_addr: u64) -> *mut u8 {
    // RFC 20260710 C2a mirror (fnsig_to_str) — a Nullable fn-typed
    // slot's nullish reprs stringify as their JS values.
    if fn_addr == 0 {
        return unsafe { alloc_str(b"null") };
    }
    if unsafe { __torajs_str_is_undef(fn_addr as *const u8) } != 0 {
        return unsafe { __torajs_str_undef() };
    }
    let mut src_len: u32 = 0;
    let src_ptr = unsafe { __torajs_fn_source_lookup(fn_addr, &mut src_len) };
    if !src_ptr.is_null() {
        // Safety: hit rows point at `src_len` readable rodata bytes
        // (ASCII byte count; non-ASCII UTF-16 sources share the
        // fn-print helper's recorded TODO).
        return unsafe { alloc_str(core::slice::from_raw_parts(src_ptr, src_len as usize)) };
    }
    // Native form — pull the name from the name face, stripping the
    // `bound ` SetFunctionName marker like the print faces: bun
    // (JSC) spells a bound fn's toString with the TARGET name
    // (`function add() {\n    [native code]\n}` — probe 2026-07-19).
    let mut name_len: u32 = 0;
    let mut arity: u32 = 0;
    let name_ptr = unsafe { __torajs_fn_name_lookup(fn_addr, &mut name_len, &mut arity) };
    let (name_ptr, name_len) = if name_ptr.is_null() {
        (core::ptr::null(), 0)
    } else {
        unsafe { print_face_name(name_ptr, name_len) }
    };
    unsafe { __torajs_fn_native_form_str(name_ptr, name_len) }
}

/// Mint the JSC native ToString form
/// `function <name>() {\n    [native code]\n}` (anonymous when
/// `name_ptr` is NULL / `name_len` is 0). Shared by the registry
/// fallback above and the anyvalue reified-builtin-method-cell
/// toString arm, which carries its own interned name.
///
/// # Safety
/// `name_ptr` is NULL or points at `name_len` readable ASCII bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fn_native_form_str(
    name_ptr: *const u8,
    name_len: u32,
) -> *mut u8 {
    let name_len = if name_ptr.is_null() { 0 } else { name_len };
    let total = NATIVE_PREFIX.len() + name_len as usize + NATIVE_SUFFIX.len();
    let s = unsafe { __torajs_str_alloc_pooled(total as u64) };
    let mut dst = unsafe { s.add(STR_DATA_OFF) };
    unsafe {
        core::ptr::copy_nonoverlapping(NATIVE_PREFIX.as_ptr(), dst, NATIVE_PREFIX.len());
        dst = dst.add(NATIVE_PREFIX.len());
        if name_len > 0 {
            core::ptr::copy_nonoverlapping(name_ptr, dst, name_len as usize);
            dst = dst.add(name_len as usize);
        }
        core::ptr::copy_nonoverlapping(NATIVE_SUFFIX.as_ptr(), dst, NATIVE_SUFFIX.len());
    }
    s
}

/// Fresh pooled Str holding `bytes` (Latin-1/ASCII payloads only).
unsafe fn alloc_str(bytes: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(bytes.len() as u64) };
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), s.add(STR_DATA_OFF), bytes.len()) };
    s
}

#[inline(always)]
fn emit_byte(b: u8) {
    // Safety: `__torajs_io_putc_out` is a no-libc helper from
    // torajs-io with putchar-compatible signature.
    unsafe {
        __torajs_io_putc_out(b as i32);
    }
}

#[inline(always)]
fn emit_bytes(s: &[u8]) {
    for &b in s {
        emit_byte(b);
    }
}

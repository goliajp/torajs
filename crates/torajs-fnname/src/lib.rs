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
//! `[Function (anonymous)]`. Top-level `function foo()` decls always
//! land an entry; anonymous arrow / non-binding closures
//! (`console.log(() => {})`) flow to this path because ssa_lower
//! skips them at Pass 2 fn-decl walk.
//!
//! ## Symbol ABI
//!
//! Externs the runtime imports at staticlib link time:
//! - `__torajs_io_putc_stdout(c: i32) -> i32` from `torajs-io`
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
    fn __torajs_io_putc_stdout(c: i32) -> i32;
}

const PREFIX: &[u8] = b"[Function: ";
const SUFFIX: &[u8] = b"]";
const ANON: &[u8] = b"[Function (anonymous)]";

/// Look up `fn_addr` in `__torajs_fn_name_table[]` and emit either
/// `[Function: <name>]` or `[Function (anonymous)]` to stdout.
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
    let mut name_len: u32 = 0;
    let mut arity: u32 = 0;
    let name_ptr = unsafe { __torajs_fn_name_lookup(fn_addr, &mut name_len, &mut arity) };
    let hit = if name_ptr.is_null() {
        None
    } else {
        Some((name_ptr, name_len))
    };
    match hit {
        Some((name_ptr, name_len)) => {
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

#[inline(always)]
fn emit_byte(b: u8) {
    // Safety: `__torajs_io_putc_stdout` is a no-libc helper from
    // torajs-io with putchar-compatible signature.
    unsafe {
        __torajs_io_putc_stdout(b as i32);
    }
}

#[inline(always)]
fn emit_bytes(s: &[u8]) {
    for &b in s {
        emit_byte(b);
    }
}

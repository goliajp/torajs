//! Multi-line wrapper inspect blocks — `Object(sym)` for now.
//!
//! bun expands a SymbolWrapper cell as a fixed four-field block
//! (probed 2026-07-22, rotation 184):
//!
//! ```text
//! Symbol {
//!   description: "hi",
//!   toString: [Function: toString],
//!   valueOf: [Function: valueOf],
//!   [Symbol(Symbol.toPrimitive)]: [Function: [Symbol.toPrimitive]],
//! }
//! ```
//!
//! `description` is the wrapped symbol's [[Description]] (quoted,
//! JSON-escaped) or `undefined`; the three method rows are the
//! reified `Symbol.prototype` surface and print as fixed text.
//! Fields pad at `indent + 2`, the closer at `indent` — the uniform
//! bun indent model the Arr / DynObj walkers follow. Sibling file
//! to `formatters.rs` (that file is a size-debt entry and may not
//! grow).

use core::ffi::c_void;

use super::formatters::{
    __torajs_inspect_line_add, __torajs_inspect_line_reset, put_byte, put_bytes,
    put_str_cell_inline_esc,
};

/// Emit the SymbolWrapper block at `indent`. No trailing '\n' —
/// the caller owns separators (inline contract).
///
/// # Safety
///
/// `child` is a live SymbolWrapper cell (`[header:8][*Symbol:8]`);
/// the wrapped symbol pointer is live and non-null per the mint
/// path (`Object(sym)` always stores the symbol cell).
pub(super) unsafe fn put_symbol_wrapper_at(child: *const c_void, indent: u32) {
    unsafe {
        let sym = ((child as *const u8).add(8) as *const *const c_void).read();
        put_bytes(b"Symbol {\n");
        // Same estimate handling as the dynobj walker's
        // handleFirstProperty mirror: reset to parent-indent + 1 on
        // entering the block; the estimate only gates nested wrap
        // decisions.
        __torajs_inspect_line_reset(indent + 1);
        put_pad(indent + 2);
        put_bytes(b"description: ");
        let desc = if sym.is_null() {
            core::ptr::null_mut()
        } else {
            crate::member_get_layout::symbol_desc(sym)
        };
        if desc.is_null() {
            put_bytes(b"undefined");
            __torajs_inspect_line_add(9);
        } else {
            put_byte(b'"');
            put_str_cell_inline_esc(desc, true);
            put_byte(b'"');
        }
        put_bytes(b",\n");
        put_pad(indent + 2);
        put_bytes(b"toString: [Function: toString],\n");
        put_pad(indent + 2);
        put_bytes(b"valueOf: [Function: valueOf],\n");
        put_pad(indent + 2);
        put_bytes(b"[Symbol(Symbol.toPrimitive)]: [Function: [Symbol.toPrimitive]],\n");
        put_pad(indent);
        put_byte(b'}');
        __torajs_inspect_line_add(1);
    }
}

#[inline]
unsafe fn put_pad(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}

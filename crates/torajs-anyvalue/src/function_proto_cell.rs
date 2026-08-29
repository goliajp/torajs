//! %Function.prototype% is a built-in FUNCTION object (§20.2.3), not
//! an ordinary one.
//!
//! tr minted it as a plain dynobj like every other `<Ctor>.prototype`,
//! and one wrong cell tag is all it takes for three answers to go
//! wrong together: `typeof Function.prototype` said "object",
//! `Function.prototype()` threw instead of answering undefined, and
//! `Function.prototype.toString()` fell through to the badge
//! ("[object Function]") because §20.2.3.5's source-text lane does not
//! recognise a dynobj. Fixing any one of those where it shows would
//! have been three emulations of the same missing fact.
//!
//! §20.2.3: "accepts any arguments and returns undefined". It has no
//! [[Construct]], no `prototype` property, and its `name` is the empty
//! string with `length` 0 — which is what the immortal reject-closure
//! mint already gives a cell that interns no method id.
//!
//! `Array.prototype` is the precedent: §23.1.3 makes it an Array
//! exotic object, so its slot holds a real Arr cell and every consumer
//! routes on the heap tag. This is the same move for the one other
//! prototype the spec does not make ordinary.

use core::ffi::c_void;

use crate::method_value::mint_reject_closure_cell;
use crate::nanbox::VALUE_UNDEFINED;

/// A closure cell's expando slot — `member_get_layout::
/// CLOSURE_PROPS_OFF` mirror.
const CLOSURE_PROPS_OFF: usize = 24;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
}

/// §20.2.3 [[Call]] — every argument ignored, always undefined.
unsafe extern "C" fn call_entry(_env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    VALUE_UNDEFINED
}

/// The %Function.prototype% singleton's cell, for the builtin-proto
/// mint (`torajs_rc::builtin_proto`) to install under tag 13. Called
/// once per process, on the slot's first materialization; a CAS loser
/// leaks it exactly as the dynobj mint's loser does.
///
/// The expando is allocated up front rather than lazily: §10.2.4's
/// `caller` / `arguments` accessors are installed immediately after,
/// and the narrow define kernel they go through contracts for a plain
/// dynobj receiver (`mint_symbol_method_cell` does the same for the
/// same reason). Handing it the closure cell instead wrote entry
/// records over `fn_addr` and the expando slot itself.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_function_proto_alloc() -> *mut c_void {
    let cell = mint_reject_closure_cell(call_entry);
    // SAFETY: fresh cell from the mint above; both slots are its own.
    unsafe {
        // The mint marks its cells FLAG_STATIC_LITERAL for
        // rc-immortality, and that flag carries `.rodata`
        // conceptual-immutability with it: freeze becomes a no-op,
        // `isFrozen` answers true, `preventExtensions` does nothing.
        // The builtin-proto mint site records the same trap for the
        // dynobj singletons — a prototype is an ordinary mutable
        // object and must answer as one. Like those, this cell is a
        // plain mortal one; the `<Ctor>.prototype` lowering takes its
        // own stake on the borrowed slot pointer.
        *(cell.add(6) as *mut u16) &= !torajs_rc::FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut *mut c_void) = __torajs_dynobj_alloc();
    }
    cell as *mut c_void
}

/// The IN-CELL expando slot the §10.2.4 install writes through, for
/// `torajs-meta`'s side of the boundary.
///
/// A pointer to the slot, not the table it holds: the define kernel
/// relocates a growing entry table and writes the new address back
/// through the slot it was handed. Handed a local copy, the second
/// accessor's growth left the cell pointing at the freed table — and
/// `f.caller` stopped throwing, because the entries it has to find
/// were no longer where the cell said they were.
///
/// # Safety
/// `proto` is the %Function.prototype% cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_function_proto_props_slot(
    proto: *mut c_void,
) -> *mut *mut c_void {
    unsafe { proto.cast::<u8>().add(CLOSURE_PROPS_OFF) as *mut *mut c_void }
}

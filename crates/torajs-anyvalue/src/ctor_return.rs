//! RFC 20260820-ctor-return-override — the §10.2.2 [[Construct]]
//! step 13 pick, plus the field carry that keeps a subclass's own
//! elements attached when a base constructor hands back somebody
//! else's object.
//!
//! Only classes named by `Ast::ctor_return_override` reach these; see
//! that pass for why the set is narrow. The desugar shape they serve:
//!
//! ```text
//! __cm_C__ctor(__this_in: any, __new_target: any, …): any {
//!   let __this: any = __this_in;      // ordinary local; `this` resolves here
//!   ( __this = __torajs_ctor_ret_value(__this, __cm_P__ctor(__this, …)),
//!     __torajs_ctor_ret_carry(__this_in, __this, "<own field>"),
//!     __this );                       // the rewritten `super(…)`
//!   return __this;
//! }
//! ```
//!
//! One kernel serves both step-13 sites because they ask the same
//! question: at the super site, "did the parent hand me a different
//! object?", and at the factory, "does the body's answer replace what
//! I minted?" — both are "an object wins, anything else leaves the
//! incumbent standing".
//!
//! Nothing rewrites the constructor's own `return` statements. The
//! tail gets `return __this`, so falling off the end answers the
//! current `this`, a written `return;` answers undefined, and a
//! written `return <expr>` answers it raw — and the pick at the
//! factory maps all three correctly on its own.
//!
//! `__this_in` keeps naming the object the factory minted no matter
//! what `__this` is reassigned to, which is what lets the carry find
//! the fields the mint already initialized without a second local.
//!
//! **Ownership**: the pick is BORROW-shaped — it hands back one of
//! its operands' bits without touching a refcount, the same posture
//! `member_get`'s probe pair keeps. That is the convention the rest
//! of the lowering is built on: an expression yields a view, and
//! whoever consumes it (an assignment, a `let`, a return) takes the
//! stake it needs. Retaining here instead leaked one cell per
//! construction — the assignment at the super site retains what it
//! stores, so the kernel's extra +1 had no matching release (a churn
//! probe read 26 MB against a 6.6 MB baseline; replacing the pick
//! with a bare assignment flattened it, which is what named the
//! retain as the culprit).

use crate::member_get_layout::recv_cell;
use crate::nanbox::AnyValue;
use core::ffi::c_void;
use torajs_rc::{AnySlotTag, Tag};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
}

/// Whether a value is an Object for step 13's purposes.
///
/// Deliberately NOT `in_op_any::require_object_rhs`: that one raises
/// the `in` operator's TypeError on a miss, and step 13 has no throw
/// here — a constructor answering a primitive simply leaves `this` in
/// place. The taxonomy is the same one it uses: everything that
/// reaches a heap cell is an object except the three heap-resident
/// primitives, and every immediate (undefined, null, number, boolean,
/// short string) is not.
fn is_object(v: AnyValue) -> bool {
    match recv_cell(v) {
        Some((_, tag)) => {
            tag != Tag::Str as u16 && tag != Tag::Symbol as u16 && tag != Tag::BigInt as u16
        }
        None => false,
    }
}

/// §10.2.2 step 13: an object answers itself, anything else leaves
/// the incumbent `this` standing. Used at both step-13 sites — the
/// `super(…)` answer taking over as `this`, and the factory reading
/// what the constructor answered.
///
/// Recorded boundary — for a DERIVED class step 13.b makes a
/// non-undefined non-object a TypeError rather than a fallback. This
/// kernel takes the base-class branch for both, which is what the
/// pre-RFC shape did for every class (it dropped the return outright),
/// so nothing regresses; the missing throw is on the RFC's boundary
/// list.
///
/// # Safety
/// Both operands are live AnyValue bit patterns the caller keeps
/// alive across the call. The answer is a BORROW of one of them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_ret_value(this_v: AnyValue, v: AnyValue) -> AnyValue {
    if is_object(v) { v } else { this_v }
}

/// Move one of the class's own declared elements onto the object the
/// super call handed back, so the instance carries the brand its
/// class installed (§7.3.28 InitializeInstanceElements — the derived
/// class's fields belong on whatever `this` became).
///
/// A no-op when nothing was adopted, which is the common path: the
/// mint already holds the field.
///
/// The install goes through the BASE member-set channel on purpose.
/// The private write channel next door (`member_set_private`) gates
/// on the brand being declared already, which is exactly right for a
/// program writing `o.#f` and exactly wrong here — this call is the
/// declaration.
///
/// Only the class's OWN elements are carried, one call per name, not
/// every own key of the mint: tr flattens the whole chain's fields
/// into one literal, and an ANCESTOR's fields do not belong on the
/// object its own constructor chose to return instead (they were
/// installed on the `this` it walked away from, per spec).
///
/// # Safety
/// `minted` / `target` are live AnyValue bit patterns; `key` is a
/// live Str cell. Borrows all three.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_ret_carry(
    minted: AnyValue,
    target: AnyValue,
    key: *mut c_void,
) {
    if minted == target || !is_object(target) {
        return;
    }
    // Ownership mirrors `obj_assign::copy_keys`: the probe pair is
    // borrow-shaped, so a heap payload takes +1 before it meets
    // member_set's consume contract. The mint keeps its own stake and
    // releases it when it is dropped.
    let tag = unsafe { crate::member_get::__torajs_any_member_get_tag(minted, key) };
    let value = unsafe { crate::member_get_value::__torajs_any_member_get_value(minted, key) };
    if tag == AnySlotTag::Heap as u64 && value != 0 {
        unsafe { __torajs_rc_inc(value as *mut c_void) };
    }
    let mut slot = target;
    unsafe { crate::member_set::__torajs_any_member_set(&mut slot, key, tag, value, -1) };
}

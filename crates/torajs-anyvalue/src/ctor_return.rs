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
//!   let __sup: any = undefined;
//!   ( __sup = __cm_P__ctor(__this, …),
//!     __this = __torajs_ctor_ret_value(__this, __sup),
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
//! **Ownership**: the pick is BORROW-shaped, and both call sites
//! consume it by ASSIGNING it into a slot. Those two facts are one
//! decision, because the SSA treats the two consumers differently:
//! `x = <call>` retains what it stores, while `let x = <call>` takes
//! the result over without one. So a borrow-shaped answer is right
//! under an assignment and dangles under a `let`, and an owned answer
//! is right under a `let` and leaks under an assignment. Both
//! mistakes were measured, and each is invisible to the probe that
//! catches the other: the dangling one read a fresh instance back as
//! a Str while memory stayed flat, and the leaking one stayed
//! perfectly correct while spending 65 B per construction.
//!
//! For the same reason the parent constructor's answer lands in a
//! `__sup` LOCAL first. A call result handed straight to another call
//! as an argument gets no release at all — that was a third leak, of
//! the same 65 B, sitting underneath the first two.

use crate::member_get_layout::recv_cell;
use crate::nanbox::AnyValue;
use core::ffi::c_void;
use torajs_rc::{AnySlotTag, Tag};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `nanbox` immediate for `undefined` — step 13.c's one exemption.
const VALUE_UNDEFINED: AnyValue = crate::nanbox::VALUE_UNDEFINED;

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

/// §10.2.2 step 13: an object answers itself; for a base class
/// anything else leaves the incumbent `this` standing; for a DERIVED
/// class only `undefined` may do so, and any other non-object is a
/// TypeError (step 13.c).
///
/// Used at both step-13 sites, and `derived` names a different class
/// at each. At the factory it is the class being constructed. At a
/// `super(…)` site it is the PARENT — that call is where the parent's
/// [[Construct]] step 13 happens, because tr's `super(…)` reaches
/// `__cm_<P>__ctor` directly and never goes through P's own factory.
///
/// A throw still answers the incumbent: the caller's throw check is
/// what ends the path, and handing back a live object keeps the
/// intervening drops well-formed.
///
/// # Safety
/// Both operands are live AnyValue bit patterns the caller keeps
/// alive across the call. The answer is a BORROW of one of them, and
/// must be consumed by an assignment (see the module doc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_ret_value(
    this_v: AnyValue,
    v: AnyValue,
    derived: i64,
) -> AnyValue {
    if is_object(v) {
        return v;
    }
    if derived != 0 && v != VALUE_UNDEFINED {
        unsafe {
            __torajs_throw_type_error(
                c"derived constructor may only return an object or undefined".as_ptr(),
            );
        }
    }
    this_v
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

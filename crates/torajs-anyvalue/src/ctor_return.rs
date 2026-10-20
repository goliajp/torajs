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
//! **Ownership**: the answer is OWNED. The factory relays the pick into
//! `__picked` and returns it, and a borrow there let the factory's exit
//! release the mint to zero and return the freed cell (rotation 660:
//! over a pooled Obj the residue still read right, so neither the gate
//! nor Guard Malloc saw it; an Array / Promise subclass mint lost its
//! subclass registration in the drop and `instanceof` answered false,
//! and a primitive-returning ctor's instance read back as a torn
//! object). The `super(…)` site assigns the pick into `__this`, an
//! assignment in a comma that is not the last: the slot takes the
//! pick's own reference, and the stake the assignment mints for its
//! value is released when the comma discards it. The AOT RSS churn
//! probe catches a leak there, and only the behaviour probe catches a
//! dangle — both are needed.
//!
//! For the same reason the parent constructor's answer lands in a
//! `__sup` LOCAL first. A call result handed straight to another call
//! as an argument gets no release at all.

use crate::member_get_layout::recv_cell;
use crate::nanbox::AnyValue;
use core::ffi::c_void;
use torajs_rc::{AnySlotTag, Tag};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
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
/// alive across the call. The answer is OWNED (a +1 stake; see the
/// module doc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_ret_value(
    this_v: AnyValue,
    v: AnyValue,
    derived: i64,
) -> AnyValue {
    let picked = if is_object(v) {
        v
    } else {
        if derived != 0 && v != VALUE_UNDEFINED {
            unsafe {
                __torajs_throw_type_error(
                    c"derived constructor may only return an object or undefined".as_ptr(),
                );
            }
        }
        this_v
    };
    // A pending throw (the constructor's, or step 13.c's just above)
    // ends the path at the caller's throw check, which never releases
    // this answer — so it takes no stake then.
    if unsafe { __torajs_throw_check() } == 0 {
        // SAFETY: `picked` is one of the two live operands.
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_inc(picked) };
    }
    picked
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
    let wtf8 = unsafe { torajs_rc::str_wtf8::StrWtf8::of(key) };
    let name = wtf8.as_bytes();
    // RFC 20260916-private-element-brand slice 2c — a class's private
    // method brand (`__privbrand_<C>`, no value) and its private fields
    // go into the invisible per-name store, where adding one twice is
    // the TypeError; never onto the object's own property face.
    if name.starts_with(b"__privbrand_") {
        // The brand holds the prototype the class's private methods and
        // accessors live on, so they resolve for the adopted object too.
        let proto = unsafe { __torajs_anyv_get_proto_of_any(minted) };
        unsafe { crate::private_foreign::foreign_add(target, key, proto) };
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(proto) };
        return;
    }
    // Ownership mirrors `obj_assign::copy_keys`: the probe pair is
    // borrow-shaped, so a heap payload takes +1 before it meets
    // member_set's consume contract. The mint keeps its own stake and
    // releases it when it is dropped.
    let tag = unsafe { crate::member_get::__torajs_any_member_get_tag(minted, key) };
    let value = unsafe { crate::member_get_value::__torajs_any_member_get_value(minted, key) };
    if name.starts_with(b"__priv_") {
        // An instance whose own class declared the field already holds
        // it in its layout: adding it again is the same TypeError.
        if unsafe { crate::member_get_private::priv_brand_declared(target, key) } {
            unsafe {
                __torajs_throw_type_error(
                    c"cannot add a private element twice to the same object".as_ptr(),
                )
            };
            return;
        }
        // The store takes its own stake; the pair stays a borrow.
        let v =
            unsafe { crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64) };
        unsafe { crate::private_foreign::foreign_add(target, key, v) };
        return;
    }
    if tag == AnySlotTag::Heap as u64 && value != 0 {
        unsafe { __torajs_rc_inc(value as *mut c_void) };
    }
    let mut slot = target;
    unsafe { crate::member_set::__torajs_any_member_set(&mut slot, key, tag, value, -1) };
}

//! The catch binding: the pending throw taken into the parameter, boxed
//! for an `any` parameter, or into a typed one after a check that the
//! thrown value is a value of its type; released for a `catch {}`.
//!
//! The pending slot is shared by every raise in the program, and a
//! `try` body can raise values its static throw sites never wrote: a
//! runtime TypeError out of a member read is an Error cell, an `await`
//! hands in whatever the promise rejected with, and reading a cell's
//! pointer as a number printed the address. So a typed binding reads
//! the slot's tag first and admits only what the parameter's type can
//! hold:
//!
//! | parameter | admitted |
//! |---|---|
//! | `number` | tag F64, or I64 widened |
//! | `boolean` | tag Bool |
//! | `string` | a heap Str cell |
//! | a declared non-generic class | a heap Obj cell whose class tag is the class or a subclass |
//! | any other object type, an array, a heap family by tag | what the checked cast of an annotated slot admits (`ssa_lower_checked_cast`) |
//! | a function type | any function object, its calls through the boxed entry |
//! | `null`, `undefined`, a function type with `null` or `undefined` | that value, or any function object for the last (`nullish`) |
//! | anything else | the value as it is, in an `any` slot |
//!
//! A `void` parameter binds as `any`, and an iterator annotation
//! (`Iterator<T>`) is an `any` slot already.
//!
//! A written `number` parameter binds at f64 width: the thrown value
//! may be any number, an `await` or an `any` throw among them, and an
//! integer slot cannot hold 2.5. Anything not admitted is a TypeError
//! raised where the binding is made, the way an annotated slot rejects
//! a value of another type; the thrown value is released first, and the
//! error routes like any throw out of the catch. The catch body never
//! runs on a value its type does not describe, and the binding never
//! falls back to `any`: that copy of the body would pull the dynamic
//! runtime into every typed catch.
//!
//! Under a type the compiler inferred (`ast_throw_info::catch_types`,
//! a number, a boolean or a declared class, from a try body whose every
//! raise is a `throw` of that type) the slot follows the width table
//! like a throw site does, and a mismatch means the inference missed a
//! raise: the program stops there with the TypeError reported as
//! uncaught, and the binding releases nothing (a release of a value of
//! unknown type links every destructor in the runtime, which a program
//! that stops does not need).

mod cast;
mod ir;
mod nullish;

use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use ir::{and, append, call, decode_f64, guard, icmp, or};

const TAG_NULL: i64 = 0;
const TAG_BOOL: i64 = 1;
const TAG_I64: i64 = 2;
const TAG_F64: i64 = 3;
const TAG_HEAP: i64 = 4;
const TAG_UNDEF: i64 = 5;

/// The parameter `p`'s type under the annotation `ann` (`any` when
/// there is none; one the compiler `inferred`) and the value it binds.
pub(super) fn take(
    ctx: &mut LowerCtx,
    p: &str,
    ann: Option<&str>,
    inferred: bool,
) -> (Type, Operand) {
    let Some(ann) = ann else {
        return (Type::Any, take_any(ctx, p));
    };
    let e_ty = crate::ssa_lower_parse_type::parse_type(
        Some(ann),
        ctx.aliases,
        ctx.arr_layouts,
        ctx.fn_sigs,
        ctx.generic_struct_decls,
        ctx.struct_layouts,
        ctx.inst_memo,
    );
    // an inferred `number` holds only what the throw sites wrote, at the
    // width they wrote it (RFC 20260726-array-elem-width)
    let thrown_f64 = || (ctx.num_f64_slots).slot_is_f64(&crate::num_width::SlotKey::Thrown);
    let e_ty = match e_ty {
        Type::I64 if ann.trim() == "number" && (!inferred || thrown_f64()) => Type::F64,
        // the pending slot holds a function as the cell it is, never a
        // bare code address: the slot is a closure's, as a `let` of a
        // function type with an `any` initializer re-reprs
        Type::FnSig(sig) => Type::Closure(sig),
        t => t,
    };
    // a `void` slot holds no value to compare, and `undefined` is one;
    // an inferred class the tag alone cannot decide (a generic half)
    // stays `any`: the cast route is a written slot's
    if matches!(e_ty, Type::Any | Type::Void) || (inferred && cast::by_cast(ctx, e_ty, ann.trim()))
    {
        return (Type::Any, take_any(ctx, p));
    }
    if let Some(slot) = nullish::slot(e_ty, ann.trim()) {
        return (Type::Any, nullish::take(ctx, p, ann.trim(), slot));
    }
    // a slot no test and no cast decides holds the value boxed, in an
    // `any` slot: its own storage may have no room for it
    if !tested(e_ty) && !cast::by_cast(ctx, e_ty, ann.trim()) {
        return (Type::Any, take_any(ctx, p));
    }
    (e_ty, take_typed(ctx, p, (e_ty, ann.trim()), inferred))
}

/// Box the pending throw for an `any` catch parameter `p`.
fn take_any(ctx: &mut LowerCtx, p: &str) -> Operand {
    let tag_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take, vec![]),
        Type::I64,
        Some(p),
    );
    let boxed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag_v), Operand::Value(val_v)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(boxed)
}

/// Release the pending throw of a catch that binds nothing.
pub(super) fn release_unbound(ctx: &mut LowerCtx) {
    // Unbound `catch {}` still owns the taken value — take
    // (tag, value), box, and drop it. Pre-fix this arm only
    // cleared the active flag via a discarded throw_take,
    // stranding the whole thrown heap payload (Error inst +
    // message + stack) per catch.
    let tag_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take_tag, vec![]),
        Type::I64,
        None,
    );
    let val_v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.throw_take, vec![]),
        Type::I64,
        None,
    );
    let boxed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag_v), Operand::Value(val_v)],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(Operand::Value(boxed), Type::Any);
}

/// Take the pending throw into a value of `e_ty`, the type `ann`
/// names, for the catch parameter `p`.
fn take_typed(ctx: &mut LowerCtx, p: &str, (e_ty, ann): (Type, &str), inferred: bool) -> Operand {
    let (take_tag, take) = (ctx.intrinsics.throw_take_tag, ctx.intrinsics.throw_take);
    let tag = call(ctx, take_tag, None);
    let v = call(ctx, take, Some(p));
    if cast::by_cast(ctx, e_ty, ann) {
        return cast::take_cast(ctx, p, (tag, v), (e_ty, ann));
    }
    let bad = ctx.f.add_block();
    admit(ctx, e_ty, ann, tag, v, bad);
    let ok = ctx.cur_block;
    ctx.cur_block = bad;
    if inferred {
        missed(ctx, p, ann);
    } else {
        reject(ctx, p, ann, (tag, v));
    }
    ctx.cur_block = ok;
    match e_ty {
        // The slot holds raw 8 bytes; decode them the way the throw
        // site encoded them rather than converting the number (the
        // symmetric `BitCastF64ToI64` the promise value point uses).
        // An I64 tag comes from a throw of an integer.
        Type::F64 => Operand::Value(decode_f64(ctx, tag, v)),
        _ => Operand::Value(v),
    }
}

/// Whether `admit` tests a value of `e_ty` by its tag.
fn tested(e_ty: Type) -> bool {
    matches!(
        e_ty,
        Type::I64 | Type::F64 | Type::Bool | Type::Str | Type::Obj(_)
    )
}

/// Branch to `bad` unless the thrown `(tag, v)` is a value of `e_ty`;
/// leaves the cursor in the block that continues with an admitted
/// value.
fn admit(ctx: &mut LowerCtx, e_ty: Type, ann: &str, tag: ValueId, v: ValueId, bad: BlockId) {
    match e_ty {
        Type::I64 => {
            let int = icmp(ctx, IPred::Eq, tag, TAG_I64);
            guard(ctx, int, bad)
        }
        Type::F64 => {
            let f = icmp(ctx, IPred::Eq, tag, TAG_F64);
            let i = icmp(ctx, IPred::Eq, tag, TAG_I64);
            let either = or(ctx, f, i);
            guard(ctx, either, bad)
        }
        Type::Bool => {
            let b = icmp(ctx, IPred::Eq, tag, TAG_BOOL);
            guard(ctx, b, bad)
        }
        Type::Str | Type::Obj(_) => {
            let heap = icmp(ctx, IPred::Eq, tag, TAG_HEAP);
            let live = icmp(ctx, IPred::Ne, v, 0);
            let cell = and(ctx, heap, live);
            guard(ctx, cell, bad);
            let layout = match e_ty {
                Type::Str => torajs_rc::Tag::Str,
                _ => torajs_rc::Tag::Obj,
            };
            let ptr = append(ctx, InstKind::IntToPtr(Operand::Value(v)), Type::Ptr);
            // the u16 type tag sits at +4, after the u32 refcount
            let kind = append(
                ctx,
                InstKind::LoadU16Dyn(Operand::Value(ptr), Operand::ConstI64(2)),
                Type::I64,
            );
            let same = icmp(ctx, IPred::Eq, kind, layout as i64);
            guard(ctx, same, bad);
            if matches!(e_ty, Type::Obj(_)) {
                class_guard(ctx, ptr, ann, bad);
            }
        }
        _ => unreachable!("`take_typed` tests only the types listed here"),
    }
}

/// The class half of an Obj slot whose annotation names a declared,
/// non-generic class: an instance of it or of a subclass.
fn class_guard(ctx: &mut LowerCtx, ptr: ValueId, ann: &str, bad: BlockId) {
    let tags = crate::ssa_lower_instanceof::tags::class_tags_for_catch(ctx, ann)
        .expect("an Obj slot tested by tag names a declared class");
    let class_tag = crate::ssa_lower::load_obj_class_tag(ctx, Operand::Value(ptr));
    let mut acc: Option<ValueId> = None;
    for t in tags {
        let eq = icmp(ctx, IPred::Eq, class_tag, i64::from(t));
        acc = Some(match acc {
            None => eq,
            Some(prev) => or(ctx, prev, eq),
        });
    }
    let any_of = acc.expect("a declared class has its own tag");
    guard(ctx, any_of, bad);
}

/// Raise the TypeError for a rejected value through the catch's own
/// throw route, the value released.
fn reject(ctx: &mut LowerCtx, p: &str, ann: &str, (tag, v): (ValueId, ValueId)) {
    let any_box = ctx.intrinsics.any_box;
    let pair = vec![Operand::Value(tag), Operand::Value(v)];
    let boxed = append(ctx, InstKind::Call(any_box, pair), Type::Any);
    ctx.emit_drop_value(Operand::Value(boxed), Type::Any);
    raise(ctx, p, ann);
}

/// The inferred type missed a raise: report it as uncaught and stop.
fn missed(ctx: &mut LowerCtx, p: &str, ann: &str) {
    let msg = format!(
        "internal error: catch parameter `{p}` was inferred as `{ann}`, the thrown value is not one"
    );
    let msg = Operand::Value(ctx.intern_string_literal(msg.as_str()));
    let raiser = ctx.intrinsics.throw_type_error_str;
    let blk = ctx.cur_block;
    ctx.f.append_void(blk, InstKind::Call(raiser, vec![msg]));
    let exit = *(ctx.fn_table)
        .get(crate::ssa_lower_main_exit::UNCAUGHT_EXIT_SYM)
        .expect("__torajs_uncaught_exit declared in module setup");
    ctx.f.append_void(blk, InstKind::Call(exit, vec![]));
    ctx.f.set_term(blk, Terminator::Unreachable);
}

/// Record the TypeError of a rejected value and leave through the throw
/// route; the cursor moves to a fresh block no edge reaches.
fn raise(ctx: &mut LowerCtx, p: &str, ann: &str) {
    let msg = format!("catch parameter `{p}` of type `{ann}` cannot hold the thrown value");
    let msg = Operand::Value(ctx.intern_string_literal(msg.as_str()));
    let raiser = ctx.intrinsics.throw_type_error_str;
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(raiser, vec![msg]));
    let blk = ctx.cur_block;
    ctx.emit_throw_path(blk, None);
    ctx.cur_block = ctx.f.add_block();
}

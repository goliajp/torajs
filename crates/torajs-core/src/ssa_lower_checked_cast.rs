//! Checked cast from `any` into an annotated typed slot — the S0
//! measurement gate of RFC 20260922-any-to-typed-checked-cast.
//!
//! Today every boundary where an `any` value enters a typed slot runs
//! a ToNumber / ToString / ToBoolean. The RFC decides that a slot the
//! program **annotated** receives the value only when it already is
//! of the slot's type, and throws a TypeError otherwise. Before that
//! switch lands, `TORAJS_CHECKED_CAST` routes those boundaries through
//! the cast kernels so a full sweep can count what the switch costs:
//!
//! | value | boundaries cast |
//! |---|---|
//! | unset / anything else | none — byte-identical to today |
//! | `1` | the slots whose type the program wrote (let annotation, `as T`, parameter, declared return) |
//! | `2` | those plus the slots whose type the compiler inferred (reassignment, field write, element write) |
//!
//! The two levels answer two different questions: `1` is the price of
//! decision 1 itself; `2` is how many passing programs lean on an
//! inferred slot being wider than its inference, which is what the
//! RFC's S6 (sound inference) has to fix before those sites can move.
//!
//! The boxed entry (an any-lane call into a typed body,
//! `ssa_lower_boxed_entry/unbox.rs`) is not covered: it is built
//! outside a lowering context and has no throw check to unwind from a
//! rejected parameter, so a cast there would run the body on NaN.
//!
//! **Not a production path.** With the gate on, the binary makes no
//! promise beyond "a boundary that would cast now throws".
//!
//! An object-layout slot the program annotated is cast always, gate or
//! not ([`obj`]): before the cast its boundary had no conversion at
//! all, and the slot read the box's bits as a cell.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

mod obj;
mod obj_ann;
pub(crate) mod plan;
mod plan_emit;
pub(crate) use plan_emit::emit_plan_operands;

/// Where the program got the slot's type from (RFC §3.1).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CastScope {
    /// The program wrote it: annotation, `as T`, parameter, return.
    Annotated,
    /// The compiler inferred it: reassignment, field or element write.
    Inferred,
}

/// Whether the slot's annotation admits `undefined` beside its type.
/// SSA storage cannot answer this — `__nullable(T)` is storage- and
/// ABI-identical to `T` (`ssa_lower_parse_type`) — so the boundary
/// reads it off the annotation text it already holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admits {
    /// Exactly the target type.
    Exact,
    /// The target type or `undefined` (`string | undefined`).
    OrUndefined,
}

/// What the boundary does with the value — the verb of the message.
#[derive(Clone, Copy)]
pub(crate) enum CastSlot {
    Assign,
    Pass,
    Return,
    Cast,
    Store,
}

/// The gate level: 0 off, 1 annotated boundaries, 2 all boundaries.
/// Only the literal spellings count — a mistyped switch must read as
/// off, never as on (same protocol as `TORAJS_S0_ADMIT`).
fn gate_level() -> u8 {
    match std::env::var("TORAJS_CHECKED_CAST").as_deref() {
        Ok("1") => 1,
        Ok("2") => 2,
        _ => 0,
    }
}

/// Whether the gate is on at any level.
pub(crate) fn gate_on() -> bool {
    gate_level() != 0
}

fn gate_covers(scope: CastScope) -> bool {
    match gate_level() {
        1 => scope == CastScope::Annotated,
        2 => true,
        _ => false,
    }
}

fn site_text(slot: CastSlot, target: Type) -> Option<String> {
    let ty = match target {
        Type::F64 | Type::I64 => "number",
        Type::Str => "string",
        Type::Bool => "boolean",
        _ => return None,
    };
    Some(site_line(slot, ty))
}

/// The kernel's message for a slot of type `ty`, `{}` standing for the
/// rejected value's `typeof` class.
pub(crate) fn site_line(slot: CastSlot, ty: &str) -> String {
    let verb = match slot {
        CastSlot::Assign => "assign {} to",
        CastSlot::Pass => "pass {} to parameter of type",
        CastSlot::Return => "return {} from function returning",
        CastSlot::Cast => "cast {} to",
        CastSlot::Store => "store {} into",
    };
    format!("cannot {verb} `{ty}`")
}

impl LowerCtx<'_> {
    /// Whether this function's return annotation admits `undefined`
    /// (`string | undefined`).
    pub(crate) fn ret_ann_nullable(&self) -> bool {
        (self.ret_ann.as_deref()).is_some_and(|r| r.starts_with("__nullable("))
    }

    /// The checked cast of `op` (an `any` operand) into `target`, when
    /// the gate covers `scope` and `target` is a scalar slot; `None`
    /// leaves the boundary's own conversion in charge. A string result
    /// is owned, exactly as the ToString it replaces.
    pub(crate) fn checked_cast(
        &mut self,
        op: Operand,
        target: Type,
        slot: CastSlot,
        scope: CastScope,
        admits: Admits,
    ) -> Option<Operand> {
        if self.operand_ty(&op) != Type::Any {
            return None;
        }
        if let Type::Closure(_) = target {
            return self.check_fn_slot(op, slot, admits);
        }
        if !gate_covers(scope) {
            return None;
        }
        let site = site_text(slot, target)?;
        let (kernel, ret) = match target {
            Type::F64 | Type::I64 => (self.intrinsics.anyv_cast_number, Type::F64),
            Type::Str if admits == Admits::OrUndefined => {
                (self.intrinsics.anyv_cast_str_or_undef, Type::Str)
            }
            Type::Str => (self.intrinsics.anyv_cast_str, Type::Str),
            _ => (self.intrinsics.anyv_cast_bool, Type::Bool),
        };
        let site = Operand::Value(self.intern_string_literal(site.as_str()));
        let v = Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Call(kernel, vec![op, site]),
            ret,
            None,
        ));
        self.emit_throw_check(None);
        Some(if target == Type::I64 {
            self.coerce_to_i64(v)
        } else {
            v
        })
    }

    /// The check of `op` (an `any` operand) written into a function
    /// slot, gate or not: a function object (any `Closure` cell: a
    /// compiled closure, a bound or builtin function) is stored as it
    /// is, anything else is a TypeError where the value is stored, as
    /// for an object slot. The slot's calls go through the boxed entry
    /// (`check::face_slots`). A return and a slot that admits
    /// `undefined` are not checked. Always `None`: the boundary stores
    /// the value as it did.
    fn check_fn_slot(&mut self, op: Operand, slot: CastSlot, admits: Admits) -> Option<Operand> {
        if matches!(slot, CastSlot::Return) || admits == Admits::OrUndefined {
            return None;
        }
        let site = site_line(slot, "function");
        let site = Operand::Value(self.intern_string_literal(site.as_str()));
        let f = self.fn_table[crate::ssa_lower_intrinsics_any_cast::CHECK_FN];
        self.f
            .append_void(self.cur_block, InstKind::Call(f, vec![op, site]));
        self.emit_throw_check(None);
        None
    }
}

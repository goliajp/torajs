//! The checked cast of an `any` value into an object-layout slot the
//! program annotated (kernel `torajs-anyvalue::anyv_cast::obj`, RFC
//! 20260922-any-to-typed-checked-cast §4.1 and §4.4 item 3).
//!
//! What the slot admits follows its annotation, compiled into a cast
//! plan ([`super::plan`]):
//!
//! - a declared class: an instance of the class or of a subclass,
//!   which crosses with its identity (an instance of another class
//!   would run this one's methods, and is rejected); or an object that
//!   is no class instance (an object literal through `any`, a
//!   `JSON.parse` result) whose fields all have the class's field
//!   types, copied into a fresh instance;
//! - any other object type: a cell with the layout's fields, crossing
//!   as it is, or any other object with fields of those types, copied
//!   into a fresh cell of the layout;
//! - either, written `T | undefined` / `T | null`: `undefined` and
//!   `null` too.
//!
//! Anything else throws at the write. An interface whose members
//! dispatch through witness rows (`ssa_lower_iface`), or any object
//! type with a method, takes an instance of any class implementing it,
//! whose methods are not among its fields, so neither is cast here.

use crate::ast::ExprId;
use crate::check_type_ann::strip_nullable;
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;

use super::CastSlot;

impl LowerCtx<'_> {
    /// `op`, an `any` value returned, cast into an object return type
    /// the program wrote; the value's own stake goes. A return type a
    /// callback's context seeded is not one the program wrote, and
    /// keeps its own lanes.
    pub(crate) fn cast_returned_obj(&mut self, op: Operand) -> Option<Operand> {
        let (Type::Obj(sid), Some(ann)) = (self.f.ret, self.ret_ann.clone()) else {
            return None;
        };
        // a return type spelled as an object literal's shape is also what
        // the compiler infers for a function returning one; an inferred
        // one is not sound for an object later given accessors, and such
        // a return keeps the value
        let inferred_shape = strip_nullable(&ann).trim().starts_with("__inlobj(")
            && !self.scans.written_ret_fns.contains(&self.f.name);
        if self.ast.contextual_ret_fns.contains(&self.f.name) || inferred_shape {
            return None;
        }
        let v = self.checked_cast_obj(op, sid, &ann, CastSlot::Return)?;
        self.emit_drop_value(op, Type::Any);
        Some(v)
    }

    /// `v`, an `any` value assigned to the binding `name` (a module
    /// global when `global`) whose slot is `slot`, cast when the slot is
    /// an object layout the program annotated; the stake the assignment
    /// took on `v` goes.
    pub(crate) fn cast_assigned_obj(
        &mut self,
        name: &str,
        global: bool,
        slot: Type,
        v: Operand,
    ) -> Option<Operand> {
        let Type::Obj(sid) = slot else { return None };
        let ann = self.binding_ann(name, global)?;
        let cast = self.checked_cast_obj(v, sid, &ann, CastSlot::Assign)?;
        self.emit_drop_value(v, Type::Any);
        Some(cast)
    }

    /// Whether an `any` value assigned to the binding `name` (a module
    /// global when `global`) whose slot is `slot` is cast
    /// ([`Self::cast_assigned_obj`]).
    pub(crate) fn casts_assigned_obj(&self, name: &str, global: bool, slot: Type, v: Type) -> bool {
        let (Type::Obj(sid), Type::Any) = (slot, v) else {
            return false;
        };
        (self.binding_ann(name, global)).is_some_and(|a| self.casts_into(sid, &a))
    }

    /// `v`, an `any` value `value` stores into the field `field` of the
    /// class instance `obj`, cast when the field's type is an object
    /// layout; an owned temporary's stake goes, and the field takes the
    /// cast's.
    pub(crate) fn cast_stored_obj(
        &mut self,
        field_ty: Type,
        v: Operand,
        value: ExprId,
        obj: ExprId,
        field: &str,
    ) -> Option<Operand> {
        let Type::Obj(sid) = field_ty else {
            return None;
        };
        let ann = self.field_ann(obj, field)?;
        let cast = self.checked_cast_obj(v, sid, &ann, CastSlot::Store)?;
        if self.expr_transfers_ownership(value) {
            self.emit_drop_value(v, Type::Any);
        }
        Some(cast)
    }

    /// `v`, an `any` value `arg` appends to an array of the object
    /// layout `elem` (`push`, `unshift`), cast into the element type
    /// the array's annotation names; an owned temporary's stake goes,
    /// and the element takes the cast's. The checker admits such an
    /// append only onto an array whose element annotation it traced to
    /// a type the cast tests (`check_type_of_call_array_push_unshift`),
    /// and an array whose element type is the compiler's inference was
    /// retyped `any[]` before (`let_widen::push`).
    pub(crate) fn cast_pushed_obj(
        &mut self,
        elem: Type,
        v: Operand,
        arg: ExprId,
    ) -> Option<Operand> {
        let Type::Obj(sid) = elem else { return None };
        if self.operand_ty(&v) != Type::Any {
            return None;
        }
        let ann = (self.pushed_elem_ann(arg)).expect("the checker traced the element annotation");
        let cast = (self.checked_cast_obj(v, sid, &ann, CastSlot::Store))
            .expect("the checker admitted only an element type the cast tests");
        if self.expr_transfers_ownership(arg) {
            self.emit_drop_value(v, Type::Any);
        }
        Some(cast)
    }

    /// The checked cast of `op` into the object slot `sid` the program
    /// annotated `ann`: a value typed `Obj(sid)` holding a stake of its
    /// own, after a throw check. `None` when `op` is not `any` or the
    /// slot is not cast (a witness interface's, a compiler-made type's).
    pub(crate) fn checked_cast_obj(
        &mut self,
        op: Operand,
        sid: StructId,
        ann: &str,
        slot: CastSlot,
    ) -> Option<Operand> {
        if self.operand_ty(&op) != Type::Any {
            return None;
        }
        let base = strip_nullable(ann).trim();
        // a `__`-named type is one the compiler made (a generator's
        // wrapper), never the program's own annotation; so is the empty
        // shape inferred from an object built up from `{}`. A written
        // object type literal is spelled `__inlobj(fields)`
        let compiler_made =
            base == "__inlobj()" || (base.starts_with("__") && !base.starts_with("__inlobj("));
        if compiler_made || !self.casts_into(sid, ann) {
            return None;
        }
        let shown = if base.len() != ann.len() {
            format!("{base} | undefined")
        } else {
            base.to_string()
        };
        let site = super::site_line(slot, &shown);
        Some(self.emit_plan_cast(op, Type::Obj(sid), ann, &site))
    }

    /// [`Self::checked_cast_obj`] for an array of object layouts,
    /// at any depth (`const ps: P[] = anyValue`): an array whose
    /// elements each pass as the element slot would take them is
    /// copied; anything else raises the TypeError.
    pub(crate) fn checked_cast_obj_arr(
        &mut self,
        op: Operand,
        ty: Type,
        ann: &str,
        slot: CastSlot,
    ) -> Option<Operand> {
        if self.operand_ty(&op) != Type::Any {
            return None;
        }
        // a slot the width table widened to `any[]` holds the array
        // cell as it is, its elements read on the any lane
        if let Type::Arr(id) = ty
            && self.arr_layouts[id.0 as usize] == Type::Any
        {
            let fid = *(self.fn_table.get("__torajs_arr_cell_from_any"))
                .expect("__torajs_arr_cell_from_any intrinsic missing");
            let cell = self
                .f
                .append_inst(self.cur_block, InstKind::Call(fid, vec![op]), ty, None);
            self.emit_throw_check(None);
            self.emit_rc_inc(Operand::Value(cell));
            return Some(Operand::Value(cell));
        }
        let (mut elem_ty, mut elem) = (ty, ann.to_string());
        while let Type::Arr(id) = elem_ty {
            elem_ty = self.arr_layouts[id.0 as usize];
            elem = crate::ast_throw_info::elem_ann(&elem)?;
        }
        let Type::Obj(sid) = elem_ty else {
            return None;
        };

        let base = strip_nullable(&elem).trim();
        if base.starts_with("__") && !base.starts_with("__inlobj(") || !self.casts_into(sid, &elem)
        {
            return None;
        }
        let site = super::site_line(slot, strip_nullable(ann).trim());
        Some(self.emit_plan_cast(op, ty, ann, &site))
    }

    /// Whether a slot of layout `sid` annotated `ann` is cast
    /// ([`super::plan::PlanTables::casts_into`]).
    fn casts_into(&self, sid: StructId, ann: &str) -> bool {
        self.plan_tables().casts_into(sid, ann)
    }
}

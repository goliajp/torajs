//! The checked cast of an `any` value into an object-layout slot the
//! program annotated (RFC 20260922 §4.1, the `Obj(sid)` row; kernel
//! `torajs-anyvalue::anyv_cast::obj`).
//!
//! The slot's readers load its fields at the layout's offsets and call
//! a class's methods as that class's own functions, so the value has to
//! be a cell the slot can read as it is: the cell crosses with its
//! identity, or the write throws. What the slot admits follows its
//! annotation:
//!
//! - a declared class: an instance of the class or of a subclass —
//!   classes with the same fields share one layout, and an instance of
//!   another class would run this one's methods (the boxed adapter's
//!   class slots answer the same, `ssa_lower_boxed_entry::kin`);
//! - any other object type: a cell with the layout's fields, and a
//!   fresh dynamic object (an owned temporary nothing else holds) the
//!   kernel can rebuild as such a cell without the copy showing;
//! - either, written `T | undefined` / `T | null`: `undefined` and
//!   `null` too.
//!
//! An interface whose members dispatch through witness rows
//! (`ssa_lower_iface`), or any object type with a method, takes an
//! instance of any class implementing it, whose methods are not among
//! its fields, so neither is cast here.

use crate::ast::ExprId;
use crate::check_type_ann::strip_nullable;
use crate::ssa::{InstKind, Operand, StringId, StringLiteral, StructId, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_instanceof::tags::descendant_tags;

use super::CastSlot;

/// Kernel `mode` bits (`torajs-anyvalue::anyv_cast::obj`).
const NULLISH: i64 = 1;
const FRESH: i64 = 2;

impl LowerCtx<'_> {
    /// `op`, an `any` value `maybe` returns, cast into an object return
    /// type the program wrote; the value's own stake goes. A return
    /// type a callback's context seeded is no annotation (RFC 20260922
    /// decision 2), and keeps its own lanes.
    pub(crate) fn cast_returned_obj(
        &mut self,
        op: Operand,
        maybe: Option<ExprId>,
    ) -> Option<Operand> {
        let (Type::Obj(sid), Some(ann)) = (self.f.ret, self.ret_ann.clone()) else {
            return None;
        };
        if self.ast.contextual_ret_fns.contains(&self.f.name) {
            return None;
        }
        let v = self.checked_cast_obj(op, sid, &ann, CastSlot::Return, maybe)?;
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
        value: ExprId,
    ) -> Option<Operand> {
        let Type::Obj(sid) = slot else { return None };
        let ann = self.binding_ann(name, global)?;
        let cast = self.checked_cast_obj(v, sid, &ann, CastSlot::Assign, Some(value))?;
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
        let cast = self.checked_cast_obj(v, sid, &ann, CastSlot::Store, Some(value))?;
        if self.expr_transfers_ownership(value) {
            self.emit_drop_value(v, Type::Any);
        }
        Some(cast)
    }

    /// `v`, an `any` value `arg` appends to an array of the object
    /// layout `elem` (`push`, `unshift`), cast into the element type
    /// the array's annotation names; an owned temporary's stake goes,
    /// and the element takes the cast's. An array whose element type
    /// is the compiler's inference was retyped `any[]` before
    /// (`let_widen::push`), so one without an annotation to cast to is
    /// refused here, as the checker refused it before casts.
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
        let ann = self.pushed_elem_ann(arg);
        let cast = ann.and_then(|a| self.checked_cast_obj(v, sid, &a, CastSlot::Store, Some(arg)));
        let Some(cast) = cast else {
            panic!(
                "ssa-lower: an `any` value appended to an array of {elem:?} whose element type \
                 is not an annotated class or object type"
            );
        };
        if self.expr_transfers_ownership(arg) {
            self.emit_drop_value(v, Type::Any);
        }
        Some(cast)
    }

    /// The checked cast of `op` into the object slot `sid` the program
    /// annotated `ann`: a value typed `Obj(sid)` holding a stake of its
    /// own, after a throw check. `value` is the expression `op` was
    /// lowered from, when it may be a fresh temporary. `None` when `op`
    /// is not `any` or the slot is a witness interface's.
    pub(crate) fn checked_cast_obj(
        &mut self,
        op: Operand,
        sid: StructId,
        ann: &str,
        slot: CastSlot,
        value: Option<ExprId>,
    ) -> Option<Operand> {
        if self.operand_ty(&op) != Type::Any {
            return None;
        }
        if !self.casts_into(sid, ann) {
            return None;
        }
        let base = strip_nullable(ann).trim();
        let class = self.slot_class(base, sid);
        let (tag, kin) = match class {
            Some(name) => {
                let kin = self.class_list(name);
                (self.class_name_to_tag[name], kin)
            }
            None => (self.sid_class_tag(sid), Operand::ConstI64(0)),
        };
        let mut mode = 0;
        if base.len() != ann.len() {
            mode |= NULLISH;
        }
        if class.is_none() && value.is_some_and(|e| self.expr_owned_shape(e)) {
            mode |= FRESH;
        }
        let shown = if mode & NULLISH != 0 {
            format!("{base} | undefined")
        } else {
            base.to_string()
        };
        let site = Operand::Value(self.intern_string_literal(&super::site_line(slot, &shown)));
        let args = vec![
            op,
            Operand::ConstI64(i64::from(tag)),
            kin,
            site,
            Operand::ConstI64(mode),
        ];
        let kernel = self.intrinsics.anyv_cast_obj;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(kernel, args),
            Type::Obj(sid),
            None,
        );
        self.emit_throw_check(None);
        Some(Operand::Value(v))
    }

    /// Whether a slot of layout `sid` annotated `ann` is cast: a class
    /// slot always; another object type unless it is an interface with
    /// witness rows or has a method (module doc).
    fn casts_into(&self, sid: StructId, ann: &str) -> bool {
        self.slot_class(strip_nullable(ann).trim(), sid).is_some()
            || !(crate::ssa_lower_iface::sid_is_iface(self, sid) || self.has_methods(sid))
    }

    /// Whether layout `sid` has a function-typed member: an object type
    /// with methods, which an instance of a class implementing it has
    /// on its prototype and not among its fields, so the layout test
    /// cannot answer for it.
    fn has_methods(&self, sid: StructId) -> bool {
        (self.struct_layouts[sid.0 as usize].iter())
            .any(|(_, t)| matches!(t, Type::FnSig(_) | Type::Closure(_)))
    }

    /// The declared class `base` names, when its instances are laid out
    /// as `sid` (a generic instance has a layout of its own, and keeps
    /// the layout test).
    fn slot_class<'b>(&self, base: &'b str, sid: StructId) -> Option<&'b str> {
        (self.ast.class_parents.contains_key(base)
            && self.class_name_to_tag.contains_key(base)
            && self.aliases.get(base) == Some(&Type::Obj(sid)))
        .then_some(base)
    }

    /// The class list of `class` (its tag and its subclasses'), as the
    /// address of a rodata block the kernel reads; generic rows of the
    /// classes join it after Pass 2 (`admit_generic_rows`).
    fn class_list(&mut self, class: &str) -> Operand {
        let tags = descendant_tags(&self.ast.class_parents, self.class_name_to_tag, class);
        let bytes = crate::ssa_lower_boxed_entry::encode_class_list(&tags);
        let id = StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings
            .push(StringLiteral::from_latin1_bytes(bytes));
        self.anon_stamp_pool.borrow_mut().note_kin_list(id);
        let ptr = self
            .f
            .append_inst(self.cur_block, InstKind::StringRef(id), Type::Ptr, None);
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::PtrToInt(Operand::Value(ptr)),
            Type::I64,
            None,
        );
        Operand::Value(bits)
    }

    /// The class tag the cells of layout `sid` carry: a named class's
    /// own (the lowest, when classes share the layout), otherwise the
    /// anonymous stamp — the split the object literal alloc stamps by.
    fn sid_class_tag(&self, sid: StructId) -> u32 {
        (self.class_name_to_tag.iter())
            .filter(|(name, _)| self.aliases.get(*name) == Some(&Type::Obj(sid)))
            .map(|(_, &tag)| tag)
            .min()
            .unwrap_or_else(|| self.anon_stamp_pool.borrow_mut().assign_or_get(sid))
    }
}

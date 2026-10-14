//! Calling the object cast kernel with a compiled plan
//! ([`super::plan`]): the plan bytes become a rodata literal, and the
//! vtables of its object nodes a stack block filled at the call.

use crate::ssa::{self, BlockId, InstKind, Operand, StringId, StringLiteral, Type};
use crate::ssa_lower::LowerCtx;

use super::plan::PlanTables;

impl LowerCtx<'_> {
    /// The tables a cast plan is compiled from.
    pub(super) fn plan_tables(&self) -> PlanTables<'_> {
        PlanTables {
            ast: self.ast,
            struct_layouts: self.struct_layouts,
            arr_layouts: self.arr_layouts,
            class_name_to_tag: self.class_name_to_tag,
            aliases: self.aliases,
            anon_stamp_pool: self.anon_stamp_pool,
        }
    }

    /// `op`, an `any` value, cast by the plan of a slot of type `ty`
    /// annotated `ann` (an object layout, or an array of them); `site`
    /// is the kernel's message, `{}` standing for the rejected value's
    /// `typeof` class. The answer holds a stake of its own, after a
    /// throw check.
    pub(crate) fn emit_plan_cast(
        &mut self,
        op: Operand,
        ty: Type,
        ann: &str,
        site: &str,
    ) -> Operand {
        let v = self.emit_plan_cast_unchecked(op, ty, ann, site);
        self.emit_throw_check(None);
        v
    }

    /// Whether the cast plan of a slot of type `ty` annotated `ann`
    /// admits `null` / `undefined` in some node.
    pub(crate) fn plan_admits_nullish(&self, ty: Type, ann: &str) -> bool {
        super::plan::compile(&self.plan_tables(), ty, ann).nullish
    }

    /// [`Self::emit_plan_cast`] without the throw check: the answer is
    /// NULL under a pending throw, which the caller checks.
    pub(crate) fn emit_plan_cast_unchecked(
        &mut self,
        op: Operand,
        ty: Type,
        ann: &str,
        site: &str,
    ) -> Operand {
        let plan = super::plan::compile(&self.plan_tables(), ty, ann);
        let site = Operand::Value(self.intern_string_literal(site));
        let id = StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings
            .push(StringLiteral::from_latin1_bytes(plan.bytes));
        self.anon_stamp_pool.borrow_mut().note_cast_plan(id);
        let block = self.cur_block;
        let (plan_bits, vtables) = emit_plan_operands(self.f, block, id, &plan.vtables);
        let kernel = match plan.members {
            true => *(self
                .fn_table
                .get(crate::ssa_lower_intrinsics_any_cast::MEMBERS_CAST))
            .expect("a plan with a member declares its cast"),
            false => self.intrinsics.anyv_cast_obj,
        };
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(kernel, vec![op, plan_bits, vtables, site]),
            ty,
            None,
        );
        Operand::Value(v)
    }
}

/// Emit into `block` of `f` the kernel's plan and vtable operands: the
/// address of the plan registered as the string `id`, and a block
/// holding `vtables` (null when no object node wears a vtable).
pub(crate) fn emit_plan_operands(
    f: &mut ssa::Function,
    block: BlockId,
    id: StringId,
    vtables: &[Option<String>],
) -> (Operand, Operand) {
    let ptr = f.append_inst(block, InstKind::StringRef(id), Type::Ptr, None);
    let bits = f.append_inst(
        block,
        InstKind::PtrToInt(Operand::Value(ptr)),
        Type::I64,
        None,
    );
    if vtables.iter().all(Option::is_none) {
        return (Operand::Value(bits), Operand::ConstPtrNull);
    }
    let n = vtables.len() as u64;
    let vts = f.append_inst(BlockId(0), InstKind::AllocaBytes(n * 8), Type::Ptr, None);
    for (i, vt) in vtables.iter().enumerate() {
        let vt = match vt {
            Some(g) => Operand::Value(f.append_inst(
                block,
                InstKind::GlobalRef(g.clone()),
                Type::Ptr,
                None,
            )),
            None => Operand::ConstPtrNull,
        };
        f.append_void(
            block,
            InstKind::Store(vt, Operand::Value(vts), i as u64 * 8),
        );
    }
    (Operand::Value(bits), Operand::Value(vts))
}

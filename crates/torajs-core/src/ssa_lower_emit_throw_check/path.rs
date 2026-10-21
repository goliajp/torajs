//! The throw edge a throw check leaves by: the catch of the innermost
//! active try, or the propagation out of the function.

use crate::ssa::{BlockId, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_temp_scratch::ThrowTemp;

impl LowerCtx<'_> {
    /// Leave by the throw edge with no test, after a call that always
    /// raises: the flag it sets needs no reading. The current block is
    /// left terminated.
    pub(crate) fn emit_throw_now(&mut self) {
        let throw_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(cb, Terminator::Br(throw_blk));
        self.emit_throw_path(throw_blk, None);
    }

    /// Fill `throw_blk`: route to the innermost active try's catch, or
    /// propagate (drop owned locals + ret sentinel). Either way an
    /// owned call result dies here, and so does every temp parked on
    /// `temps.throw_live`: release them first (result, then parked
    /// temps newest-first).
    pub(super) fn emit_throw_path(&mut self, throw_blk: BlockId, owned: Option<(Operand, Type)>) {
        let live: Vec<ThrowTemp> = self.temps.throw_live.iter().flatten().cloned().collect();
        if let Some(catch) = self.try_stack.last().copied() {
            // the jump into the catch also leaves every frame opened
            // since the try body began (`catch.scope_depth`); their
            // owners declared before this call are live and die here,
            // in the same block-close protocol the fall-through would
            // have used
            let scope_drops = self.scopes_have_drops_from(catch.scope_depth);
            let closes = catch.teardown_depth < self.for_of_teardown_stack.len();
            if owned.is_some() || !live.is_empty() || scope_drops || closes {
                self.cur_block = throw_blk;
                if let Some((op, ty)) = owned {
                    self.emit_drop_value(op, ty);
                }
                for t in live.iter().rev() {
                    self.emit_drop_throw_temp(t.clone());
                }
                // the for-ofs the jump leaves run IteratorClose under
                // the suspended pending throw, before their slots drop
                crate::ssa_lower_for_of_teardown::emit_closes_from(
                    self,
                    catch.teardown_depth,
                    true,
                );
                self.emit_drops_for_scopes_from(catch.scope_depth);
                let cb2 = self.cur_block;
                self.f.set_term(cb2, Terminator::Br(catch.blk));
            } else {
                self.f.set_term(throw_blk, Terminator::Br(catch.blk));
            }
        } else if self.is_main_fn && !self.suspend_body {
            // the throw escaped every user frame: an uncaught exception
            // reports the pending throw to stderr and exits 1
            // (__torajs_uncaught_exit_code), where returning the
            // sentinel 0 would exit clean
            self.cur_block = throw_blk;
            if let Some((op, ty)) = owned {
                self.emit_drop_value(op, ty);
            }
            for t in live.iter().rev() {
                self.emit_drop_throw_temp(t.clone());
            }
            self.emit_drops_for_owned_locals();
            let uncaught_fid = *self
                .fn_table
                .get("__torajs_uncaught_exit_code")
                .expect("__torajs_uncaught_exit_code declared in module setup");
            let code = self.f.append_inst(
                self.cur_block,
                InstKind::Call(uncaught_fid, vec![]),
                Type::I32,
                None,
            );
            let cb2 = self.cur_block;
            self.f
                .set_term(cb2, Terminator::Ret(Some(Operand::Value(code))));
        } else {
            self.cur_block = throw_blk;
            if let Some((op, ty)) = owned {
                self.emit_drop_value(op, ty);
            }
            for t in live.iter().rev() {
                self.emit_drop_throw_temp(t.clone());
            }
            self.emit_drops_for_owned_locals();
            let cb2 = self.cur_block;
            self.f.append_void(cb2, InstKind::ThrowSignal);
            let ret_ty = self.f.ret;
            let term = match ret_ty {
                Type::Void => Terminator::Ret(None),
                Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
                Type::I32 => Terminator::Ret(Some(Operand::ConstI32(0))),
                Type::Bool => Terminator::Ret(Some(Operand::ConstBool(false))),
                _ => Terminator::Ret(Some(Operand::ConstI64(0))),
            };
            self.f.set_term(cb2, term);
        }
    }
}

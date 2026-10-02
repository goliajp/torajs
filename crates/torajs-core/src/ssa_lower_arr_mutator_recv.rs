//! Receiver resolution for the array mutators (`pop` / `shift` /
//! `unshift`) — which receiver shapes the typed lanes take and how
//! each one is read. Moved out of `ssa_lower_arr_mutators.rs` at the
//! 500-line limit; the host keeps what a mutation does, this file
//! keeps what it is done to.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Chunk 628 — mutator receiver resolution shared by pop / shift /
    /// unshift. Four receiver shapes:
    ///
    /// - Ident bound to a local `Array<T>` — load the stack slot.
    /// - Ident bound to a K.3 const-global `Array<T>` — GlobalRef +
    ///   load.
    /// - Arr-typed Member expr (`b.arr.shift()`) — the field read is
    ///   a +0 borrow and the mutators work in place (pop/shift) or on
    ///   the B1 fixed cell (unshift), so no write-back is needed. The
    ///   checker's expr type gates BEFORE lowering so a decline emits
    ///   nothing.
    /// - Arr-typed Index expr (`z[i].pop()`, chunk 700) — same story:
    ///   the elem read is a +0 borrow and B1 fixed the cell across
    ///   grow, so the historical "no place to store a realloc'd
    ///   pointer" rejection is moot (the chunk-697 push twin).
    pub(crate) fn resolve_mutator_arr_receiver(
        &mut self,
        recv_id: ExprId,
    ) -> Option<(Operand, Type)> {
        match self.ast.get_expr(recv_id) {
            Expr::Ident(recv_name) => {
                let recv_name = recv_name.clone();
                if let Some(info) = self.locals.get(&recv_name).copied()
                    && matches!(info.ty, Type::Arr(_))
                {
                    let cur_arr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                        info.ty,
                        None,
                    );
                    return Some((Operand::Value(cur_arr), info.ty));
                }
                if let Some(gty) = self.globals.get(&recv_name).copied()
                    && matches!(gty, Type::Arr(_))
                {
                    let gref = self.f.append_inst(
                        self.cur_block,
                        InstKind::GlobalRef(recv_name.clone()),
                        Type::Ptr,
                        None,
                    );
                    let cur_arr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(gty, Operand::Value(gref), 0),
                        gty,
                        None,
                    );
                    return Some((Operand::Value(cur_arr), gty));
                }
                None
            }
            Expr::Member { .. } | Expr::Index { .. } => {
                if !matches!(
                    self.expr_types.get(&recv_id),
                    Some(crate::check::Type::Array(_))
                ) {
                    return None;
                }
                let v = self.lower_expr(recv_id);
                let ty = self.operand_ty(&v);
                if matches!(ty, Type::Arr(_)) {
                    return Some((v, ty));
                }
                None
            }
            _ => match &self.temps.mutator_recv {
                Some((id, op)) if *id == recv_id => {
                    let op = *op;
                    let ty = self.operand_ty(&op);
                    Some((op, ty))
                }
                _ => None,
            },
        }
    }

    /// An owned temporary array as the receiver of a mutator —
    /// `[1, 2].pop()`, `mk().shift()`, `[0].unshift(v)`. Legal
    /// programs every engine runs; the resolver above only knew
    /// places (bindings, fields, elements), so these died at lowering
    /// with "unsupported member call shape". B1 fixed the cell across
    /// grow, so the temp needs no write-back: lower it once here,
    /// park it for the throw edges the arm may emit, and let
    /// [`Self::release_mutator_temp`] drop it after the arm.
    ///
    /// Gated to the arms' own admission (`unshift` takes one argument)
    /// so a declined call never sees its receiver evaluated twice.
    pub(crate) fn lower_mutator_temp(
        &mut self,
        callee: ExprId,
        args: &[ExprId],
    ) -> Option<Option<usize>> {
        let Expr::Member { obj, name } = self.ast.get_expr(callee) else {
            return None;
        };
        let admitted =
            matches!(name.as_str(), "pop" | "shift") || (name == "unshift" && args.len() == 1);
        let obj = *obj;
        if !admitted
            || !self.expr_owned_shape(obj)
            || !matches!(
                self.expr_types.get(&obj),
                Some(crate::check::Type::Array(_))
            )
        {
            return None;
        }
        let op = self.lower_expr(obj);
        if !matches!(self.operand_ty(&op), Type::Arr(_)) {
            return None;
        }
        let tok = self.park_owned_temp(obj, &op);
        self.temps.mutator_recv = Some((obj, op));
        Some(tok)
    }

    /// The release half of [`Self::lower_mutator_temp`].
    pub(crate) fn release_mutator_temp(&mut self, tok: Option<usize>) {
        if let Some((obj, op)) = self.temps.mutator_recv.take() {
            self.unpark_owned_temp(tok);
            self.release_owned_temp(obj, &op);
        }
    }
}

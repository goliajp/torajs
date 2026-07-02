//! Throw-check emission helper for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 393 — Path A.3-batch14.
//!
//! Single method:
//!
//! - `emit_throw_check(target)` — load the `throw_active` flag after
//!   a call; if non-zero, branch to the innermost active try-block's
//!   catch (via `try_stack`) or — if no try is active in this fn —
//!   emit drops + ret a sentinel so the caller's own throw_check picks
//!   it up. Skips entirely for runtime intrinsics (they never throw)
//!   and for verified-non-throwing user fns (M4.3.b — fib40 / gcd /
//!   mandelbrot etc., recovering the M4.1 5% slowdown for programs
//!   that never touch try/throw). For `main`, the escaped-throw path
//!   routes through `__torajs_uncaught_exit_code` to report the
//!   pending throw to stderr + yield exit code 1 (bug-327 C2.5).
//!
//! Method body is byte-for-byte preserved from the source; the sibling
//! reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`, so the
//! numerous per-call-site invocations from the various lower_expr call
//! arms need zero edits.

use crate::ssa::{FuncId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// load the throw_active flag; if non-zero, branch to the innermost
    /// active try-block's catch (via `try_stack`) or — if no try is
    /// active in this fn — emit drops + ret a sentinel so the caller's
    /// own throw_check picks it up. Skips entirely for runtime intrinsics
    /// (they never throw).
    pub(crate) fn emit_throw_check(&mut self, target: Option<FuncId>) {
        if let Some(fid) = target {
            if self.is_intrinsic(fid) {
                return;
            }
            // M4.3.b — skip the check entirely if the callee is a
            // verified-non-throwing user fn. fib40 / popcount / gcd /
            // mandelbrot etc. all live here, so the M4.1 5% slowdown
            // is gone for any program that doesn't use try/throw at
            // all (or whose hot fns provably can't reach a throw).
            let callee_name = self.f_name_of(fid);
            if !self.may_throw_fns.contains(&callee_name) {
                return;
            }
        }
        let active = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.throw_check, vec![]),
            Type::I64,
            None,
        );
        let cmp = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let normal_blk = self.f.add_block();
        let throw_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(cmp),
                then_blk: throw_blk,
                else_blk: normal_blk,
            },
        );
        // throw_blk: route to innermost active try's catch, or
        // propagate (drop owned locals + ret sentinel).
        if let Some(catch) = self.try_stack.last().copied() {
            self.f.set_term(throw_blk, Terminator::Br(catch));
        } else if self.is_main_fn {
            // bug-327 C2.5 — the throw escaped every user frame: this
            // is an uncaught exception. Pre-fix main ret'd the I32
            // sentinel 0, so a crashing program exited clean (bun:
            // error report + exit 1). __torajs_uncaught_exit_code
            // reports the pending throw to stderr and yields 1.
            self.cur_block = throw_blk;
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
            self.emit_drops_for_owned_locals();
            let cb2 = self.cur_block;
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
        self.cur_block = normal_blk;
    }
}

//! Seed the constructor's fresh-`this` writes into the body's proofs
//! (`ssa_lower_fn` step 5b); the analysis itself is
//! [`crate::ast::fresh_this`], shared with the throw census.

use crate::ast;
use crate::ssa_lower::LowerCtx;

pub(crate) fn seed_fresh_this(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    params: &[ast::Param],
    body: &[ast::Stmt],
) {
    let fresh = ast::fresh_this::fresh_this_writes(ctx.ast, name, params, body);
    ctx.proofs.fresh_this_recv = fresh.recv;
    ctx.proofs.fresh_this_first_write = fresh.first_write;
}

//! State-machine assembly for the generator desugar — the
//! `while (true) { if (st==N) {arm} ... }` builder.
//!
//! RFC 20260802-generator-try-region C0 — verbatim move of
//! `build_state_machine_next_body` out of `desugar_generators.rs`
//! (491 LOC, touching the 500 hard limit) so the try-region
//! dispatch wrap (C1) has room to land here next to the assembly
//! it extends.

use super::{Ast, BinOp, Expr, Stmt, default_init_for_type};

/// Build the next()'s state-machine body: lower `gen_body` through
/// GenSm into `arms`, then assemble `[while(true) { if(state==0){arm0}
/// if(state==1){arm1} ... ; catch-all }, return {value:0,done:true}]`
/// as the two-stmt body. Caller wraps it into the ClassMethod.
pub(super) fn build_state_machine_next_body(
    ast: &mut Ast,
    gen_body: Vec<Stmt>,
    yield_ty: &str,
) -> Vec<Stmt> {
    // Build the state machine. Each arm is the body of one state in
    // an if-chain wrapped by `while (true) { ... }`. Yields close an
    // arm with `return {value:e, done:false}`; control-flow gotos
    // close with `state = N; continue;` and the `while(true)` loop
    // re-enters the if-chain at the new state.
    let mut sm = super::desugar_generators_sm::GenSm::new(ast, yield_ty.to_string());
    sm.lower_seq(gen_body);
    // After the last body stmt, the natural exit is "done forever".
    let zero = default_init_for_type(yield_ty);
    let zero_id = sm.ast.add_expr(zero);
    let done_lit = sm.ast.add_expr(Expr::Bool(true));
    let final_obj = sm.ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_id), ("done".into(), done_lit)],
    });
    sm.cur_buf.push(Stmt::Return(Some(final_obj)));
    sm.flush_cur();

    // Assemble: while (true) { if (st==0){arm0} if (st==1){arm1} ... ; catch-all }
    let mut loop_body: Vec<Stmt> = Vec::new();
    for (i, arm_stmts) in sm.arms.iter().enumerate() {
        let i_lit = ast.add_expr(Expr::Number(i as f64));
        let st_ref = ast.add_expr(Expr::Ident(
            super::desugar_generators_sm::RESUME_LOCAL.into(),
        ));
        let cond = ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: st_ref,
            right: i_lit,
        });
        loop_body.push(Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Block(arm_stmts.clone())),
            else_branch: None,
        });
    }
    // Catch-all for any state past the last allocated arm (covers
    // unreachable dead-states from break/continue and any "fell off
    // the end" case that didn't return inside the if-chain).
    let zero_tail = default_init_for_type(yield_ty);
    let zero_tail_id = ast.add_expr(zero_tail);
    let done_tail = ast.add_expr(Expr::Bool(true));
    let final_tail = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_tail_id), ("done".into(), done_tail)],
    });
    loop_body.push(Stmt::Return(Some(final_tail)));

    let true_lit = ast.add_expr(Expr::Bool(true));
    // Unreachable trailing return after the `while (true)` — the
    // typechecker's "all paths return" analysis doesn't infer that
    // a `cond=true` while never falls out, so without this the
    // function's tail path looks indeterminate. Cheap to emit, no
    // runtime cost (LLVM dead-code-eliminates it).
    let zero_after = default_init_for_type(yield_ty);
    let zero_after_id = ast.add_expr(zero_after);
    let done_after = ast.add_expr(Expr::Bool(true));
    let final_after = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_after_id), ("done".into(), done_after)],
    });
    // Prologue — take the resume label into a local and mark the
    // generator DEAD in the same breath (`-1`, the sentinel `return()`
    // and `throw()` already write). Only a yield writes the field back,
    // so ANY other way out of `next()` — running off the end, an early
    // `return`, a throw from the body or from anything it calls —
    // leaves the generator completed, per ES §27.5.1.2. See
    // `desugar_generators_sm::RESUME_LOCAL`.
    let this_read = ast.add_expr(Expr::This);
    let state_read = ast.add_expr(Expr::Member {
        obj: this_read,
        name: "__state".into(),
    });
    let seed_local = Stmt::LetDecl {
        mutable: true,
        name: super::desugar_generators_sm::RESUME_LOCAL.into(),
        type_ann: Some("number".into()),
        init: state_read,
        is_var: false,
    };
    let this_kill = ast.add_expr(Expr::This);
    let state_kill = ast.add_expr(Expr::Member {
        obj: this_kill,
        name: "__state".into(),
    });
    let dead_lit = ast.add_expr(Expr::Number(-1.0));
    let kill = ast.add_expr(Expr::Assign {
        target: state_kill,
        value: dead_lit,
    });
    vec![
        seed_local,
        Stmt::Expr(kill),
        Stmt::While {
            cond: true_lit,
            body: Box::new(Stmt::Block(loop_body)),
        },
        Stmt::Return(Some(final_after)),
    ]
}

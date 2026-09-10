//! The relay redirect — a `__forward_<t>` adapter that enters `t`
//! itself when `t` is on the head-less argv face.
//!
//! A `__forward_<t>` relay is minted for a value escape of a
//! head-less top-level fn: `(t as any)(…)`, `[t][0](…)`, `o.f = t`,
//! `take(t)`. Every any-world call of that value lands on the relay's
//! boxed adapter, which holds the true argument list (argc + a
//! pointer to that many NaN-boxes) — and then, before rotation 625,
//! called the relay's closure body, whose forwarding call `t(a, b)`
//! could only hand `t` the DECLARED parameters. `function g() {
//! return arguments[0] }` answered undefined through every escape
//! spelling while `g(7)` answered 7.
//!
//! The two values the argv face wants at a call are the two the
//! adapter already has, in the exact order the head-less signature
//! spells them (`[argc: I64, argv: Ptr, …declared]`, see
//! `ssa_lower_pass_1` H1 and `inject_argc_params`). So the adapter
//! for such a relay calls `t` directly: argc / argv through, the
//! declared slots unboxed per the relay's face — which is `t`'s own
//! face, cloned by the synthesizer — and nothing packed twice. The
//! relay body stays what it was for the other route: a typed direct
//! call (`call_indirect` on a closure-typed slot) can only carry
//! exactly the declared arguments, which on that route ARE the whole
//! list, so its forwarding call's static pack is already the truth.
//!
//! The target is whatever the relay's own forwarding call names —
//! read off the call, not off the `__forward_` prefix: a generic
//! target's relay is erased to `any`, and the monomorph pass
//! retargeted that call to the all-`any` clone (`t$$_any`), which is
//! the FnDecl on the face (the clone mirrors into the head-less
//! sets). Until rotation 626 the prefix was stripped instead, the
//! template name matched nothing, and every untyped-param target —
//! `function f(a) { … arguments … }`, the plain JavaScript spelling —
//! silently fell back to the relay body: `f(42, ...arr)` answered
//! `arguments.length` 1.
//!
//! Redirect only when the relay's face is the target's SSA face slot
//! for slot: a mismatch falls back to the relay body, never to a
//! mis-typed call.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, Stmt};
use crate::ssa::{self, FuncId, Type};

/// `Some(fid of t)` when `name` is a `__forward_` relay whose
/// forwarding call names `t`, `t` is on the head-less argv face, and
/// `t`'s signature is `[I64, Ptr, …]` with exactly the relay's user
/// types and return type behind the two synthetic slots.
pub(super) fn headless_target(
    ast: &Ast,
    call_retargets: &crate::ssa_lower::CallRetargets,
    name: &str,
    fn_table: &HashMap<String, FuncId>,
    fn_sigs: &[(Vec<Type>, Type)],
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    user_tys: &[Type],
    ret_ty: &Type,
) -> Option<FuncId> {
    name.strip_prefix("__forward_")?;
    let target = forwarding_call_target(ast, call_retargets, name)?;
    if !ast.headless_argv_fns.contains(&target) || !ast.headless_argc_fns.contains(&target) {
        return None;
    }
    let fid = *fn_table.get(&target)?;
    let (tys, ret) = &fn_sigs[fn_sig_ids.get(&fid)?.0 as usize];
    let face_matches = tys.len() == user_tys.len() + 2
        && tys[0] == Type::I64
        && tys[1] == Type::Ptr
        && tys[2..] == *user_tys
        && ret == ret_ty;
    face_matches.then_some(fid)
}

/// The fn the relay's body calls — `return t(…)` as
/// `forwarder_synth` spells it, after the monomorph retarget of that
/// call (a generic `t` becomes its clone's name).
fn forwarding_call_target(
    ast: &Ast,
    call_retargets: &crate::ssa_lower::CallRetargets,
    relay: &str,
) -> Option<String> {
    let body = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, body, .. } if name == relay => Some(body),
        _ => None,
    })?;
    let [Stmt::Return(Some(call))] = body.as_slice() else {
        return None;
    };
    if let Some(mono) = call_retargets.get(call) {
        return Some(mono.clone());
    }
    match ast.get_expr(*call) {
        Expr::Call { callee, .. } => match ast.get_expr(*callee) {
            Expr::Ident(n) => Some(n.clone()),
            _ => None,
        },
        _ => None,
    }
}

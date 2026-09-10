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
//! Redirect only when the relay's face is `t`'s SSA face slot for
//! slot (a generic target's relay is erased to `any` and its mono
//! clones live under other names): a mismatch falls back to the
//! relay body, never to a mis-typed call.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::ssa::{self, FuncId, Type};

/// `Some(fid of t)` when `name` is `__forward_<t>`, `t` is on the
/// head-less argv face, and `t`'s signature is `[I64, Ptr, …]` with
/// exactly the relay's user types and return type behind the two
/// synthetic slots.
pub(super) fn headless_target(
    ast: &Ast,
    name: &str,
    fn_table: &HashMap<String, FuncId>,
    fn_sigs: &[(Vec<Type>, Type)],
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    user_tys: &[Type],
    ret_ty: &Type,
) -> Option<FuncId> {
    let target = name.strip_prefix("__forward_")?;
    if !ast.headless_argv_fns.contains(target) || !ast.headless_argc_fns.contains(target) {
        return None;
    }
    let fid = *fn_table.get(target)?;
    let (tys, ret) = &fn_sigs[fn_sig_ids.get(&fid)?.0 as usize];
    let face_matches = tys.len() == user_tys.len() + 2
        && tys[0] == Type::I64
        && tys[1] == Type::Ptr
        && tys[2..] == *user_tys
        && ret == ret_ty;
    face_matches.then_some(fid)
}

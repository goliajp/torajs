//! The one `__forward_<target>` synthesizer.
//!
//! Four passes mint forwarders — [`super::forwarders`] (a fn-typed
//! `return f`), [`super::forwarders_object_synth`] (a member-assigned
//! `o.m = f`), [`super::namedfn_this_value`] (a `this`-reading
//! declaration used as a value) and
//! [`crate::ast_closure_param_tag_axes`] (a name reaching a retagged
//! lane). Each arrived with its own copy of the same decl shape, and
//! the copies had begun to drift: the fourth forwarded a rest param as
//! a bare ident where the other three spread it. A rule with four
//! transcripts is a rule that will be applied to three of them.
//!
//! The shape itself is fixed by what the callers of a forwarder expect:
//!
//! - **`__env` first.** A forwarder is a closure value; its callers
//!   invoke it through the closure ABI, which feeds the env pointer.
//! - **The hidden `__this` never enters the public face.** A
//!   `bind_this_param`-promoted target carries `__this` as its first
//!   declared param. Left in, it would become a declared slot of the
//!   forwarder and the caller's first real argument would land in the
//!   receiver — `function take(f) { return f(7) }; take(t)` seeds
//!   `__this = 7`. So it is skipped on the face and fed `undefined` by
//!   the forwarding call, the same plain-call receiver
//!   `this_param.rs`'s direct-call rewrite supplies.
//! - **A rest param forwards as a SPREAD.** `target(args)` would be
//!   re-packed by `apply_rest_args` (which runs after every one of
//!   these passes) into `target([args])`. `target(...args)` rides that
//!   pass's single-spread arm instead.
//! - **A `__torajs_argv` never enters the face either.** The relay's
//!   own routes supply the target's argv (see the synthesizer body).
//! - **A trailing `GEN_ARGV_PARAM` never enters the face.** An
//!   argv-consuming generator factory takes its argv from the
//!   forwarder's own `[...arguments]`, so the true call-site argv
//!   reaches the factory however the forwarder is invoked.
//! - **The span is the TARGET's.** `toString` answers the wrapped
//!   user function's source; the forwarder is that function as far as
//!   an observer is concerned.
//!
//! Not folded in: giving the public face a receiver-first channel.
//! That was tried twice (rotations 366 and 595) and reverted both
//! times; the audit at
//! `.claude/rfcs/20260905-named-fn-value-identity/boxed-entry-caller-audit.md`
//! says why it can only move per route. This file is that audit's §5.4
//! precondition — one place to apply a per-route conclusion, instead
//! of four places to apply it to three of.

use super::{push_gen_argv_spread, split_gen_argv_tail};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// Build `__forward_<target>(__env, ...user params) { return
/// target(...); }` from the target's own signature.
pub(crate) fn synth_forwarder_decl(
    ast: &mut Ast,
    forward_name: &str,
    target: &str,
    params: &[Param],
    return_type: Option<String>,
    target_span: crate::lexer::Span,
) -> Stmt {
    let twin = static_twin_target(ast, target);
    let (target, params, return_type) = match &twin {
        Some((name, params, ret)) => (name.as_str(), params.as_slice(), ret.clone()),
        None => (target, params, return_type),
    };
    let takes_this = params.first().is_some_and(|p| p.name == "__this");
    // A relay minted after `desugar_arguments_object` sees the
    // target's injected `__torajs_argv` (head-less: position 0;
    // this-first: right after `__this`). It never enters the face
    // and is never forwarded: the forwarding call's terminal packs
    // the callee's argv itself, and the relay's boxed adapter enters
    // the target with its own (`ssa_lower_boxed_entry::relay`).
    let peeled: Vec<Param> = params
        .iter()
        .filter(|p| p.name != "__torajs_argv")
        .cloned()
        .collect();
    let user_params = if takes_this {
        &peeled[1..]
    } else {
        &peeled[..]
    };
    let (user_params, takes_gen_argv) = split_gen_argv_tail(user_params);
    let user_params = user_params.to_vec();

    let mut fwd_params: Vec<Param> = Vec::with_capacity(user_params.len() + 1);
    fwd_params.push(Param {
        name: "__env".into(),
        type_ann: Some("__env()".to_string()),
        default: None,
        is_rest: false,
    });
    fwd_params.extend(user_params.iter().cloned());

    let mut arg_eids: Vec<ExprId> = Vec::with_capacity(user_params.len() + 1);
    if takes_this {
        arg_eids.push(ast.add_expr(Expr::Ident("undefined".into())));
    }
    for p in &user_params {
        let id = ast.add_expr(Expr::Ident(p.name.clone()));
        if p.is_rest {
            arg_eids.push(ast.add_expr(Expr::Spread { expr: id }));
        } else {
            arg_eids.push(id);
        }
    }
    if takes_gen_argv {
        push_gen_argv_spread(ast, &mut arg_eids);
    }

    let callee_id = ast.add_expr(Expr::Ident(target.to_string()));
    let call_id = ast.add_expr(Expr::Call {
        callee: callee_id,
        args: arg_eids,
    });
    Stmt::FnDecl {
        name: forward_name.to_string(),
        type_params: Vec::new(),
        params: fwd_params,
        return_type,
        body: vec![Stmt::Return(Some(call_id))],
        is_generator: false,
        span: target_span,
    }
}

/// A `this`-reading STATIC method's relay forwards to its
/// receiver-polymorphic twin, not to the mono.
///
/// The mono body `__sm_<C>__<m>` has no receiver channel: desugar
/// pass 2 resolved every `this` in it to `Ident(<C>)`, which is the
/// right answer for the direct `C.m()` call and the wrong one for
/// every escape — `var r = C.m; r()` binds undefined per §10.2.1.2
/// (a class body is strict code), and a relay that enters the mono
/// answered `this === C` true, `this.x` the class's field where the
/// spec throws. Knife 3a (RFC 20260804-fn-this-channel) minted
/// `__smany_<C>__<m>` beside such a mono with the receiver as its
/// leading `__this`; the relay forwards there, and the `undefined`
/// the caller seeds into a `__this` head is exactly the plain-call
/// receiver. The forwarder keeps the mono's `__forward_` name, so the
/// escaped value's identity, `.name` and `.length` are unchanged.
///
/// `None` for anything that is not a twin-bearing static: a this-free
/// static never observes its receiver, and a mint residue (super
/// route) keeps the recorded mono behavior.
fn static_twin_target(ast: &Ast, target: &str) -> Option<(String, Vec<Param>, Option<String>)> {
    let twin = ast.smany_twins.get(target)?;
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } if name == twin => Some((name.clone(), params.clone(), return_type.clone())),
        _ => None,
    })
}

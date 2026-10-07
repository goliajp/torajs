//! The one `__forward_<target>` synthesizer.
//!
//! Five passes mint forwarders — [`super::forwarders`] (a fn-typed
//! `return f`), [`super::forwarders_object_synth`] (a member-assigned
//! `o.m = f`), [`super::namedfn_this_value`] (a `this`-reading
//! declaration used as a value) and
//! [`crate::ast_closure_param_tag_axes`] (a name reaching a retagged
//! lane) and [`super::namedfn_recv_cb`] (a thisArg callback position,
//! receiver-first). Each arrived with its own copy of the same decl shape, and
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
//! The receiver-first face (`ShellFace::ReceiverFirst`, the
//! `__fwdrecv_<target>` shell `namedfn_recv_cb` mints for a thisArg
//! callback position) is the same synthesizer since rotation 649: it
//! keeps `__this` on the face and forwards it, everything else — the
//! `__torajs_argv` peel, the rest spread, the gen-argv tail, the
//! static twin — is shared. Which SITES get that face is still
//! decided per route: giving every value use the receiver-first face
//! was tried twice (rotations 366 and 595) and reverted both times —
//! it can only move per route. This file applies that per-route
//! conclusion.

use super::{push_gen_argv_spread, split_gen_argv_tail};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// Which public face the shell gets. A `bind_this_param`-promoted
/// target carries `__this` first; the plain face hides it and feeds
/// `undefined` (the plain-call receiver, §10.2.1.2), the
/// receiver-first face keeps it as the shell's first user param and
/// forwards it — the `__fwdrecv_<target>` shell a thisArg callback
/// position mints (`namedfn_recv_cb`), registered in
/// `fnexpr_recv_fns` so every receiver-aware invoker puts the
/// receiver in argv[0]. Both faces are one synthesizer since rotation
/// 649: the receiver-first shell used to be a second copy with no argv
/// relay and no rest / gen-argv handling.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellFace {
    Plain,
    ReceiverFirst,
}

impl ShellFace {
    /// The face a shell's name spells.
    pub(crate) fn of(shell: &str) -> Self {
        if shell.starts_with("__fwdrecv_") {
            Self::ReceiverFirst
        } else {
            Self::Plain
        }
    }
}

/// The declaration a synthesized relay shell forwards to, whichever
/// face it wears — `__forward_` and `__fwdrecv_` are two faces of the
/// same shell ([`ShellFace`]), minted by the same synthesizer from the
/// same signature.
///
/// Almost every question asked about a shell has the same answer for
/// both faces: which declaration is behind it, whether it is a relay
/// rather than a user function, whether it already exists so a second
/// one need not be minted. Asking with a single prefix is how a
/// receiver-first shell gets mistaken for an ordinary anonymous
/// closure — and the mistakes are quiet ones (a cloned singleton, a
/// second forwarder over a forwarder, a missing property bag).
pub(crate) fn shell_target(name: &str) -> Option<&str> {
    name.strip_prefix("__forward_")
        .or_else(|| name.strip_prefix("__fwdrecv_"))
}

/// Is this name one of the synthesized relay shells? See
/// [`shell_target`] for why both faces answer together.
pub(crate) fn is_shell(name: &str) -> bool {
    shell_target(name).is_some()
}

/// The names a declaration's shell can go by, receiver-first first.
///
/// The inverse of [`shell_target`], for the sites that go the other
/// way — they hold a declaration name and want the shell that was
/// minted for it. Which face that is depends on the declaration:
/// `promote_this_fn_values` mints the receiver-first one for every
/// `this`-reading declaration, and the plain face is what the other
/// synthesizers mint. Asking for one spelling finds nothing for half
/// the declarations, and "no shell" is a plausible-looking answer at
/// each of these sites (`instanceof` falls back to constant false, a
/// promise handler to the generic path, the param-tag axis to minting
/// a SECOND shell).
pub(crate) fn shell_names(target: &str) -> [String; 2] {
    [format!("__fwdrecv_{target}"), format!("__forward_{target}")]
}

/// Build `__forward_<target>(__env, ...user params) { return
/// target(...); }` (or the receiver-first `__fwdrecv_<target>(__env,
/// __this, ...user)`) from the target's own signature.
pub(crate) fn synth_forwarder_decl(
    ast: &mut Ast,
    forward_name: &str,
    target: &str,
    params: &[Param],
    return_type: Option<String>,
    target_span: crate::lexer::Span,
    face: ShellFace,
) -> Stmt {
    let twin = static_twin_target(ast, target);
    let (target, params, return_type) = match &twin {
        Some((name, params, ret)) => (name.as_str(), params.as_slice(), ret.clone()),
        None => (target, params, return_type),
    };
    // 402-01 — a GENERIC target's signature still spells its own
    // TypeVars (`gid<T>(v: T): T`), but the shell is a plain FnDecl
    // with no type params to bind them — the checker rejects it loud
    // ("unknown type `T`"). Erase every TypeVar to `any`: the shell
    // body's `target(v)` call is then an ordinary generic call site
    // the checker infers at Any, and the mono pass emits the `$$_any`
    // specialization the call retargets to. Until rotation 650 only
    // the member-assigned mint (`forwarders_object_synth`) erased, so
    // a generic `this`-reading declaration at a thisArg callback
    // position (`[3].map(g, o)`) died at "unknown type `T` for
    // parameter `a` of function `__fwdrecv_g`".
    let (params, return_type) = erase_generic_typevars(ast, target, params.to_vec(), return_type);
    let params = params.as_slice();
    let takes_this = params.first().is_some_and(|p| p.name == "__this");
    let forced_recv = takes_this && face == ShellFace::ReceiverFirst;
    // A relay minted after `desugar_arguments_object` sees the
    // target's injected `__torajs_argv` (head-less: position 0;
    // this-first: right after `__this`). It never enters the face
    // and is never forwarded: the forwarding call's terminal packs
    // the callee's argv itself, and the relay's boxed adapter enters
    // the target with its own (`ssa_lower_boxed_entry::relay`).
    //
    // A `new.target` head slot (`new_target_param.rs`) is peeled the
    // same way: a value face is only ever [[Call]]ed, which passes
    // undefined.
    let takes_new_target = params
        .get(1)
        .is_some_and(|p| p.name == crate::ast::new_target_param::NEW_TARGET);
    let peeled: Vec<Param> = params
        .iter()
        .filter(|p| p.name != "__torajs_argv" && p.name != crate::ast::new_target_param::NEW_TARGET)
        .cloned()
        .collect();
    let user_params = if takes_this && !forced_recv {
        &peeled[1..]
    } else {
        &peeled[..]
    };
    if forced_recv {
        ast.fnexpr_recv_fns.insert(forward_name.to_string());
    }
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
    if takes_this && !forced_recv {
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
    if takes_new_target {
        let undef = ast.add_expr(Expr::Ident("undefined".into()));
        arg_eids.insert(1, undef);
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

/// 402-01 — substitute a generic target's own TypeVars with `any`
/// across the forwarder's param / return annotations (the target's
/// `type_params` are read off its FnDecl in place, so every caller
/// of the synthesizer gets the erasure without threading a table).
/// Non-generic targets pass through untouched.
fn erase_generic_typevars(
    ast: &Ast,
    target: &str,
    params: Vec<Param>,
    return_type: Option<String>,
) -> (Vec<Param>, Option<String>) {
    let type_params: Vec<String> = ast
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::FnDecl {
                name, type_params, ..
            } if name == target => Some(type_params.clone()),
            _ => None,
        })
        .unwrap_or_default();
    if type_params.is_empty() {
        return (params, return_type);
    }
    let subst: Vec<(String, String)> = type_params
        .into_iter()
        .map(|tp| (tp, "any".to_string()))
        .collect();
    let params = params
        .into_iter()
        .map(|mut p| {
            if let Some(a) = &mut p.type_ann {
                *a = crate::ssa_lower_generics_monomorph::substitute_in_ann(a, &subst);
            }
            p
        })
        .collect();
    let return_type =
        return_type.map(|rt| crate::ssa_lower_generics_monomorph::substitute_in_ann(&rt, &subst));
    (params, return_type)
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

//! The never-calls VALUE-position classifiers (rotation 410 split
//! from `fnexpr_this_args.rs` at the 500-line cap): an equality
//! operand, the right of `instanceof`, a `typeof` operand, and the
//! audited `Object.*` argument slots. The proof family is "it never
//! calls the binding" (`fnexpr_this_shapes` module doc, kind 1) — the
//! sibling keeps the any-escape / call-channel positions (kind 2),
//! whose proof rides FLAG_CLOSURE_RECV_FIRST instead.

use super::{Expr, ExprId, Stmt};

/// The bare name in `export default <name>`.
///
/// By the time this pass runs, module resolution has already happened:
/// an importer that asked for the default got a synthetic
/// `let <alias> = <name>` (`modules::materialize`), which is an
/// ordinary alias declaration and proves itself through the alias
/// fixpoint. What is still spelled as an `ExportDecl` is therefore the
/// ENTRY module's export, which nothing imports and which
/// `ssa_lower_stmt_dispatch` drops. Either way the position invokes
/// nothing.
///
/// This is the last shape between the ES5 class lane and the
/// `export default class extends <expr> {}` spelling — the one every
/// top-level-await heritage case in t262 is written in.
pub(super) fn export_default_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::ExportDecl {
                default_expr: Some(e),
                ..
            } => Some(*e),
            _ => None,
        })
        .filter(|e| matches!(&exprs[e.0 as usize], Expr::Ident(_)))
        .collect()
}

/// The BARE NAME under `typeof` (§13.5.3).
///
/// The thinnest never-calls position there is: the operator resolves
/// the reference, and answers a string picked from the value's type.
/// It reads no property, coerces nothing, and invokes nothing — less
/// contact with the cell than a `.prototype` read or the right of
/// `instanceof`, both of which already admit.
///
/// This is what the ES5 class lane hits every time a program asks
/// `typeof K` about a class whose heritage is a value expression
/// (`class D extends (calls++, C) {}`, `class extends fn(x)`). That
/// lane lowers the class to `let K = function (…) { … this … }`, so
/// the constructor's `this` needs the knife-2 promotion — and one
/// `typeof K` anywhere in the program refuted it, leaving `__this`
/// an unbound capture. The t262 spelling of "did the class evaluate"
/// is `assert.sameValue(typeof K, 'function')`, so the assertion the
/// tests use to observe the lane was the thing disabling it.
///
/// Only the bare-name spelling qualifies, matching `instanceof`: a
/// larger operand is a value expression that may itself call.
pub(super) fn typeof_operand_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    exprs
        .iter()
        .filter_map(|e| match e {
            Expr::TypeOf { expr } if matches!(&exprs[expr.0 as usize], Expr::Ident(_)) => {
                Some(*expr)
            }
            _ => None,
        })
        .collect()
}

/// B6 刀 2 — an EQUALITY operand (`result.constructor === C`, the
/// t262 identity-assert spelling) is the fifth receiver-safe use
/// shape: like a `.prototype` read it enters no call lane at all —
/// the comparison consumes the cell pointer as a value. Ordering /
/// arithmetic operands stay loud (they coerce, which is observable
/// and untested here).
pub(super) fn eq_operand_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    exprs
        .iter()
        .filter_map(|e| match e {
            Expr::BinOp {
                op:
                    super::BinOp::Eq
                    | super::BinOp::Neq
                    | super::BinOp::LooseEq
                    | super::BinOp::LooseNeq,
                left,
                right,
            } => Some([*left, *right]),
            _ => None,
        })
        .flatten()
        .filter(|a| matches!(&exprs[a.0 as usize], Expr::Ident(_)))
        .collect()
}

/// The seventh receiver-safe use shape: the BARE NAME on the right of
/// `instanceof`.
///
/// Safer than the `.prototype` read above — that one at least reads a
/// property off the cell, while this position materialises no value at
/// all. `Expr::InstanceOf`'s target used to be a `String` field, so
/// this use was invisible to the parity scan; once it became an
/// ordinary `Expr::Ident` (rotation 390) every `x instanceof f` in a
/// program silently un-promoted `f`'s binding, and the `__this` the
/// promotion was there to bind went back to being a capture the
/// checker rejects.
///
/// Only the bare-name spelling qualifies: a larger target IS a value
/// expression and takes the runtime operator, which calls a handler.
pub(super) fn instanceof_name_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    exprs
        .iter()
        .filter_map(|e| match e {
            Expr::InstanceOf { rhs, .. } if matches!(&exprs[rhs.0 as usize], Expr::Ident(_)) => {
                Some(*rhs)
            }
            _ => None,
        })
        .collect()
}

/// The ninth receiver-safe use shape: the TARGET argument of
/// `Object.defineProperty` / `Object.defineProperties`.
///
/// §20.1.2.4 does four things to `O` — reject a non-object, take a
/// property key off the second argument, build a descriptor from the
/// third, and DefinePropertyOrThrow into it. Not one of them invokes
/// `O`, so this position is the "never calls the binding" kind of
/// proof, the same one behind a member's object and the right of
/// `instanceof` — not the escape kind, which needs the value to reach
/// the any lane. `defineProperties` (§20.1.2.5) is the same shape with
/// the keys in a bag.
///
/// Index 0 only. Standing as the DESCRIPTOR is a different question:
/// `ToPropertyDescriptor` reads `.get` / `.set` off it and installs
/// what it finds as an accessor, which is a call path — and the key
/// argument is coerced, which is observable and untested here.
///
/// This is the wall three faces of the capturing-class lane were
/// waiting on. A class member is non-enumerable (§15.7.14), which
/// means `defineProperty`, which means the class binding lands in an
/// argument — fine on `K.prototype`, where the binding still stands
/// under a named member, but a STATIC member passes the binding
/// itself. So static members stayed assignments (enumerable, wrongly),
/// static accessors and computed static members were declined
/// outright, and a keyed store could not join the member-object shape
/// because that one excludes `.call` / `.apply` / `.bind` by NAME —
/// three names a runtime key defeats. Handing the key to
/// `defineProperty` as data is what dissolves that.
///
/// `Object.setPrototypeOf(D, P)` joins with BOTH argument positions
/// (405-01): §20.1.2.21 validates the proto, then writes an internal
/// slot — neither argument is ever invoked. This is the class-side
/// static-inheritance statement the extends lane mints, and both
/// spellings in it are lane bindings.
///
/// `Object.getPrototypeOf(O)` joins (rotation 410): §20.1.2.12 is a
/// single internal-slot read — the argument is never invoked. First
/// surfaced by `Object.getPrototypeOf(K)` on a value-shaped-parent
/// class binding, which refuted the implicit ctor's promotion and
/// left its `__this` capture unbound.
pub(super) fn define_property_target_idents(exprs: &[Expr]) -> std::collections::HashSet<ExprId> {
    exprs
        .iter()
        .flat_map(|e| -> Vec<ExprId> {
            let Expr::Call { callee, args } = e else {
                return Vec::new();
            };
            let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
                return Vec::new();
            };
            if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Object") {
                return Vec::new();
            }
            match name.as_str() {
                "defineProperty" | "defineProperties" => {
                    args.first().copied().into_iter().collect()
                }
                "setPrototypeOf" => args.iter().take(2).copied().collect(),
                "getPrototypeOf" => args.first().copied().into_iter().collect(),
                _ => Vec::new(),
            }
        })
        .filter(|a| matches!(&exprs[a.0 as usize], Expr::Ident(_)))
        .collect()
}

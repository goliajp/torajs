//! Static-call receiver routing — the walks that run BEFORE
//! `desugar_classes`' flat `C.m` → `Ident("__sm_C__m")` rewrite,
//! because the shape they key on (a Member callee naming the
//! ACCESSING class) is exactly what that rewrite erases.
//!
//! Moved out of `desugar_classes.rs` verbatim (rotation 627) when the
//! file reached 492 lines and a second such walk had nowhere to go.
//! Both walks answer the same question — which static call carries a
//! receiver the flat rewrite would lose — for two spellings: an
//! inherited `Sub.m(…)` and a cast `(C.m as any)(…)`.

use super::desugar_classes_statics::InheritedStaticOwners;
use super::{Ast, Expr, PropKey};

/// RFC 20260804-fn-this-channel knife 3d — give `Sub.<inherited
/// static>(…)` its receiver back.
///
/// Runs BEFORE the flat rewrite below, for the same reason the accessor
/// walk does: it consumes the whole `Sub.make(…)` Call, and the shape it
/// needs — a Member callee naming the ACCESSING class — is exactly what
/// the flat rewrite erases.
///
/// The V3-18 inheritance walk aliases `Sub.make` onto the owner's
/// binding, so the flat rewrite below turns the call into a direct
/// `__sm_Base__make(3)`. That mono carries no receiver channel and Pass
/// 2 already minted its `this` to the DECLARING class name, so a static
/// factory silently built a `Base` where §15.7.14 says the receiver is
/// the `Sub` constructor object (`Sub.make(3) instanceof Sub`).
///
/// Only calls are routed, and only when the owner's body actually reads
/// `this` (that is precisely when a twin got minted). Everything else
/// keeps today's direct-mono dispatch:
///   - the class's OWN static — receiver is the declaring class, so the
///     minted name is already the right answer;
///   - a `this`-free inherited body — runs identically under any
///     receiver, and routing it would only cost an indirection;
///   - `Sub.make` read as a VALUE — an unbound function per §15.7.14,
///     whose receiver is decided at the eventual call site, not here.
///
/// The receiver argument is the accessing class name itself. It survives
/// the flat rewrite (a bare `Ident` is not a static-member Member) and
/// resolves to the class-object global, which is the same value the
/// already-working `Base.make.call(Sub, …)` path hands the twin.
pub(super) fn route_inherited_static_calls(
    ast: &mut Ast,
    inherited_static_owners: &InheritedStaticOwners,
) {
    if inherited_static_owners.is_empty() {
        return;
    }
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let (callee, args) = (*callee, args.clone());
        let Expr::Member { obj, name } = &ast.exprs[callee.0 as usize] else {
            continue;
        };
        let Expr::Ident(class_name) = &ast.exprs[obj.0 as usize] else {
            continue;
        };
        let key = (class_name.clone(), PropKey::from(name));
        let Some(owner) = inherited_static_owners.get(&key) else {
            continue;
        };
        let Some(twin) = ast
            .smany_twins
            .get(&format!("__sm_{owner}__{name}"))
            .cloned()
        else {
            continue;
        };
        let twin_callee = ast.add_expr(Expr::Ident(twin));
        let recv = ast.add_expr(Expr::Ident(key.0));
        let mut new_args = Vec::with_capacity(args.len() + 1);
        new_args.push(recv);
        new_args.extend(args);
        ast.exprs[i] = Expr::Call {
            callee: twin_callee,
            args: new_args,
        };
    }
}

/// `(C.m as any)(…)` keeps its receiver.
///
/// TypeScript erases the cast, so the program bun runs is `C.m(…)`: a
/// member call whose `this` is the class object (§13.3.6.1
/// EvaluateCall binds the reference's base). The flat rewrite turns
/// the Member into `Ident("__sm_C__m")` under the `As`, and the cast
/// then makes the value ESCAPE — a `__forward_` relay with the
/// plain-call `undefined` receiver — so `this === C` answered false
/// and a `this.x` read threw where bun reads the field. Route the
/// call to the receiver-polymorphic twin with the accessing class as
/// its `__this`: the twin's all-`any` face is also what `as any`
/// asked for (no static arity or argument-type check).
///
/// Only a twin-bearing (this-reading) static is routed; a this-free
/// one runs identically through the relay. Only `as any` and the `!`
/// non-null assertion are admitted — a cast to a concrete fn type is
/// a typed value whose call the checker owns (registered residue).
pub(super) fn route_cast_static_calls(
    ast: &mut Ast,
    inherited_static_owners: &InheritedStaticOwners,
) {
    if ast.smany_twins.is_empty() {
        return;
    }
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let (callee, args) = (*callee, args.clone());
        let Expr::As { expr, ty_ann } = &ast.exprs[callee.0 as usize] else {
            continue;
        };
        if ty_ann != "any" && ty_ann != "__nonnull__" {
            continue;
        }
        let Expr::Member { obj, name } = &ast.exprs[expr.0 as usize] else {
            continue;
        };
        let Expr::Ident(class_name) = &ast.exprs[obj.0 as usize] else {
            continue;
        };
        let key = (class_name.clone(), PropKey::from(name));
        let owner = inherited_static_owners.get(&key).unwrap_or(class_name);
        let Some(twin) = ast
            .smany_twins
            .get(&format!("__sm_{owner}__{name}"))
            .cloned()
        else {
            continue;
        };
        let twin_callee = ast.add_expr(Expr::Ident(twin));
        let recv = ast.add_expr(Expr::Ident(key.0));
        let mut new_args = Vec::with_capacity(args.len() + 1);
        new_args.push(recv);
        new_args.extend(args);
        ast.exprs[i] = Expr::Call {
            callee: twin_callee,
            args: new_args,
        };
    }
}

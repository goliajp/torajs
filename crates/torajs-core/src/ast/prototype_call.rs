//! `X.prototype.m.call(...)` direct-method rewrite (chunk 430),
//! extracted verbatim from ast.rs. Re-exported from `crate::ast` so
//! torajs-cli callers keep the canonical
//! `ast::desugar_prototype_call` path.

use super::*;

/// V3-18 m2.f — rewrite `X.prototype.foo.call(recv, ...args)` to
/// the equivalent direct-method form `recv.foo(...args)`. Tora has
/// no real prototype object so the literal traversal would fail at
/// `Number.prototype.toString` (Type::Null doesn't have .toString).
/// Pattern-matched at the AST level so check.rs / ssa_lower see only
/// the rewritten form. Ns coverage: every constructor namespace
/// listed (Number / String / Boolean / Object / Array / BigInt /
/// Symbol / Function / Date / RegExp / Error). `.apply(recv, args)`
/// is similar but takes args as an array — handled in a follow-up
/// when an array-spread call shape lands.
pub fn desugar_prototype_call(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let Expr::Call { callee, args } = ast.exprs[i].clone() else {
            continue;
        };
        let Expr::Member {
            obj: outer_obj,
            name: outer_name,
        } = ast.get_expr(callee).clone()
        else {
            continue;
        };
        if outer_name != "call" || args.is_empty() {
            continue;
        }
        let Expr::Member {
            obj: inner_obj,
            name: method_name,
        } = ast.get_expr(outer_obj).clone()
        else {
            continue;
        };
        let Expr::Member {
            obj: ns_id,
            name: proto_name,
        } = ast.get_expr(inner_obj).clone()
        else {
            continue;
        };
        if proto_name != "prototype" {
            continue;
        }
        let Expr::Ident(ns) = ast.get_expr(ns_id).clone() else {
            continue;
        };
        let known_ns = matches!(
            ns.as_str(),
            "Number"
                | "String"
                | "Boolean"
                | "BigInt"
                | "Symbol"
                | "Object"
                | "Array"
                | "Function"
                | "Date"
                | "RegExp"
                | "Error"
                | "Promise"
                | "Map"
                | "Set"
        );
        if !known_ns {
            continue;
        }
        // RFC 20260713-array-proto-residual blade 2 — `Object.
        // prototype.toString.call(x)` SKIPS the rewrite: `x.
        // toString()` is the receiver's OWN toString (an array
        // joins), not the §20.1.3.6 badge classifier. The runtime
        // path reifies the distinct badge cell (proto alias) and
        // the `.call` short-circuit re-dispatches its carried mid.
        if ns == "Object" && method_name == "toString" {
            continue;
        }
        // RFC 20260712-array-generic-receiver chunks 2+3a — the
        // Array read family SKIPS the rewrite: `recv.m(args)` is
        // semantically wrong for it (a receiver's own `m` would
        // shadow the explicitly-called Array.prototype.m, and a
        // plain-object receiver needs the runtime's ES generic
        // array-like arm, which dispatches the reified cell's
        // carried mid with NULL name bytes). Mutators (push / pop /
        // splice …) keep the rewrite until the generic write family
        // lands — their runtime `.call` re-dispatch cannot write a
        // grow-relocated Arr receiver back (recorded boundary).
        // Name list mirrors torajs-anyvalue
        // `method_call_arraylike::arraylike_supported` — extend
        // together.
        if ns == "Array"
            && matches!(
                method_name.as_str(),
                "indexOf"
                    | "lastIndexOf"
                    | "includes"
                    | "at"
                    | "join"
                    | "toString"
                    | "slice"
                    | "every"
                    | "some"
                    | "forEach"
                    | "map"
                    | "filter"
                    | "find"
                    | "findIndex"
                    | "findLast"
                    | "findLastIndex"
                    | "reduce"
                    | "reduceRight"
            )
        {
            continue;
        }
        let recv = args[0];
        let rest = args[1..].to_vec();
        let new_callee = ast.add_expr(Expr::Member {
            obj: recv,
            name: method_name,
        });
        ast.exprs[i] = Expr::Call {
            callee: new_callee,
            args: rest,
        };
    }
}

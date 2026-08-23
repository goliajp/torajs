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
        // §20.1.3 — the other Object.prototype methods run ToObject
        // on their receiver, which throws a runtime TypeError on
        // null / undefined; the rewrite turns that into a compile-
        // time member reject (`undefined.hasOwnProperty`). Same
        // literal-receiver rule as Function's `call`/`apply` below:
        // skip exactly when the receiver is the `null` literal or
        // the `undefined` name — the reified cell's `.call`
        // short-circuit runs the real ToObject gate (and
        // `isPrototypeOf`'s primitive-V-first ordering) — and every
        // other receiver keeps the rewrite the checker resolves.
        if ns == "Object" {
            let nullish_literal = matches!(ast.get_expr(args[0]), Expr::Null)
                || matches!(ast.get_expr(args[0]), Expr::Ident(n) if n == "undefined");
            if nullish_literal {
                continue;
            }
        }
        // §20.5.3.4 — `Error.prototype.toString.call(x)` SKIPS the
        // rewrite for the same reason: `x.toString()` is the
        // receiver's OWN toString (a plain object answers the badge),
        // not the generic Get(name)/Get(message) error formatter. The
        // runtime path reads Error.prototype's own `toString` entry
        // (the dedicated ANY_METHOD_ERROR_TO_STRING cell) and the
        // `.call` short-circuit re-dispatches its carried mid.
        if ns == "Error" && method_name == "toString" {
            continue;
        }
        // §23.1.3 "intentionally generic" — the Array read family
        // SKIPS the rewrite: `recv.find(...)` is the receiver's OWN
        // find (a TypedArray receiver's §23.2.3 twin ValidateTypedArray-
        // throws on an out-of-bounds view exactly where the generic
        // scan answers empty — the resizable-buffer families count
        // that difference). The runtime path reifies the
        // Array-prototype-minted cell and the `.call` short-circuit's
        // family gate routes a non-Array receiver through the
        // array-like generic arm. Mutators keep the rewrite (the
        // array-like write face is dynobj-shaped).
        if ns == "Array"
            && matches!(
                method_name.as_str(),
                "find"
                    | "findIndex"
                    | "findLast"
                    | "findLastIndex"
                    | "forEach"
                    | "some"
                    | "every"
                    | "indexOf"
                    | "lastIndexOf"
                    | "includes"
                    | "at"
                    | "join"
                    | "map"
                    | "filter"
                    | "slice"
                    | "reduce"
                    | "reduceRight"
                    | "flat"
                    | "flatMap"
                    | "toReversed"
                    | "toSorted"
                    | "toSpliced"
                    | "with"
            )
        {
            continue;
        }
        // §20.4.3 — the WHOLE Symbol namespace SKIPS the rewrite:
        // toString / valueOf run thisSymbolValue, which throws a
        // TypeError on every non-Symbol receiver — `recv.m()` would
        // run the receiver's OWN m instead ("not-ok".toString()
        // answers itself, [] joins). The runtime path reifies the
        // tag-5 alias cells (ANY_METHOD_SYMBOL_TO_STRING/_VALUE_OF)
        // and the `.call` short-circuit re-dispatches the gate.
        if ns == "Symbol" {
            continue;
        }
        // RFC 20260713-string-proto-residual blade 6 — the String
        // generic family SKIPS the rewrite: §22.1.3 methods accept
        // any this and coerce via ToString (observable
        // OrdinaryToPrimitive order, trim 15.5.4.20-2-42), which
        // `recv.m()` cannot express — a plain-object receiver has no
        // `m` (checker reject / runtime garbage throw), and an own
        // `m` would shadow the explicitly-called builtin. The
        // runtime path reifies the method cell and the `.call`
        // re-dispatch coerces (`generic_str_this`). `toString` and
        // `valueOf` used to keep the rewrite (the thisStringValue
        // mid-aliasing boundary) — they now join the brand-checked
        // block below. `toLocaleString` is the one that still keeps
        // it: §20.1.4.6 is the inherited generic, never brand-checked,
        // and the rewritten form is what the checker resolves.
        if ns == "String" && method_name != "toLocaleString" {
            continue;
        }
        // §21.1.3 thisNumberValue / §20.3.3 thisBooleanValue /
        // §22.1.3.28 thisStringValue — the brand-checked wrapper
        // methods SKIP the rewrite (String's are caught by the block
        // above, which now skips everything but toLocaleString): they
        // throw
        // a TypeError on a receiver of the wrong brand, which
        // `recv.m()` cannot express (a plain object answers its OWN
        // toString, i.e. the badge). The runtime path reifies the
        // family-tagged cell and the `.call` short-circuit runs the
        // brand gate (`generic_builtin_this`) — the same gate the
        // through-a-binding form (`const m = Boolean.prototype
        // .toString; m.call({})`) already reaches.
        if matches!(ns.as_str(), "Number" | "Boolean")
            && matches!(method_name.as_str(), "toString" | "valueOf")
        {
            continue;
        }
        // RFC 20260712-array-generic-receiver chunks 2+3a + RFC
        // 20260721-array-proto-cluster 刀 8-B — the WHOLE Array
        // namespace SKIPS the rewrite: `recv.m(args)` is
        // semantically wrong for it (a receiver's own `m` would
        // shadow the explicitly-called Array.prototype.m, and a
        // plain-object receiver needs the runtime's ES generic
        // array-like arm, which dispatches the reified cell's
        // carried mid with NULL name bytes). The mutator boundary
        // this skip used to carve out (grow-relocated Arr receiver
        // writeback through an argv re-dispatch) no longer exists —
        // push 1→9 and splice 2→10 growth through the reified
        // cell's `.call` short-circuit read back bun-equal.
        if ns == "Array" {
            continue;
        }
        // Rotation 431 — the remaining WHOLE-namespace brand-checked
        // families SKIP the rewrite for the Number/Boolean reason
        // above, extended to every member: §21.4.4 thisTimeValue
        // (Date), §24.1.3 thisMapObject / §24.2.3 thisSetObject, and
        // the §27.2.5.4 promise brand. Rewriting to `recv.m()` turns
        // the spec's runtime TypeError on a wrong-brand receiver
        // into a compile-time member reject (t262 probes the throw
        // with try/catch), and a plain object's OWN `m` would shadow
        // the explicitly-called builtin. The reified proto method
        // cell's `.call` short-circuit already runs the brand gate —
        // the through-a-binding form reads back bun-equal on both
        // the legal and wrong-brand faces.
        if matches!(ns.as_str(), "Date" | "Map" | "Set" | "Promise") {
            continue;
        }
        // §20.2.3 Function.prototype — `bind` / `toString` join the
        // whole-member skip (the reified cell's dispatch runs the
        // IsCallable gate). `call` / `apply` CANNOT blanket-skip:
        // the nested legal form `Function.prototype.call.call(f,
        // recv, …)` only works through the rewrite today (the cell
        // short-circuit does not thread the double-`call`
        // this-shift). They skip exactly when the receiver is a
        // literal the spec's IsCallable gate must reject at runtime
        // — a number / string / bool / null literal or the
        // `undefined` name, the t262 this-not-callable shape; every
        // other receiver keeps the rewrite.
        if ns == "Function" {
            let wrong_brand_literal = matches!(
                ast.get_expr(args[0]),
                Expr::Number(_) | Expr::Bool(_) | Expr::String(_) | Expr::Null
            ) || matches!(ast.get_expr(args[0]), Expr::Ident(n) if n == "undefined");
            if !matches!(method_name.as_str(), "call" | "apply") || wrong_brand_literal {
                continue;
            }
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

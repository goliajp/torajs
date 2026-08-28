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
        if skips_rewrite(&ns, &method_name, ast.get_expr(args[0])) {
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

/// Which `X.prototype.m` may NOT be rewritten to `recv.m(...)`.
///
/// The rewrite exists because tr has no literal prototype object to
/// traverse, and for most methods `recv.m(...)` is the same call. For
/// these it is not, and the two reasons recur: the receiver's OWN `m`
/// would shadow the very function the program named, and a spec gate
/// the method runs on its receiver (a brand check, ToObject, ToString
/// coercion) becomes a compile-time member reject instead of the
/// runtime TypeError a test can catch. Each arm below records which
/// of the two it is answering, because the list only stays decidable
/// while every entry can name its reason.
///
/// `recv` is the `.call` receiver ARGUMENT, which two arms read: the
/// spec gate they protect fires on a literal the checker would
/// otherwise reject first.
fn skips_rewrite(ns: &str, method_name: &str, recv: &Expr) -> bool {
    // The WHOLE `Object.prototype` surface SKIPS the rewrite,
    // for the reason `toString` already did (RFC
    // 20260713-array-proto-residual blade 2): `recv.m(...)` is
    // the RECEIVER's `m`, and naming %Object.prototype% is
    // precisely how a program says it does not want that one.
    // `Object.prototype.hasOwnProperty.call({ hasOwnProperty:
    // () => "own", a: 1 }, "a")` answered "own" where the spec
    // says true — the function being called shadowed by the
    // object it was called on. `valueOf` did the same.
    //
    // The rewrite is also lossy about the receiver's CHAIN,
    // which the same-shaped `recv.m(...)` has to consult and the
    // `.call` spelling must not: every `groups` object
    // (§22.2.7.2) and `Array.prototype[Symbol.unscopables]`
    // (§23.1.3.35) is OrdinaryObjectCreate(null), so the rewrite
    // turned the test262 property helpers' probes into
    // TypeErrors the moment tr started refusing an inherited
    // surface a null prototype does not have.
    //
    // The runtime path handles all of it: the reified cell's
    // `.call` short-circuit re-dispatches the carried mid with
    // the thisArg as receiver, which is the ToObject gate
    // (§20.1.3) and `isPrototypeOf`'s primitive-V-first ordering
    // (§20.1.3.3) both. The nullish-literal carve-out this
    // replaces existed to reach that same path for one receiver
    // shape; now every shape reaches it.
    if ns == "Object" {
        return true;
    }
    // §20.5.3.4 — `Error.prototype.toString.call(x)` SKIPS the
    // rewrite for the same reason: `x.toString()` is the
    // receiver's OWN toString (a plain object answers the badge),
    // not the generic Get(name)/Get(message) error formatter. The
    // runtime path reads Error.prototype's own `toString` entry
    // (the dedicated ANY_METHOD_ERROR_TO_STRING cell) and the
    // `.call` short-circuit re-dispatches its carried mid.
    if ns == "Error" && method_name == "toString" {
        return true;
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
            method_name,
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
                | "entries"
                | "keys"
                | "values"
                | "fill"
                | "copyWithin"
                | "reverse"
                | "sort"
        )
    {
        return true;
    }
    // §20.4.3 — the WHOLE Symbol namespace SKIPS the rewrite:
    // toString / valueOf run thisSymbolValue, which throws a
    // TypeError on every non-Symbol receiver — `recv.m()` would
    // run the receiver's OWN m instead ("not-ok".toString()
    // answers itself, [] joins). The runtime path reifies the
    // tag-5 alias cells (ANY_METHOD_SYMBOL_TO_STRING/_VALUE_OF)
    // and the `.call` short-circuit re-dispatches the gate.
    if ns == "Symbol" {
        return true;
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
        return true;
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
    if matches!(ns, "Number" | "Boolean") && matches!(method_name, "toString" | "valueOf") {
        return true;
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
        return true;
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
    if matches!(ns, "Date" | "Map" | "Set" | "Promise") {
        return true;
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
            recv,
            Expr::Number(_) | Expr::Bool(_) | Expr::String(_) | Expr::Null
        ) || matches!(recv, Expr::Ident(n) if n == "undefined");
        if !matches!(method_name, "call" | "apply") || wrong_brand_literal {
            return true;
        }
    }
    false
}

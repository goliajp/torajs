//! Program-level adapter-admission predicates — which classes'
//! factories are worth a boxed adapter at all. Split from the parent
//! when the rotation-451 Promise-heir arm pushed it past the
//! 500-line file rule (verbatim moves).

use crate::ast::Ast;

/// S-NEW 刀 2 — factory adapters exist only so
/// `__torajs_anyv_construct` can reach a class through a value. A
/// program that never constructs from a value gets none of them: one
/// extra function per class is a real cost to an artifact whose size
/// is a differentiator. r293 — `Reflect.construct` reaches the same
/// registry through its own kernel, so its call shape arms the
/// synthesis too.
pub(crate) fn program_constructs_from_value(ast: &Ast) -> bool {
    ast.exprs.iter().any(|e| match e {
        crate::ast::Expr::NewDynamic { .. } => true,
        // A `.construct(...)` call arms regardless of the receiver
        // shape: the direct `Reflect.construct` form is provable,
        // but an alias (`const R: any = Reflect; R.construct(C,
        // ...)`) reaches the same kernel through the ns-object
        // singleton's cell, and the AST cannot see through the
        // binding. The method NAME is the signal; a user object's
        // own `construct` method false-positives into one adapter
        // per class — the pay-for-use trade the species
        // `constructor`-write arm below already makes.
        crate::ast::Expr::Call { callee, .. } => match ast.get_expr(*callee) {
            crate::ast::Expr::Member { name, .. } if name == "construct" => true,
            // Rotation 650 — a Promise settle static reached with an
            // explicit receiver (`Promise.reject.call(P, v)`, `.apply`,
            // `.bind`) runs NewPromiseCapability(|this|) through the
            // construct channel, and |this| may be any class object
            // (§27.2.4.6 step 1 asks only IsConstructor). Without the
            // adapters a class with no Promise in its chain stopped at
            // "cannot be reached through a runtime value yet".
            crate::ast::Expr::Member { obj, name }
                if matches!(name.as_str(), "call" | "apply" | "bind") =>
            {
                // `(Promise.reject as any).call(…)` — the cast is the
                // spelling the checker asks for; peel it.
                let obj = match ast.get_expr(*obj) {
                    crate::ast::Expr::As { expr, .. } => *expr,
                    _ => *obj,
                };
                matches!(ast.get_expr(obj), crate::ast::Expr::Member { obj: ns, .. }
                    if matches!(ast.get_expr(*ns), crate::ast::Expr::Ident(n) if n == "Promise"))
            }
            _ => false,
        },
        // A bare `Reflect.construct` member READ detaches the cell
        // (`const c = Reflect.construct; c(C, ...)`) — the call
        // through the detached value is invisible, so the read
        // itself arms.
        crate::ast::Expr::Member { obj, name } => {
            name == "construct"
                && matches!(ast.get_expr(*obj), crate::ast::Expr::Ident(ns) if ns == "Reflect")
        }
        // RFC 20260808-construct-channel B5 — a `constructor`
        // property write is the ArraySpeciesCreate consumption
        // signal (§9.4.2.3 step 5 reads it back and step 10
        // constructs through the value; an `extends Array` class
        // needs no explicit `@@species` write — the inherited
        // default getter answers the class itself). Without the
        // adapters the species construct dead-ends on a loud
        // entry-miss for a class the program clearly wired up.
        crate::ast::Expr::Assign { target, .. } => matches!(
            ast.get_expr(*target),
            crate::ast::Expr::Member { name, .. } if name == "constructor"
        ),
        _ => false,
    })
}

/// Every class whose ctor chain bottoms out at the Promise builtin —
/// the direct `extends Promise` classes plus their user-class
/// descendants (`class CP2 extends CP`). These are the classes the
/// runtime capability path (`NewPromiseCapability(C)`, rotation 451)
/// can construct through a first-class value, so their factories
/// always get a boxed adapter.
pub(crate) fn promise_heir_classes(ast: &Ast) -> std::collections::HashSet<String> {
    let mut out: std::collections::HashSet<String> = ast
        .builtin_class_parents
        .iter()
        .filter(|(_, p)| p.as_str() == "Promise")
        .map(|(c, _)| c.clone())
        .collect();
    loop {
        let mut grew = false;
        for (c, p) in &ast.class_parents {
            if let Some(parent) = p
                && !out.contains(c)
                && out.contains(parent)
            {
                out.insert(c.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    out
}

/// The class factories that get a boxed adapter whatever the program
/// spells — the consumers that construct through them are runtime
/// ones no syntactic probe sees. A Promise heir: the inherited settle
/// statics and `then`'s SpeciesConstructor run NewPromiseCapability(C)
/// through the construct channel. An `extends Array` / TypedArray
/// heir: the inherited `@@species` construct runs inside the family
/// methods. One predicate for both consumers — the adapter synthesis
/// and the implicit-generics pass, which must keep these factories
/// non-generic or the adapter has no bare `__new_<C>` to wrap.
pub(crate) fn factory_always_admitted(
    ast: &Ast,
    promise_heirs: &std::collections::HashSet<String>,
    cname: &str,
) -> bool {
    promise_heirs.contains(cname)
        // An INJECTED intrinsic (`Error` and the NativeError family,
        // the two disposable stacks) is admitted unconditionally,
        // because §10.2.1 step 2 does not refuse it: `Error("m")`
        // CONSTRUCTS (§20.5.1.1 step 1 treats a call and a `new`
        // alike), and `call_class_ctor` sends it straight down the
        // construct channel. So the call position is a consumer of
        // the factory, and it is one the syntactic probe below
        // cannot see — `program_constructs_from_value` enumerates
        // the shapes that reach [[Construct]] through a `new`, and
        // this one arrives through a plain call. `Error("m")` in a
        // program with no `new`-through-value anywhere answered
        // "this constructor cannot be reached through a runtime
        // value yet" about a constructor the program had named
        // directly.
        //
        // `intrinsic_classes` is the compile-time mirror of the
        // runtime's `class_is_intrinsic`, which is the very test
        // `call_class_ctor` makes — so the set admitted here is
        // exactly the set that can arrive, and no user class pays
        // for it.
        || ast.intrinsic_classes.contains(cname)
        || crate::ast::desugar_classes_builtin_heritage::exotic_root_parent(ast, cname).is_some_and(
            |r| r == "Array" || crate::ast::desugar_classes_builtin_heritage::is_typedarray_ctor(r),
        )
}

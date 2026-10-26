//! `xs.{push,unshift}(...vals)` variadic arm extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 235 — twenty-eighth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! V3-18 wedge — Array.push / Array.unshift accept a variable
//! number of args per JS spec §23.1.3.23 / §23.1.3.35. Each
//! arg is appended (or prepended) in order. Pre-fix tora's
//! strict 1-arg signature rejected the multi-arg form. Every arg
//! must be assignable to the element type (an `any[]` takes
//! anything), and the call answers the new length.
//!
//! Returns `Some(Ok(Number))` on Array-receiver match with all
//! args type-compatible; `Some(Err(_))` on type mismatch;
//! `None` otherwise (non-Array receiver, non-{push,unshift},
//! or arity = 1 which falls to the generic arg-unify path).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    else {
        return None;
    };
    if !matches!(m_name.as_str(), "push" | "unshift") {
        return None;
    }
    let src_ty = match crate::check::receiver_type(checker, ast, *src_id) {
        Ok(t) => t,
        Err(e) => return Some(Err(e)),
    };
    let Type::Array(elem) = src_ty else {
        return None;
    };
    let inner = (*elem).clone();
    if args.len() == 1 {
        // TS any-assignability at the 1-arg boundary: an Any arg into
        // a Number/String elem array admits (`out.push(a * 2)` with
        // a: any — bun accepts), paired with `coerce_push_value`'s
        // Any→scalar/Str arm on the lowering side. Every OTHER 1-arg
        // shape keeps the generic arg-unify path and its admit stack
        // (Nullable match / empty-[] contextual / container widen),
        // so only the Any-arg form is intercepted; the ret is the
        // member-table's Number (new length), the same answer the
        // variadic arm below gives.
        // An object-layout element takes it too when the lowering casts
        // it (`casts_any_into`); an inferred one is retyped `any[]`
        // first (`let_widen::push`).
        // A function element takes any value the lowering checks is a
        // function; a call through the element rides the boxed entry
        // (`face_slots::record_elem_store`).
        let fn_elem = crate::check::face_slots::is_fn_elem(&inner);
        if !matches!(inner, Type::Number | Type::String)
            && !fn_elem
            && !casts_any_into(checker, ast, &inner, *src_id)
        {
            return None;
        }
        let aty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if fn_elem {
            crate::check::face_slots::record_elem_store(checker, &inner, &aty);
        }
        if aty != Type::Any {
            return None;
        }
        return Some(Ok(Type::Number));
    }
    for (i, aid) in args.iter().enumerate() {
        let aty = match checker.type_of(ast, *aid) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let aty = crate::check_assignable::in_declared_order(checker, ast, &inner, *aid, aty);
        // an `any` value for an object-layout element is cast as the
        // one-argument append casts it, or refused as that one is
        if aty == Type::Any
            && matches!(inner, Type::Struct(_) | Type::ClassRef(_))
            && !casts_any_into(checker, ast, &inner, *src_id)
        {
            return Some(Err(format!("argument {i}: expected {inner:?}, got Any")));
        }
        crate::check::face_slots::record_elem_store(checker, &inner, &aty);
        let admitted = inner == Type::Any
            || aty == Type::Any
            || crate::check_assignable::is_assignable_to(&inner, &aty)
            || crate::iface_witness::class_admit(&inner, &aty) == Some(true);
        if !admitted {
            return Some(Err(format!(
                "Array.{m_name} arg {i}: expected element type {:?}, got {aty:?}",
                inner
            )));
        }
    }
    Some(Ok(Type::Number))
}

/// Whether an `any` value appended to `recv`, an array of the object
/// type `elem`, is cast into the element type the program wrote for it
/// (`ssa_lower_checked_cast::obj`): `elem` is a type the cast can test
/// (no interface with witness rows, no method), and `recv` is an array
/// whose element annotation the lowering traces
/// (`ast_throw_info::written_ann`). Any other append keeps the refusal.
fn casts_any_into(checker: &Checker, ast: &Ast, elem: &Type, recv: ExprId) -> bool {
    let testable = match elem {
        Type::ClassRef(_) => true,
        Type::Struct(fields) => {
            let names = || fields.iter().map(|(k, _)| k);
            !fields.iter().any(|(_, t)| matches!(t, Type::Function(..)))
                && !(ast.iface_slots.layouts())
                    .any(|(_, iface)| iface.len() == fields.len() && iface.into_iter().eq(names()))
        }
        _ => false,
    };
    let binding = |n: &str| match checker.lookup(n) {
        Some(info) => info.ann,
        None => ast.stmts.iter().find_map(|s| match s {
            crate::ast::Stmt::LetDecl { name, type_ann, .. } if name == n => type_ann.clone(),
            _ => None,
        }),
    };
    let class_of = |e| crate::ast_throw_info::class_of(&checker.expr_types, e);
    testable
        && crate::ast_throw_info::written_ann(ast, recv, &binding, &class_of)
            .and_then(|a| crate::ast_throw_info::elem_ann(ast, &a))
            .is_some()
}

#[cfg(test)]
mod tests {
    fn check_src(src: &str) -> Result<(), String> {
        let tokens = crate::lexer::tokenize(src).map_err(|e| format!("lex: {e}"))?;
        let mut ast = crate::parser::parse(src, &tokens).map_err(|e| format!("parse: {e}"))?;
        crate::ast::lift_arrow_fns(&mut ast);
        crate::check::check(&ast).map(|_| ())
    }

    // the checker alone sees no class (each `ClassDecl` is split before
    // it runs), so the element type here is a declared type of fields
    const P: &str = "type P = { x: number };\nconst a: any = 1;\n";

    fn refused(body: &str) {
        let err = check_src(&format!("{P}{body}")).expect_err("program must be refused");
        assert!(err.contains("got Any"), "{err}");
    }

    fn accepted(body: &str) {
        let r = check_src(&format!("{P}{body}"));
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn an_annotated_array_reached_by_name_or_field_takes_any() {
        accepted("const xs: P[] = [];\nxs.push(a);\nxs.unshift(a);\nxs.push(a, a);\n");
        accepted(
            "function f(): number { const out: P[] = [];\n\
             [1].forEach((i: number) => { out.push(a); }); return out.length; }\n",
        );
        accepted(
            "type B = { items: P[] };\ntype H = { b: B };\n\
             const h: H = { b: { items: [] } };\nh.b.items.push(a);\n",
        );
        accepted("type T = { x: number };\nconst ts: Array<T> = [];\nts.push(a);\n");
    }

    #[test]
    fn an_element_type_the_cast_cannot_test_keeps_the_refusal() {
        refused("interface S { area(): number }\nconst xs: S[] = [];\nxs.push(a);\n");
        refused("interface S { area(): number }\nconst xs: S[] = [];\nxs.push(a, a);\n");
        refused("type T = { n: number; f(): number };\nconst xs: T[] = [];\nxs.unshift(a);\n");
    }

    #[test]
    fn an_array_without_a_traced_annotation_keeps_the_refusal() {
        refused("const store: P[] = [];\nfunction get(): P[] { return store; }\nget().push(a);\n");
        refused("const grid: P[][] = [[]];\nfor (const row of grid) row.push(a);\n");
        refused("const o = { items: [{ x: 1 }] };\no.items.push(a);\n");
        refused("const grid: P[][] = [[]];\ngrid[0].push(a);\n");
    }
}

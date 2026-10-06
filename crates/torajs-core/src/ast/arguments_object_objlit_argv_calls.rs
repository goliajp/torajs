//! The call-argument arms of [`super::arguments_object_objlit_argv`]:
//! a fn-expr whose only reference is an argument position of a call
//! the runtime answers through the closure cell's boxed adapter.
//!
//! The descriptor arm — `Object.defineProperty(o, "p", { set:
//! function (a) { …arguments… } })` hands the function to §10.1.6.3 as
//! a descriptor face and nothing else ever names it. An accessor face
//! is entered only by the [[Get]] / [[Set]] machinery, and a `value`
//! face only through a member read of the defined property, which is
//! dynamic and so an any-lane call; both enter through the boxed
//! adapter that carries the real argc/argv. Without this arm the body
//! sat in no face at all and `arguments` stayed a bare name ("unknown
//! identifier `arguments`", then a ReferenceError at run time), while
//! the same function bound to a `const` first, or the descriptor bound
//! to a variable first, was admitted. Shapes: `Object.defineProperty` /
//! `Reflect.defineProperty` (descriptor is argument 2) and
//! `Object.defineProperties` (each field of argument 1); faces are the
//! `get` / `set` / `value` fields of an inline object literal. A `value`
//! face also needs a literal key whose member spelling nowhere in the
//! program could be a typed-dispatch site (the field arm's by-name
//! refusal).

use std::collections::{HashMap, HashSet};

use super::{Ast, Expr, ExprId};

pub(super) fn admit_call_arg_faces(
    ast: &Ast,
    excluded: &HashSet<String>,
    ident_refs: &HashMap<&str, usize>,
    closure_refs: &HashMap<&str, usize>,
    member_names: &HashSet<&str>,
    value_fns: &mut HashSet<String>,
    method_fns: &mut HashSet<String>,
) {
    let mut faces: Vec<&str> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        if let Expr::Ident(ns) = ast.get_expr(*obj) {
            descriptor_call_faces(ast, ns, name, args, member_names, &mut faces);
        }
        // Rotation 372 apply arm — an INLINE fn-expr as the `.apply`
        // callee whose LITERAL argArray carries a `...spread`
        // (`(function () { …arguments… }).apply(null, [1, ...src])`,
        // the t262 expressions/array family): the fn-value apply route
        // hands exactly this shape to the spread lane's bare kernel,
        // which invokes the closure cell's boxed adapter — real
        // argc/argv, the face's own channel. Inline means the Closure
        // expr is the fn's only reference (the shared gate re-checks).
        if name != "apply" || args.len() != 2 {
            continue;
        }
        let Expr::Closure { fn_name, .. } = ast.get_expr(*obj) else {
            continue;
        };
        let spread_arg_array = matches!(ast.get_expr(args[1]), Expr::Array(els)
            if els.iter().any(|el| matches!(ast.get_expr(*el), Expr::Spread { .. })));
        if spread_arg_array {
            faces.push(fn_name);
        }
    }
    for fn_name in faces {
        super::arguments_object_objlit_argv::admit_boxed_only(
            ast,
            fn_name,
            excluded,
            ident_refs,
            closure_refs,
            value_fns,
            method_fns,
        );
    }
}

/// The descriptor faces of one `<ns>.<name>(args)` call (module doc).
fn descriptor_call_faces<'a>(
    ast: &'a Ast,
    ns: &str,
    name: &str,
    args: &[ExprId],
    member_names: &HashSet<&str>,
    out: &mut Vec<&'a str>,
) {
    match (ns, name) {
        ("Object" | "Reflect", "defineProperty") => {
            let (Some(&key), Some(&desc)) = (args.get(1), args.get(2)) else {
                return;
            };
            let key = match ast.get_expr(key) {
                Expr::String(s) => s.as_str(),
                _ => None,
            };
            literal_descriptor_faces(ast, desc, key, member_names, out);
        }
        ("Object", "defineProperties") => {
            let Some(&props) = args.get(1) else { return };
            let Expr::ObjectLit { fields } = ast.get_expr(props) else {
                return;
            };
            for (key, desc) in fields {
                literal_descriptor_faces(ast, *desc, key.as_str(), member_names, out);
            }
        }
        _ => {}
    }
}

fn literal_descriptor_faces<'a>(
    ast: &'a Ast,
    desc: ExprId,
    key: Option<&str>,
    member_names: &HashSet<&str>,
    out: &mut Vec<&'a str>,
) {
    let Expr::ObjectLit { fields } = ast.get_expr(desc) else {
        return;
    };
    for (fname, feid) in fields {
        let face = match fname.as_str() {
            Some("get" | "set") => true,
            Some("value") => key.is_some_and(|k| !member_names.contains(k)),
            _ => false,
        };
        if let (true, Expr::Closure { fn_name, .. }) = (face, ast.get_expr(*feid)) {
            out.push(fn_name);
        }
    }
}

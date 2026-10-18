//! The class on its own: its declaration, and the factory and
//! constructor the class desugaring wrote for it.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};

use super::Candidate;

/// Classes whose declaration and desugared factory / constructor pass
/// the checks of the census's first paragraph.
pub(super) fn candidates(ast: &Ast) -> Vec<Candidate<'_>> {
    let mut decls: Vec<(&str, &[(PropKey, String)])> = Vec::new();
    for s in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
            && type_params.is_empty()
            && matches!(ast.class_parents.get(name), Some(None))
        {
            decls.push((name, fields));
        }
    }
    if decls.is_empty() {
        return Vec::new();
    }
    let mut fns: HashMap<&str, Vec<&Stmt>> = HashMap::new();
    collect_fns(&ast.stmts, &mut fns);
    let mut out = Vec::new();
    for (name, fields) in decls {
        let Some(fields) = plain_fields(ast, name, fields) else {
            continue;
        };
        if ast
            .class_parents
            .values()
            .any(|p| p.as_deref() == Some(name))
            || ast.ctor_return_override.contains(name)
            || ast.exotic_parent.contains_key(name)
            || ast.eval_unit_classes.contains(name)
            || ast.private_elements.contains_key(name)
            || ast.accessor_getters.keys().any(|(o, _)| o == name)
            || ast.accessor_setters.keys().any(|(o, _)| o == name)
            || has_members(&fns, name)
        {
            continue;
        }
        let ctor_name = format!("__cm_{name}__ctor");
        let ctor = match fns.get(ctor_name.as_str()).map(Vec::as_slice) {
            None => None,
            Some([one]) => Some(*one),
            Some(_) => continue,
        };
        let factory = match fns.get(format!("__new_{name}").as_str()).map(Vec::as_slice) {
            Some([one]) => *one,
            _ => continue,
        };
        let arity = match ctor {
            Some(c) => match ctor_shape(ast, c, &fields) {
                Some(n) => n,
                None => continue,
            },
            None => 0,
        };
        if !factory_shape(
            ast,
            factory,
            name,
            &fields,
            &ctor_name,
            ctor.is_some(),
            arity,
        ) {
            continue;
        }
        out.push(Candidate {
            name,
            fields,
            arity,
        });
    }
    out
}

/// The declared fields, when every one is a non-optional `number` or
/// `boolean` no spelling of the program can turn into anything but a
/// plain data slot.
fn plain_fields<'a>(
    ast: &Ast,
    owner: &str,
    fields: &'a [(PropKey, String)],
) -> Option<Vec<&'a str>> {
    let names = &ast.accessor_field_names;
    let mut out = Vec::with_capacity(fields.len());
    for (k, ann) in fields {
        let std::borrow::Cow::Borrowed(f) = k.lossy() else {
            return None;
        };
        if !matches!(ann.as_str(), "number" | "boolean")
            || ast.optional_members.declared(owner, k)
            || names.may(f)
            || names.may_shadow(f, false)
        {
            return None;
        }
        out.push(f);
    }
    (!out.is_empty()).then_some(out)
}

/// A method, method twin or a second constructor of `class`.
fn has_members(fns: &HashMap<&str, Vec<&Stmt>>, class: &str) -> bool {
    let cm = format!("__cm_{class}__");
    let cmany = format!("__cmany_{class}__");
    fns.keys()
        .any(|n| (n.starts_with(&cm) && n[cm.len()..] != *"ctor") || n.starts_with(&cmany))
}

fn collect_fns<'a>(stmts: &'a [Stmt], out: &mut HashMap<&'a str, Vec<&'a Stmt>>) {
    for s in stmts {
        if let Stmt::FnDecl { name, body, .. } = s {
            out.entry(name.as_str()).or_default().push(s);
            collect_fns(body, out);
        } else {
            super::super::live_field_census::walk::nested_stmts(s, &mut |st| {
                collect_fns(std::slice::from_ref(st), out)
            });
        }
    }
}

/// `__cm_C__ctor(__this, __new_target, p..)` whose body (after the
/// directive) is `__this.f = <param or literal>` once per distinct
/// field and nothing else: the user arity.
fn ctor_shape(ast: &Ast, ctor: &Stmt, fields: &[&str]) -> Option<usize> {
    let Stmt::FnDecl {
        params,
        body,
        type_params,
        is_generator: false,
        ..
    } = ctor
    else {
        return None;
    };
    let [this, nt, user @ ..] = params.as_slice() else {
        return None;
    };
    if !type_params.is_empty()
        || this.name != "__this"
        || nt.name != "__new_target"
        || params.iter().any(|p| p.default.is_some() || p.is_rest)
    {
        return None;
    }
    let mut written: HashSet<&str> = HashSet::new();
    for s in body {
        let Stmt::Expr(e) = s else { return None };
        let (target, value) = match ast.get_expr(*e) {
            Expr::String(_) => continue,
            Expr::Assign { target, value } => (*target, *value),
            _ => return None,
        };
        let Expr::Member { obj, name } = ast.get_expr(target) else {
            return None;
        };
        let value_ok = match ast.get_expr(value) {
            Expr::Ident(n) => user.iter().any(|p| p.name == *n),
            e => literal(e),
        };
        if !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "__this")
            || !fields.contains(&name.as_str())
            || !written.insert(name)
            || !value_ok
        {
            return None;
        }
    }
    Some(user.len())
}

/// `__new_C(p..)` = `let __this: C = { <every field: literal> };`
/// `__cm_C__ctor(__this, __torajs_my_class_ref("C"), p..)` (when there
/// is a constructor) `; return __this`.
fn factory_shape(
    ast: &Ast,
    factory: &Stmt,
    class: &str,
    fields: &[&str],
    ctor_name: &str,
    has_ctor: bool,
    arity: usize,
) -> bool {
    let Stmt::FnDecl { params, body, .. } = factory else {
        return false;
    };
    if params.len() != arity || params.iter().any(|p| p.default.is_some() || p.is_rest) {
        return false;
    }
    let (seed, rest) = match body.as_slice() {
        [Stmt::LetDecl { name, init, .. }, rest @ ..] if name == "__this" => (*init, rest),
        _ => return false,
    };
    let Expr::ObjectLit { fields: lit } = ast.get_expr(seed) else {
        return false;
    };
    if lit.len() != fields.len()
        || !lit
            .iter()
            .zip(fields)
            .all(|((k, v), f)| k.lossy() == *f && literal(ast.get_expr(*v)))
    {
        return false;
    }
    let is_this = |e: &ExprId| matches!(ast.get_expr(*e), Expr::Ident(n) if n == "__this");
    let ret_ok = |s: &Stmt| matches!(s, Stmt::Return(Some(e)) if is_this(e));
    match (has_ctor, rest) {
        (false, [ret]) => ret_ok(ret),
        (true, [Stmt::Expr(call), ret]) => {
            let Expr::Call { callee, args } = ast.get_expr(*call) else {
                return false;
            };
            let [this, nt, user @ ..] = args.as_slice() else {
                return false;
            };
            let class_ref = matches!(ast.get_expr(*nt), Expr::Call { callee, args }
                if matches!(ast.get_expr(*callee), Expr::Ident(n) if n == "__torajs_my_class_ref")
                    && matches!(args.as_slice(), [a] if matches!(ast.get_expr(*a), Expr::String(s) if *s == *class)));
            matches!(ast.get_expr(*callee), Expr::Ident(n) if n == ctor_name)
                && is_this(this)
                && class_ref
                && user.len() == arity
                && user
                    .iter()
                    .zip(params)
                    .all(|(a, p)| matches!(ast.get_expr(*a), Expr::Ident(n) if *n == p.name))
                && ret_ok(ret)
        }
        _ => false,
    }
}

/// A literal with no effect and no identity.
fn literal(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Number(_) | Expr::Bool(_) | Expr::Null | Expr::Uninit
    ) || matches!(e, Expr::Ident(n) if n == "undefined")
}

//! The check rounds that retype bindings `any`.
//!
//! A binding that may store a value of another kind
//! (`num_width::hidden_any`: an element of a typed array whose alias
//! escaped into the `any` world, or of an `any[]`) is annotated `any`,
//! a call argument that may carry one is read as `any` by the call
//! check (`check_type_of_call::any_widen`), an object literal field
//! that may be stored one is initialized through `as any` (the field's
//! slot is laid out from the value it is initialized with), a function
//! that may return one returns `any`, and the program checked again,
//! until no round asks for another. A program that asks for none is
//! checked once.

use std::collections::{HashMap, HashSet};

use super::{Round, check_round};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Type;

/// Run rounds from `first`, the round of `ast` as written.
pub(super) fn settle(ast: &Ast, first: Round) -> Result<Round, String> {
    let mut round = first;
    let mut inits: HashSet<ExprId> = HashSet::new();
    let mut args: HashSet<ExprId> = HashSet::new();
    let mut rets: HashSet<String> = HashSet::new();
    let mut fields: HashSet<ExprId> = HashSet::new();
    let mut retypable: Option<HashSet<String>> = None;
    let mut retyped: Option<Ast> = None;
    loop {
        let hidden = &round.0.5.widths.hidden_any;
        let before = rets.len();
        if !hidden.rets.is_empty() {
            let ok = retypable.get_or_insert_with(|| retypable_returns(ast));
            rets.extend((hidden.rets.iter()).filter(|f| ok.contains(*f)).cloned());
        }
        let grew = (rets.len() > before)
            | extend_own(ast, &mut inits, &hidden.inits)
            | extend_own(ast, &mut fields, &scalar_values(&round.0.1, &hidden.fields))
            | extend_own(ast, &mut fields, &objects_given_any(ast, &round.0.1));
        if !(extend_own(ast, &mut args, &hidden.args) || grew) {
            return Ok(round);
        }
        let r = retyped.get_or_insert_with(|| ast.clone());
        retype_any(&mut r.stmts, &inits);
        retype_returns_any(&mut r.stmts, &rets);
        spell_fields_any(r, &fields);
        round = check_round(r, &args)?;
    }
}

/// Add the ids of `found` that belong to `ast` to `asked`; true when
/// one was new. Ids past `ast.exprs` belong to specializations the mono
/// pass clones, and name nothing of `ast`.
fn extend_own(ast: &Ast, asked: &mut HashSet<ExprId>, found: &HashSet<ExprId>) -> bool {
    let before = asked.len();
    asked.extend((found.iter().copied()).filter(|e| (e.0 as usize) < ast.exprs.len()));
    asked.len() > before
}

/// The values among `found` the checker types `number`, `string` or
/// `boolean`; a field initialized with anything else has a slot that
/// holds a value of any kind already, or a value whose name a field
/// initializer gives it (§13.2.5.5) that an `as` would hide.
fn scalar_values(types: &HashMap<ExprId, Type>, found: &HashSet<ExprId>) -> HashSet<ExprId> {
    (found.iter().copied())
        .filter(|e| {
            matches!(
                types.get(e),
                Some(Type::Number | Type::String | Type::Boolean)
            )
        })
        .collect()
}

/// The object values of the object literal fields an `any` value is
/// stored into later (`o.p = v`, `o` a binding the literal initializes
/// without an annotation). The field's slot is the compiler's inference
/// from its first value, and an inferred slot never rejects a value at
/// run time (only an annotation the program wrote does): it holds the value of any kind. Bindings
/// resolve by name, so a same-named binding elsewhere can widen a field
/// too, which costs the typed lane, never an answer.
fn objects_given_any(ast: &Ast, types: &HashMap<ExprId, Type>) -> HashSet<ExprId> {
    let mut lits: HashMap<String, ExprId> = HashMap::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl {
            name,
            type_ann: None,
            init,
            ..
        } = s
            && matches!(ast.get_expr(*init), Expr::ObjectLit { .. })
        {
            lits.insert(name.clone(), *init);
        }
    });
    let mut out = HashSet::new();
    for e in &ast.exprs {
        let Expr::Assign { target, value } = e else {
            continue;
        };
        let (Expr::Member { obj, name: f }, Some(Type::Any)) =
            (ast.get_expr(*target), types.get(value))
        else {
            continue;
        };
        let lit = match ast.get_expr(*obj) {
            Expr::Ident(n) => lits.get(n).map(|l| ast.get_expr(*l)),
            _ => None,
        };
        let Some(Expr::ObjectLit { fields }) = lit else {
            continue;
        };
        out.extend(
            (fields.iter())
                .filter(|(k, v)| {
                    k == f && matches!(types.get(v), Some(Type::Struct(_) | Type::ClassRef(_)))
                })
                .map(|(_, v)| *v),
        );
    }
    out
}

/// Spell `<value> as any` for each object literal field value in
/// `vals` that is still the field's value. The field may be stored a
/// value of another kind later (§10.1.9 stores it as is), and its slot
/// takes the kind of the value it is initialized with.
fn spell_fields_any(r: &mut Ast, vals: &HashSet<ExprId>) {
    for i in 0..r.exprs.len() {
        let Expr::ObjectLit { fields } = &r.exprs[i] else {
            continue;
        };
        let hits: Vec<(usize, ExprId)> = (fields.iter().enumerate())
            .filter(|(_, (_, v))| vals.contains(v))
            .map(|(j, (_, v))| (j, *v))
            .collect();
        for (j, v) in hits {
            let span = r.expr_spans[v.0 as usize];
            let w = r.add_expr(Expr::As {
                expr: v,
                ty_ann: "any".to_string(),
            });
            r.set_expr_span(w, span);
            if let Expr::ObjectLit { fields } = &mut r.exprs[i] {
                fields[j].1 = w;
            }
        }
    }
}

/// Annotate `any` every `let` / `const` whose initializer is in
/// `inits`. Such a binding can be handed a value of any kind at run
/// time, and §13.15.2 stores it as is; the annotation gives it the
/// boxed slot every `any` binding has, in the checker and the lowering
/// alike.
fn retype_any(stmts: &mut [Stmt], inits: &HashSet<ExprId>) {
    let mut stack: Vec<&mut Stmt> = stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::LetDecl { init, type_ann, .. } = s
            && inits.contains(init)
        {
            *type_ann = Some("any".to_string());
        }
        crate::ast_closure_param_tag_retag::push_child_stmts_mut(s, &mut stack);
    }
}

/// The functions whose return a round may retype `any`: a declaration
/// or a lifted closure declared to return a `number`, `string` or
/// `boolean`, or with no annotation, and only ever called by name or
/// handed to a fn-typed slot ([`called_by_name_only`]). A generator or
/// an async function answers an object of its own and keeps its face.
fn retypable_returns(ast: &Ast) -> HashSet<String> {
    let mut out = HashSet::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::FnDecl {
            name,
            return_type,
            is_generator: false,
            ..
        } = s
            && (!name.starts_with("__") || name.starts_with("__closure_"))
            && !ast.async_fns.contains(name)
            && matches!(
                return_type.as_deref(),
                None | Some("number" | "string" | "boolean")
            )
        {
            out.insert(name.clone());
        }
    });
    let by_name = called_by_name_only(ast);
    out.retain(|f| by_name.contains(f));
    out
}

/// The functions only ever called by name, or handed to a fn-typed
/// slot whose calls follow the face of what it holds: the initializer
/// of a binding annotated with a function type (`const h: (x: number)
/// => number = q`, the let lane's face census) or the argument of a
/// declared function's fn-typed parameter (`run(q)`, the parameter
/// census), whose calls then answer what `q` returns. A function that
/// flows on as a value anywhere else may land in a slot typed with its
/// declared return (an array, a field, a builtin's callback), whose
/// callers read that return; retyping it would take the function out
/// of the slot. A closure that initializes a binding with no annotation
/// is called by name when every read of that binding is a call.
fn called_by_name_only(ast: &Ast) -> HashSet<String> {
    let mut callees: HashSet<ExprId> = fn_slot_values(ast);
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e {
            callees.insert(*callee);
        }
    }
    // closure → the bindings it initializes
    let mut bound: HashMap<ExprId, String> = HashMap::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl {
            name,
            type_ann: None,
            init,
            ..
        } = s
            && let Expr::Closure { .. } = ast.get_expr(*init)
        {
            bound.insert(*init, name.clone());
        }
    });
    let (mut called, mut values) = (HashSet::new(), HashSet::new());
    let mut names: HashMap<&str, Vec<&str>> = HashMap::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let eid = ExprId(i as u32);
        match e {
            Expr::Ident(n) if callees.contains(&eid) => {
                called.insert(n.as_str());
            }
            Expr::Ident(n) => {
                values.insert(n.as_str());
            }
            Expr::Closure { fn_name, .. } => match bound.get(&eid) {
                Some(b) => names.entry(fn_name).or_default().push(b),
                None => {
                    values.insert(fn_name.as_str());
                }
            },
            _ => {}
        }
    }
    let mut out = HashSet::new();
    for f in called.iter().chain(names.keys()) {
        let mut all = names.get(f).cloned().unwrap_or_default();
        all.push(*f);
        if all.iter().all(|n| !values.contains(n)) && all.iter().any(|n| called.contains(n)) {
            out.insert(f.to_string());
        }
    }
    out
}

/// The names read as the value of a fn-typed slot of the face census:
/// a binding's initializer under a function-type annotation, or the
/// argument of a declared function's fn-typed parameter.
fn fn_slot_values(ast: &Ast) -> HashSet<ExprId> {
    let fn_ann = |a: &Option<String>| a.as_deref().is_some_and(crate::ast::is_fn_like_ann);
    let mut params: HashMap<String, Vec<bool>> = HashMap::new();
    let mut out = HashSet::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::FnDecl {
            name, params: ps, ..
        } => {
            let user = ps.iter().filter(|p| !p.name.starts_with("__"));
            params.insert(name.clone(), user.map(|p| fn_ann(&p.type_ann)).collect());
        }
        Stmt::LetDecl { type_ann, init, .. }
            if fn_ann(type_ann) && matches!(ast.get_expr(*init), Expr::Ident(_)) =>
        {
            out.insert(*init);
        }
        _ => {}
    });
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Some(fns) = (match ast.get_expr(*callee) {
            Expr::Ident(g) => params.get(g),
            _ => None,
        }) else {
            continue;
        };
        out.extend(
            (args.iter().zip(fns))
                .filter(|(a, f)| **f && matches!(ast.get_expr(**a), Expr::Ident(_)))
                .map(|(a, _)| *a),
        );
    }
    out
}

/// Annotate `any` the return of every function in `fns`. It may hand
/// back a value of any kind, which §10.2.1.12 returns as is.
fn retype_returns_any(stmts: &mut [Stmt], fns: &HashSet<String>) {
    let mut stack: Vec<&mut Stmt> = stmts.iter_mut().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name, return_type, ..
        } = s
            && fns.contains(name)
        {
            *return_type = Some("any".to_string());
        }
        crate::ast_closure_param_tag_retag::push_child_stmts_mut(s, &mut stack);
    }
}

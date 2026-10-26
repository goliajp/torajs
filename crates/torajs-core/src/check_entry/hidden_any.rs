//! The check rounds that retype bindings `any`.
//!
//! A binding that may store a value of another kind
//! (`num_width::hidden_any`: an element of a typed array whose alias
//! escaped into the `any` world, or of an `any[]`) is annotated `any`,
//! a call argument that may carry one is read as `any` by the call
//! check (`check_type_of_call::any_widen`), an object literal field
//! that may be stored one is initialized through `as any` (the field's
//! slot is laid out from the value it is initialized with), a function
//! that may return one returns `any` (a method together with every
//! declaration that shares its return slot), and the program checked
//! again, until no round asks for another. A program that asks for none
//! is checked once.

use std::collections::{HashMap, HashSet};

use super::{Round, check_round};
use crate::ast::named_prop_census::shadow_names::ShadowedNames;
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::Type;
use fn_slots::{Guarded, boxed};
use retypable::{retypable_returns, roots};

/// Run rounds from `first`, the round of `ast` as written. A function
/// retyped while a slot it reaches holds it only through the census
/// ([`Guarded`]) needs the census to box the slot: a closure's
/// parameter from the first round on (the closure's own return is
/// inferred from the calls through it), a binding by the last round.
/// When the last round's census misses one, the rounds run again from
/// `first` with the slot left out.
pub(super) fn settle(ast: &Ast, first: Round) -> Result<Round, String> {
    let early = &first.0.5.mono_ast.argv_boxed_params;
    let mut denied: HashSet<Guarded> = HashSet::new();
    loop {
        let allow = |g: &Guarded| {
            !denied.contains(g)
                && match g {
                    Guarded::Param(f, p) => early.get(f).is_some_and(|ps| ps.contains(p)),
                    Guarded::Let(_) => true,
                }
        };
        let (round, needs) = settle_with(ast, &first, &allow)?;
        let mono = &round.as_ref().unwrap_or(&first).0.5.mono_ast;
        let unboxed: Vec<Guarded> = (needs.into_iter())
            .filter(|g| !boxed(ast, mono, g))
            .collect();
        if unboxed.is_empty() {
            return Ok(round.unwrap_or(first));
        }
        denied.extend(unboxed);
    }
}

/// [`settle`] with the slots `allow` refuses left out: the last round,
/// `None` when it is `first`, and the census entries the functions it
/// retyped need.
fn settle_with(
    ast: &Ast,
    first: &Round,
    allow: &dyn Fn(&Guarded) -> bool,
) -> Result<(Option<Round>, Vec<Guarded>), String> {
    let mut round: Option<Round> = None;
    let (mut inits, mut args): (HashSet<ExprId>, HashSet<ExprId>) = Default::default();
    let mut rets: HashSet<String> = HashSet::new();
    let mut fields: HashSet<ExprId> = HashSet::new();
    let mut promise_rets: HashSet<String> = HashSet::new();
    let mut executors: HashSet<ExprId> = HashSet::new();
    let mut retypable = None;
    let mut retyped: Option<Ast> = None;
    let mut objects = object_asks::ObjectAsks::of(ast);
    loop {
        let cur = round.as_ref().unwrap_or(first);
        let hidden = &cur.0.5.widths.hidden_any;
        let before = rets.len();
        let (found, objects_grew) = objects.extend(ast, &cur.0.1, &hidden.rets, &mut fields);
        if !found.is_empty() {
            let (ok, _) = retypable.get_or_insert_with(|| retypable_returns(ast, allow));
            rets.extend(found.into_iter().filter(|f| ok.contains(f)));
        }
        let grew = (rets.len() > before)
            | objects_grew
            | promise_rets::extend(ast, &cur.0.1, &mut promise_rets)
            | executor_any::extend(ast, &cur.0.1, &mut executors)
            | extend_own(ast, &mut inits, &hidden.inits)
            | extend_own(ast, &mut fields, &scalar_values(&cur.0.1, &hidden.fields))
            | extend_own(ast, &mut fields, &objects_given_any(ast, &cur.0.1));
        if !(extend_own(ast, &mut args, &hidden.args) || grew) {
            let needs = match &retypable {
                Some((_, flow)) if !rets.is_empty() => {
                    flow.guarded_from(roots(ast, &rets).iter().map(String::as_str))
                }
                _ => Vec::new(),
            };
            return Ok((round, needs));
        }
        let r = retyped.get_or_insert_with(|| ast.clone());
        retype_any(&mut r.stmts, &inits);
        retype_returns_any(&mut r.stmts, &rets);
        objects.retype(ast, r, &cur.0.1);
        promise_rets::retype(&mut r.stmts, &promise_rets);
        executor_any::retype(r, &executors);
        spell_fields_any(r, &fields);
        reinfer::reinfer_returns(ast, r);
        round = Some(check_round(r, &args, &ShadowedNames::default(), false)?);
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
    let mut lits: HashMap<String, Vec<ExprId>> = HashMap::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl {
            name,
            type_ann: None,
            init,
            ..
        } = s
            && matches!(ast.get_expr(*init), Expr::ObjectLit { .. })
        {
            lits.entry(name.clone()).or_default().push(*init);
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
        let Expr::Ident(n) = ast.get_expr(*obj) else {
            continue;
        };
        for l in lits.get(n).into_iter().flatten() {
            let Expr::ObjectLit { fields } = ast.get_expr(*l) else {
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

mod executor_any;
mod fn_slots;
mod iface_types;
mod inlobj_fields;
mod method_lits;
mod method_rets;
mod obj_fields;
mod obj_rets;
mod object_asks;
mod promise_rets;
mod reinfer;
mod retypable;
mod samename;
mod type_parts;
mod value_places;
mod value_reads;

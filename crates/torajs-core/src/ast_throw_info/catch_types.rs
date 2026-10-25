//! The type of an unannotated catch parameter, from what its `try`
//! body can throw.
//!
//! `catch (e)` binds `any` (TS gives the parameter no other type), and
//! every read of `e` then goes through the dynamic runtime: `e as
//! number` is a ToNumber, `(e as Fail).code` an any-lane member read,
//! and a program whose only dynamic value is the caught one links the
//! whole any core for it. The program-wide throw analysis already knows
//! which functions a `try` body reaches; this asks what they throw.
//! When every raise the body can reach is a `throw` of one type `T`,
//! and every read of `e` in the catch body is `e as T`, the parameter
//! binds `T`:
//!
//! - the checker's view does not change: it typed `e` as `any` and
//!   each `e as T` as `T`, which is what the reads are either way, so
//!   no diagnostic can appear or disappear;
//! - a raise the scan cannot type — a runtime check (`Scan::direct`),
//!   a call it cannot follow, a `throw` of an operand whose type is not
//!   `T`, a suspension — leaves the parameter `any`;
//! - the lowering takes the answer only when the calls it emitted for
//!   the body and its callees confirm it (`ssa_lower::proofs::catch`):
//!   the scan mirrors the lowering's raise sites by hand and does not
//!   know all of them;
//! - the typed binding then reads the thrown value without a tag test
//!   (`ssa_lower_stmt_try::bind`); only a checking build tests the tag,
//!   to catch a hole in the confirmation.
//!
//! The answer is keyed by the first `e as T` read of the catch body
//! ([`cast_reads`]), which the lowering finds again from the statement
//! it lowers.

use std::collections::HashMap;

use super::Scan;
use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, NameRead, Param, Stmt};
use crate::ast_throw_info_fixpoint::{ThrowFacts, expand_called};
use crate::check::Type;
use crate::ssa_lower::proofs::catch::CatchFacts;

/// What the raises of a body can hold.
#[derive(Clone, PartialEq)]
pub(crate) enum Thrown {
    Nothing,
    One(Type),
    /// More than one type, or a raise of no known type.
    Many,
}

impl Thrown {
    fn of(ty: Option<&Type>) -> Thrown {
        match ty {
            Some(t) if *t != Type::Any => Thrown::One(t.clone()),
            _ => Thrown::Many,
        }
    }

    fn join(&mut self, other: &Thrown) {
        *self = match (&*self, other) {
            (_, Thrown::Nothing) => return,
            (Thrown::Nothing, o) => o.clone(),
            (Thrown::One(a), Thrown::One(b)) if a == b => return,
            _ => Thrown::Many,
        };
    }
}

/// A catch whose parameter may take a type: the `try` body, the
/// function it is in, and the parameter's `e as T` reads.
struct Candidate<'a> {
    fn_name: &'a str,
    params: &'a [Param],
    body: &'a [Stmt],
    cast: ExprId,
    ann: String,
}

/// The inferred catch parameter types, by the first `e as T` read of
/// each catch body ([`cast_reads`]), with each FnDecl's own `throw`s
/// for the lowering's confirmation (`ssa_lower::proofs::catch`).
pub(crate) fn infer(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    widths: &crate::num_width::WidthTable,
    live: &crate::ssa_lower_pass_0_5::live_field_census::Live,
    facts: &ThrowFacts,
) -> CatchFacts {
    let mut out = CatchFacts::default();
    let found = candidates(ast);
    if found.is_empty() {
        return out;
    }
    let fns = summarize(expr_types, facts);
    for c in found {
        let Some(want) = expr_types.get(&c.cast) else {
            continue;
        };
        let names = (&facts.assigned, live, facts.has_instance_hook);
        let mut sc = Scan::new(ast, c.fn_name, c.params, expr_types, widths, names);
        sc.stmts(c.body);
        if sc.direct
            || (sc.nonint_read && facts.props_run_code)
            || sc.called.iter().any(|n| facts.fn_lets.contains(n))
        {
            continue;
        }
        let mut called = sc.called;
        expand_called(&facts.aliases, &mut called);
        let mut thrown = Thrown::Nothing;
        for x in &sc.thrown {
            thrown.join(&Thrown::of(expr_types.get(x)));
        }
        for n in &called {
            if let Some(t) = fns.get(n.as_str()) {
                thrown.join(t);
            }
        }
        if thrown == Thrown::One(want.clone()) {
            out.inferred.insert(c.cast, (c.ann, want.clone()));
        }
    }
    if !out.inferred.is_empty() {
        out.own_thrown = (facts.decls.iter())
            .map(|d| {
                let mut t = Thrown::Nothing;
                for x in &d.thrown {
                    t.join(&Thrown::of(expr_types.get(x)));
                }
                (d.name.clone(), t)
            })
            .collect();
    }
    out
}

/// What each FnDecl can throw, closed over its calls. A call to a name
/// that is no FnDecl adds nothing, the same reading the may-throw set
/// gives it.
fn summarize<'a>(
    expr_types: &HashMap<ExprId, Type>,
    facts: &'a ThrowFacts,
) -> HashMap<&'a str, Thrown> {
    let mut out: HashMap<&str, Thrown> = (facts.decls.iter())
        .map(|d| {
            let mut t = if d.raises {
                Thrown::Many
            } else {
                Thrown::Nothing
            };
            for x in &d.thrown {
                t.join(&Thrown::of(expr_types.get(x)));
            }
            (d.name.as_str(), t)
        })
        .collect();
    loop {
        let mut grew = false;
        for d in &facts.decls {
            let mut t = out[d.name.as_str()].clone();
            for c in &d.called {
                if let Some(ct) = out.get(c.as_str()) {
                    t.join(ct);
                }
            }
            if t != out[d.name.as_str()] {
                out.insert(d.name.as_str(), t);
                grew = true;
            }
        }
        if !grew {
            return out;
        }
    }
}

/// Every unannotated catch whose parameter is read only as `e as T`,
/// `T` a number, a boolean or a declared class, in a function body or
/// at top level. A function that suspends is left out (a resumed
/// `await` or `yield` throws what the caller hands in), and so is a
/// `try` nested in a function declared inside another one, which the
/// scan would read with the outer function's parameters.
fn candidates(ast: &Ast) -> Vec<Candidate<'_>> {
    let mut out = Vec::new();
    for top in &ast.stmts {
        match top {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                if !ast.async_ssa.fns.contains(name) {
                    collect(ast, name, params, body, &mut out);
                }
            }
            s => collect(ast, "", &[], std::slice::from_ref(s), &mut out),
        }
    }
    out
}

fn collect<'a>(
    ast: &'a Ast,
    fn_name: &'a str,
    params: &'a [Param],
    stmts: &'a [Stmt],
    out: &mut Vec<Candidate<'a>>,
) {
    let mut stack: Vec<&Stmt> = stmts.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => continue,
            Stmt::Try {
                body,
                had_catch: true,
                catch_param: Some(p),
                catch_type: None,
                catch_body,
                ..
            } if !suspends(ast, body) => {
                if let Some((cast, ann)) = cast_reads(ast, catch_body, p)
                    && admits(ast, &ann)
                {
                    out.push(Candidate {
                        fn_name,
                        params,
                        body,
                        cast,
                        ann,
                    });
                }
            }
            _ => {}
        }
        crate::ast_closure_param_tag::push_child_stmts(s, &mut stack);
    }
}

/// Whether `body` suspends: a `yield` of the generator-driven state
/// machine, or an `await` an SSA-lowered async body or module parks at
/// (the parser's `e.value` read, `Ast::await_value_reads`). A resumed
/// suspension raises whatever the caller or the promise hands in.
fn suspends(ast: &Ast, body: &[Stmt]) -> bool {
    let mut found = false;
    crate::ast::walk_stmts(body, &mut |s| {
        found |= matches!(s, Stmt::Yield(_) | Stmt::YieldInto { .. });
    });
    let mut roots: Vec<ExprId> = Vec::new();
    for s in body {
        visit_stmt_exprs(s, &mut |e| roots.push(e));
    }
    while let Some(e) = roots.pop() {
        found |= ast.await_value_reads.contains(&e);
        visit_expr_children(ast, e, &mut |c| roots.push(c));
    }
    found
}

/// A type the typed binding checks by tag alone: a number, a boolean,
/// or a declared non-generic class.
fn admits(ast: &Ast, ann: &str) -> bool {
    matches!(ann, "number" | "boolean")
        || (ast.class_parents.contains_key(ann) && !ann.contains('<'))
}

/// The first of the reads of the catch parameter `p` in `body` and the
/// type they cast it to, when every read of `p` is the operand of an
/// `as` to that one type and the body declares no other `p`.
pub(crate) fn cast_reads(ast: &Ast, body: &[Stmt], p: &str) -> Option<(ExprId, String)> {
    if crate::ast::body_reads(ast, body, p) != NameRead::Direct {
        return None;
    }
    let mut shadowed = false;
    crate::ast::walk_stmts(body, &mut |s| {
        shadowed |= match s {
            Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => name == p,
            Stmt::Try { catch_param, .. } => catch_param.as_deref() == Some(p),
            _ => false,
        };
    });
    if shadowed {
        return None;
    }
    let mut roots: Vec<ExprId> = Vec::new();
    for s in body {
        visit_stmt_exprs(s, &mut |e| roots.push(e));
    }
    let (mut reads, mut casts) = (0usize, Vec::new());
    while let Some(e) = roots.pop() {
        match ast.get_expr(e) {
            Expr::Ident(n) if n == p => reads += 1,
            Expr::As { expr, ty_ann } if matches!(ast.get_expr(*expr), Expr::Ident(n) if n == p) => {
                casts.push((e, ty_ann.trim()));
            }
            _ => {}
        }
        visit_expr_children(ast, e, &mut |c| roots.push(c));
    }
    let (first, ann) = *casts.iter().min_by_key(|(e, _)| e.0)?;
    (reads == casts.len() && casts.iter().all(|(_, a)| *a == ann)).then(|| (first, ann.to_string()))
}

//! Where every FREE identifier of a `with` body sits — which is what
//! decides its rewrite. The shadowing question is [`super::scope`]'s;
//! this file asks the position question and records the answer.
//!
//! The walk is deliberately conservative in one direction: a shape it
//! does not recognise sets `err` instead of being skipped, so an
//! unhandled corner is a diagnostic rather than a name that quietly
//! resolved to the lexical binding.

use super::Position;
use super::scope::Scope;
use super::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Param, StaticInit, Stmt};

/// The synth name the parser leaves at a class EXPRESSION's use site.
/// Both spellings get one — an anonymous `class {}` and a named
/// `class Inner {}` alike — so this prefix identifies every one of
/// them.
const CLASS_EXPR_PREFIX: &str = "__ClassExpr_";

/// Entry point: record every free identifier occurrence of one `with`
/// body, nested function bodies included.
pub(crate) fn collect_body(
    ast: &Ast,
    body: &[Stmt],
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    let scope = Scope::with_body(body);
    for s in body {
        collect_stmt(ast, s, &scope, sites, err);
    }
}

fn collect_stmt(
    ast: &Ast,
    s: &Stmt,
    scope: &Scope,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match s {
        Stmt::LetDecl {
            init, is_var: true, ..
        } if !scope.in_nested_fn() && !matches!(ast.get_expr(*init), Expr::Uninit) => {
            // `var` does NOT shadow the object — it hoists to the
            // enclosing function's scope, which sits BEHIND the object
            // record — while its initialiser IS evaluated in front of
            // it. `super::var_split` has already separated the two for
            // every `var` it can reach, so an initialiser still
            // standing here is one it declined: a declarator shape it
            // does not recognise. Loud rather than silently landing on
            // the hoisted binding.
            refuse(
                err,
                "a `var` initialiser that could not be split from its declaration",
            );
        }
        Stmt::ClassDecl { .. } => {
            collect_class(ast, s, scope, err);
            return;
        }
        Stmt::FnDecl { params, body, .. } => {
            collect_fn(ast, params, body, scope, sites, err);
            return;
        }
        _ => {}
    }
    for e in stmt_exprs(s) {
        collect_expr(ast, e, scope, sites, err);
    }
    for child in stmt_children_ref(s) {
        collect_stmt(ast, child, scope, sites, err);
    }
}

/// A function nested in the body: its free names resolve when it is
/// CALLED, so they are guarded exactly like the body's own and the
/// `with` binding is captured by the closure — which works because the
/// binding is an ordinary block-scoped `let` that the capture analysis
/// downstream has no reason to treat specially.
///
/// A parameter default is walked in the FUNCTION's scope, not the
/// body's, even though tr evaluates defaults at the call site: the
/// guard it grows names the with binding, so a default is only
/// well-formed while that call site is still inside the block. One
/// that escapes the block fails loudly on the unknown binding rather
/// than answering the wrong name.
fn collect_fn(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    scope: &Scope,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    let child = scope.nested_fn(params, body);
    for p in params {
        if let Some(d) = p.default {
            collect_expr(ast, d, &child, sites, err);
        }
    }
    for s in body {
        collect_stmt(ast, s, &child, sites, err);
    }
}

/// A class declared in the body.
///
/// Every part of a class is strict code (§11.2.2), but strictness does
/// not take the object environment record out of the scope chain: the
/// class's free names still resolve through the object and would each
/// need a guard, exactly like a nested function's.
///
/// They cannot get one. A guard names the `with` binding, and
/// `hoist_nested_classes` lifts a nested class only when its bodies
/// resolve against TOP-LEVEL names — one that captured the binding
/// would stay nested and abort in `check.rs` as `internal: ClassDecl
/// reached check.rs (desugar didn't run?)`, which reads as a compiler
/// bug rather than as a shape tr does not cover. That is a gap in the
/// nested-class machinery, not in this pass, and it is the same gap an
/// ordinary `{ let a = 1; class K { m() { return a } } }` hits with no
/// `with` anywhere in sight.
///
/// So the answer here is exact rather than approximate: a CLOSED class
/// needs no guard, hoists, and runs — that is the whole set tr can
/// express — and everything else is refused naming the part that
/// carries the free name.
fn collect_class(ast: &Ast, s: &Stmt, scope: &Scope, err: &mut Option<String>) {
    let Stmt::ClassDecl {
        name,
        type_params,
        parent,
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    else {
        return;
    };
    if parent.is_some() {
        // §15.7.14 evaluates the heritage in THIS scope, so the object
        // can supply the parent — including for a name that is
        // otherwise a global, since `with (o)` over an `o` carrying
        // `Error` shadows the real one. The static class lane keys on
        // a NAME, which leaves nowhere to put a guard; a non-Ident
        // heritage expression is refused the same way until RFC
        // 20260815 knife 3 rewrites the clause with the guard inline.
        match ast.parent_ident_name(*parent) {
            Some(p) if scope.shadows(p) => {}
            _ => {
                refuse(err, "an `extends` clause the object could supply");
                return;
            }
        }
    }
    // Recorded into a scratch list, not the body's: a class that needs
    // any of these guards is refused, so its sites must never reach
    // the rewrite.
    let mut probe: Vec<(ExprId, Position)> = Vec::new();
    let inner = scope.class_body(name, type_params);
    if let Some(c) = ctor {
        // Instance field initialisers are folded into the constructor
        // body by the parser (`finalize_class_field_inits`), so this
        // walk covers them too.
        collect_fn(ast, &c.params, &c.body, &inner, &mut probe, err);
    }
    for m in methods.iter().chain(static_methods.iter()) {
        collect_fn(ast, &m.params, &m.body, &inner, &mut probe, err);
    }
    for si in static_init {
        match si {
            StaticInit::Field(f) => collect_expr(ast, f.init, &inner, &mut probe, err),
            StaticInit::Block(v) => {
                for st in v {
                    collect_stmt(ast, st, &inner, &mut probe, err);
                }
            }
        }
    }
    // Computed member keys do not hang off the statement — the parser
    // stashes them in side tables under a `__ccm_<n>__` sentinel keyed
    // by the class name. Walking only the statement tree would miss
    // `with (o) { class K { [k]() {} } }` entirely, which is the shape
    // of every silent error this chapter has produced.
    for ((cls, _), key) in &ast.class_computed_keys {
        if cls == name {
            collect_expr(ast, *key, &inner, &mut probe, err);
        }
    }
    for (cls, _, init) in &ast.class_computed_static_fields {
        if cls == name {
            collect_expr(ast, *init, &inner, &mut probe, err);
        }
    }
    if probe.iter().any(|(eid, _)| free_user_name(ast, *eid)) {
        refuse(err, "a class body reading a name the object could supply");
    }
}

/// Whether this recorded site is a name the USER wrote — compiler
/// minted spellings (`__supercall__m` for `super.m()`, a lifted class
/// expression's `__ClassExpr_<n>`) are never names the object can
/// supply, and are settled by their spelling the same way the rewrite
/// settles them.
fn free_user_name(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Ident(n) if !n.starts_with("__"))
}

/// The top-level declaration a class-expression use site names. Absent
/// only for the one shape the parser declines to buffer (`class x
/// extends x {}`, whose use site throws a TDZ ReferenceError), which
/// carries no body to ask about.
fn find_class_decl<'a>(ast: &'a Ast, name: &str) -> Option<&'a Stmt> {
    ast.stmts
        .iter()
        .find(|s| matches!(s, Stmt::ClassDecl { name: n, .. } if n == name))
}

fn refuse(err: &mut Option<String>, what: &'static str) {
    err.get_or_insert(format!(
        "{what} inside a `with` body is not yet supported \
         (RFC 20260814, ES §14.11)"
    ));
}

/// Walk one expression subtree, recording each `Ident` with the
/// position that decides its rewrite.
///
/// A bare `Ident` records ITSELF as a read, rather than being recorded
/// by whoever passed it down. 刀 1 did the latter, and so never saw an
/// identifier that is a statement's whole expression: `with (o) {
/// return x }`, `if (x)`, `throw x` all left `x` resolving lexically
/// while the object carried it. A node that owns its own site cannot
/// be missed by a caller that forgets to announce it.
///
/// The positions that are NOT plain reads (callee, assignment target,
/// the sole operand of `typeof` / `++`) are still recorded by the
/// parent, which is the only node that knows which one applies — and
/// the parent does not then descend into that child, so no site is
/// recorded twice.
fn collect_expr(
    ast: &Ast,
    eid: ExprId,
    scope: &Scope,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match ast.get_expr(eid) {
        Expr::Ident(n) => {
            // A class EXPRESSION written in the body is not HERE. The
            // parser buffers it to `synth_classes` and splices it at
            // TOP LEVEL, leaving only this name hop at the use site —
            // so by the time this pass runs its body already sits
            // outside the block, where no walk of the body can reach
            // it. Left alone, every free name in that body resolved
            // lexically and the program died on `unknown identifier`
            // at run time: a wrong answer, and one a stderr-
            // classifying harness reads as tr having RUN the program.
            //
            // Going to find the declaration is what makes the verdict
            // exact instead of a blanket refusal — a closed class
            // expression needed no guard in the first place and keeps
            // working.
            if n.starts_with(CLASS_EXPR_PREFIX) {
                if let Some(decl) = find_class_decl(ast, n) {
                    collect_class(ast, decl, scope, err);
                }
                return;
            }
            if object_may_supply(ast, eid, n, scope) {
                sites.push((eid, Position::Read));
            }
            return;
        }
        Expr::Call { callee, args } => {
            match ast.get_expr(*callee) {
                Expr::Ident(n) => {
                    if object_may_supply(ast, *callee, n, scope) {
                        sites.push((*callee, Position::Callee(eid)));
                    }
                }
                _ => collect_expr(ast, *callee, scope, sites, err),
            }
            for a in args {
                collect_expr(ast, *a, scope, sites, err);
            }
            return;
        }
        Expr::Assign { target, value } => {
            let Expr::Ident(tn) = ast.get_expr(*target) else {
                collect_expr(ast, *target, scope, sites, err);
                collect_expr(ast, *value, scope, sites, err);
                return;
            };
            if !object_may_supply(ast, *target, tn, scope) {
                collect_expr(ast, *value, scope, sites, err);
                return;
            }
            // `n op= v` reaches here as `n = n op v` with a CLONED
            // left operand (the parser's compound desugar). That clone
            // is the same reference the write resolves, not a second
            // one, so it must not get a guard of its own — the assign
            // rewrite fills it in per branch and §9.1.1.2.1 HasBinding
            // stays evaluated once, as the spec's single
            // ResolveBinding requires.
            let compound_lhs = match ast.get_expr(*value) {
                Expr::BinOp { left, .. } if matches!(ast.get_expr(*left), Expr::Ident(ln) if ln == tn) => {
                    Some(*left)
                }
                _ => None,
            };
            sites.push((*target, Position::Assign(eid, compound_lhs)));
            match (compound_lhs, ast.get_expr(*value)) {
                (Some(_), Expr::BinOp { right, .. }) => {
                    collect_expr(ast, *right, scope, sites, err);
                }
                _ => collect_expr(ast, *value, scope, sites, err),
            }
            return;
        }
        Expr::PostIncr { target, .. } => {
            wrapping_operand(ast, eid, *target, scope, sites, err);
            return;
        }
        Expr::TypeOf { expr } => {
            wrapping_operand(ast, eid, *expr, scope, sites, err);
            return;
        }
        Expr::Delete { expr } => {
            wrapping_operand(ast, eid, *expr, scope, sites, err);
            return;
        }
        Expr::ArrowFn { params, body, .. } => {
            // A function EXPRESSION (the parser gives `function () {}`
            // and `() => {}` the same node): its free names resolve at
            // call time, through the object captured by the closure.
            collect_fn(ast, params, body, scope, sites, err);
            return;
        }
        Expr::Closure { .. } => {
            // Post-`lift_arrow_fns` shape. This pass runs long before
            // that, so reaching one means the pipeline order moved.
            refuse(err, "an already-lifted closure");
            return;
        }
        _ => {}
    }
    for c in expr_children(ast, eid) {
        collect_expr(ast, c, scope, sites, err);
    }
}

/// The sole operand of `typeof` / `++` / `--`, whose WRAPPING node is
/// what gets replaced.
fn wrapping_operand(
    ast: &Ast,
    outer: ExprId,
    operand: ExprId,
    scope: &Scope,
    sites: &mut Vec<(ExprId, Position)>,
    err: &mut Option<String>,
) {
    match ast.get_expr(operand) {
        Expr::Ident(n) => {
            if object_may_supply(ast, operand, n, scope) {
                sites.push((operand, Position::Wrapping(outer)));
            }
        }
        _ => collect_expr(ast, operand, scope, sites, err),
    }
}

/// The single gate every recorded site passes through: is this `Ident`
/// occurrence one the `with` object is allowed to answer?
///
/// Two ways it is not. A name bound in front of the object record is
/// [`Scope`]'s question. The other is a global the PARSER named as
/// machinery rather than because the program said it — the exemptions
/// elsewhere in this pass are settled by SPELLING (`__`-prefixed names
/// are never the user's), which cannot work for these: they are
/// spelled `Object` / `String` / `Promise`, exactly what a user would
/// write and exactly what an object can carry. So they are keyed by
/// node.
///
/// One gate rather than a check at each site because the four
/// recording positions do not funnel: the callee, assignment-target
/// and sole-operand arms each recognise their own `Ident` without
/// going through the read arm. An exemption written in one of them
/// silently applies to a quarter of the pass — which is how the first
/// version of this shipped, exempting `String` in read position while
/// `String(sub)` (a CALLEE) went on being hijacked.
fn object_may_supply(ast: &Ast, eid: ExprId, n: &str, scope: &Scope) -> bool {
    !scope.shadows(n) && !ast.synth_global_refs.contains(&eid)
}

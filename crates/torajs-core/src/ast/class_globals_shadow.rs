//! Shadow-aware `Ident(C) → Ident(__class_C)` rewrite for
//! `class_globals` — the value-position class-reference rewrite must
//! respect lexical scope.
//!
//! The original rewrite was a flat arena scan: EVERY `Ident` spelling
//! a known class name became the class-object sentinel, including
//! references a local binding actually owns. `class R {} function
//! helper(R: any) { return R + 1 }` silently added 1 to the CLASS
//! (its toString, under `+`), not to the parameter — the
//! param-shadow-class bug (rotation 428 discovery).
//!
//! This walk carries the set of class names still visible at each
//! point. Entering any function scope (statement `FnDecl`, arena
//! `ArrowFn` in all its identities — arrow, fn-expr, generator,
//! method) removes the names its parameters or its OWN scope's
//! declarations rebind; a `catch (R)` removes the name for the catch
//! body. Scope declarations are collected to the function boundary
//! (a nested fn's `var` belongs to the nested fn), at fn-level
//! granularity: a deep block-scoped `let R` hides `R` for the whole
//! enclosing fn body, which can only turn a would-be-rewritten
//! reference into a LOUD unknown identifier — the same precision as
//! `rebinds_in_stmt`, never a silent wrong.
//!
//! `new C()` carries its class in a `String` field (`Expr::New`), not
//! an `Ident` — the deconflict census owns that spelling; this walk
//! only serves value-position reads.

use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashSet;

/// What the rewrite can still see at one point: the class names in
/// scope, plus at most one self-alias.
///
/// The alias is a named class EXPRESSION's own name. §15.7.14 step 3
/// binds a class's name inside the class scope for both halves of the
/// grammar, but only the declaration half spells that binding with
/// the name the source wrote — an expression carries a synth binding
/// (`__ClassExpr_<id>`) with the user's spelling parked in
/// `class_expr_display_names`, so its body's `C` matches no known
/// class and reaches the checker as an unknown identifier. Carrying
/// the pair here is what lets the body see it, under exactly the
/// shadowing the declaration half already gets.
#[derive(Clone)]
struct Shadow {
    active: HashSet<String>,
    /// (spelling in the source, name it denotes).
    alias: Option<(String, String)>,
    /// Keep descending even with nothing to rewrite yet — the
    /// self-name pass starts with an empty state and is looking for
    /// the class expressions that fill it in.
    scanning: bool,
}

impl Shadow {
    fn is_empty(&self) -> bool {
        self.active.is_empty() && self.alias.is_none() && !self.scanning
    }

    /// The name this one denotes, if any. The alias wins: the class's
    /// own binding lives in a scope inside every other, so it shadows
    /// a same-named class declared outside.
    fn rename(&self, n: &str) -> Option<String> {
        if let Some((from, to)) = &self.alias
            && from == n
        {
            return Some(to.clone());
        }
        if self.active.contains(n) {
            return Some(format!("__class_{n}"));
        }
        None
    }

    /// Drop whatever the given names rebind.
    fn without(&self, declared: &HashSet<String>) -> Shadow {
        Shadow {
            active: self.active.difference(declared).cloned().collect(),
            alias: self
                .alias
                .as_ref()
                .filter(|(from, _)| !declared.contains(from))
                .cloned(),
            scanning: self.scanning,
        }
    }
}

/// Give a named class expression's body the one binding §15.7.14
/// step 3 says it has: its own name.
///
/// This has to run BEFORE `desugar_classes`, and that is the whole
/// reason it is a pass of its own rather than a flag on the walk
/// below. The body's `C` is a reference to the class expression, but
/// spelled the same as any other `C` — so with a class of that name
/// declared outside, `desugar_classes` resolves the reference to the
/// OUTER class (measured: a static read answered the outer class's
/// field) and by the time the value-ref rewrite runs there is no
/// `C` left to reclaim. Renaming it here, to the synth name the class
/// actually binds under, makes every pass downstream treat it exactly
/// like a declaration's own name — including the value-ref rewrite,
/// which then turns it into the sentinel with no special case.
pub(super) fn rewrite_class_expr_self_names(ast: &mut Ast) {
    if ast.class_expr_display_names.is_empty() {
        return;
    }
    let sh = Shadow {
        active: HashSet::new(),
        alias: None,
        scanning: true,
    };
    let stmts = std::mem::take(&mut ast.stmts);
    for s in &stmts {
        rewrite_stmt(ast, s, &sh);
    }
    ast.stmts = stmts;
}

/// Rewrite every value-position class reference the given scope can
/// still see. `class_set` is the full set of known class names.
pub(super) fn rewrite_class_value_refs(ast: &mut Ast, class_set: &HashSet<String>) {
    let sh = Shadow {
        active: class_set.clone(),
        alias: None,
        scanning: false,
    };
    let stmts = std::mem::take(&mut ast.stmts);
    for s in &stmts {
        rewrite_stmt(ast, s, &sh);
    }
    ast.stmts = stmts;
}

/// The names a function scope rebinds: its parameters plus every
/// declaration in its own var scope (any block depth, stopping at
/// nested function boundaries — their declarations are their own).
fn shrunk(sh: &Shadow, params: &[Param], body: &[Stmt]) -> Shadow {
    let mut declared: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
    collect_scope_decls(body, &mut declared);
    sh.without(&declared)
}

fn collect_scope_decls(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, .. }
            | Stmt::FnDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_scope_decls(std::slice::from_ref(then_branch), out);
                if let Some(eb) = else_branch.as_deref() {
                    collect_scope_decls(std::slice::from_ref(eb), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                collect_scope_decls(std::slice::from_ref(body), out)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init.as_deref() {
                    collect_scope_decls(std::slice::from_ref(i), out);
                }
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::ForOf { var_name, body, .. } => {
                out.insert(var_name.clone());
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::ForOfSplitIter { var_name, body, .. } => {
                out.insert(var_name.clone());
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_scope_decls(body, out);
                collect_scope_decls(catch_body, out);
                if let Some(fb) = finally_body {
                    collect_scope_decls(fb, out);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_scope_decls(&c.body, out);
                }
                if let Some(d) = default {
                    collect_scope_decls(d, out);
                }
            }
            Stmt::Block(b) | Stmt::Multi(b) => collect_scope_decls(b, out),
            _ => {}
        }
    }
}

fn rewrite_stmts(ast: &mut Ast, stmts: &[Stmt], sh: &Shadow) {
    for s in stmts {
        rewrite_stmt(ast, s, sh);
    }
}

fn rewrite_stmt(ast: &mut Ast, s: &Stmt, sh: &Shadow) {
    if sh.is_empty() {
        return;
    }
    match s {
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Throw(eid) => rewrite_expr(ast, *eid, sh),
        Stmt::LetDecl { init, .. } => rewrite_expr(ast, *init, sh),
        Stmt::FnDecl { params, body, .. } => {
            let inner = shrunk(sh, params, body);
            for p in params {
                if let Some(d) = p.default {
                    rewrite_expr(ast, d, &inner);
                }
            }
            rewrite_stmts(ast, body, &inner);
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            // The bodies below are inside the class scope, which binds
            // the class's own name. A named class EXPRESSION is the
            // only one that needs saying so: it binds under a synth
            // name with the source spelling parked in the display
            // channel. The alias replaces any from an enclosing class
            // expression — that is what "inside" means — while an
            // anonymous one keeps the enclosing alias, being inside it
            // too.
            let display = ast.class_expr_display_names.get(name).cloned();
            let body_sh = match display {
                Some(d) => Shadow {
                    active: sh.active.clone(),
                    alias: Some((d, name.clone())),
                    scanning: sh.scanning,
                },
                None => sh.clone(),
            };
            if let Some(c) = ctor {
                let inner = shrunk(&body_sh, &c.params, &c.body);
                rewrite_stmts(ast, &c.body, &inner);
            }
            for m in methods.iter().chain(static_methods) {
                let inner = shrunk(&body_sh, &m.params, &m.body);
                rewrite_stmts(ast, &m.body, &inner);
            }
            for si in static_init {
                if let super::StaticInit::Block(b) = si {
                    rewrite_stmts(ast, b, &body_sh);
                }
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(ast, *cond, sh);
            rewrite_stmt(ast, then_branch, sh);
            if let Some(eb) = else_branch.as_deref() {
                rewrite_stmt(ast, eb, sh);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rewrite_expr(ast, *cond, sh);
            rewrite_stmt(ast, body, sh);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref() {
                rewrite_stmt(ast, i, sh);
            }
            if let Some(c) = cond {
                rewrite_expr(ast, *c, sh);
            }
            if let Some(st) = step {
                rewrite_expr(ast, *st, sh);
            }
            rewrite_stmt(ast, body, sh);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            rewrite_expr(ast, *elem_expr, sh);
            rewrite_stmt(ast, body, sh);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rewrite_expr(ast, *parent, sh);
            rewrite_expr(ast, *sep, sh);
            rewrite_stmt(ast, body, sh);
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(ast, body, sh),
        Stmt::Block(b) | Stmt::Multi(b) => rewrite_stmts(ast, b, sh),
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_stmts(ast, body, sh);
            // catch (R) shadows R for exactly the catch body
            let catch_sh = match catch_param {
                Some(p) => sh.without(&std::iter::once(p.clone()).collect()),
                None => sh.clone(),
            };
            rewrite_stmts(ast, catch_body, &catch_sh);
            if let Some(fb) = finally_body {
                rewrite_stmts(ast, fb, sh);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_expr(ast, *scrutinee, sh);
            for c in cases {
                rewrite_expr(ast, c.value, sh);
                rewrite_stmts(ast, &c.body, sh);
            }
            if let Some(d) = default {
                rewrite_stmts(ast, d, sh);
            }
        }
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => rewrite_expr(ast, *eid, sh),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner.as_deref() {
                rewrite_stmt(ast, i, sh);
            }
        }
        _ => {}
    }
}

fn rewrite_expr(ast: &mut Ast, eid: ExprId, sh: &Shadow) {
    let mut seen: HashSet<ExprId> = HashSet::new();
    let mut stack: Vec<ExprId> = vec![eid];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Expr::Ident(n) = &ast.exprs[id.0 as usize] {
            if let Some(new_name) = sh.rename(n.as_str()) {
                ast.exprs[id.0 as usize] = Expr::Ident(new_name);
            }
            continue;
        }
        match ast.exprs[id.0 as usize].clone() {
            Expr::BinOp { left, right, .. }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            }
            | Expr::Sequence { left, right }
            | Expr::Assign {
                target: left,
                value: right,
            }
            | Expr::Index {
                obj: left,
                index: right,
            }
            | Expr::OptIndex {
                obj: left,
                index: right,
            }
            | Expr::InstanceOf {
                expr: left,
                rhs: right,
            } => {
                stack.push(left);
                stack.push(right);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::Delete { expr }
            | Expr::As { expr, .. }
            | Expr::PostIncr { target: expr, .. } => stack.push(expr),
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => stack.push(obj),
            Expr::Call { callee, args }
            | Expr::OptCall { callee, args }
            | Expr::NewDynamic { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    stack.push(a);
                }
            }
            Expr::Array(els) => {
                for e in els {
                    stack.push(e);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    stack.push(e);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                stack.push(cond);
                stack.push(then_branch);
                stack.push(else_branch);
            }
            Expr::ArrowFn { params, body, .. } => {
                let inner = shrunk(sh, &params, &body);
                for p in &params {
                    if let Some(d) = p.default {
                        rewrite_expr(ast, d, &inner);
                    }
                }
                if !inner.is_empty() {
                    rewrite_stmts(ast, &body, &inner);
                }
            }
            _ => {}
        }
    }
}

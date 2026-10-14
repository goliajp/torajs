//! §15.7.1 class-field early errors — ContainsArguments over a field
//! initializer (ES §8.4.1: `FieldDefinition : ClassElementName
//! Initializer` — it is a Syntax Error if ContainsArguments of
//! Initializer is true).
//!
//! The walk follows the spec's ContainsArguments recursion: an
//! IdentifierReference spelled `arguments` answers true; arrow
//! functions are pierced (they inherit the outer `arguments` binding,
//! §8.4.1 recurses into ArrowFunction); ordinary function expressions
//! and declarations bind their own `arguments`, so the walk stops at
//! them. In this AST both shapes share `Expr::ArrowFn` — a function
//! expression is the same node registered in `ast.fn_expr_exprs`
//! (RFC 20260717-fnexpr-this-channel) or `ast.gen_fn_exprs`, so the
//! boundary test is side-table membership, not the variant. Nested
//! `class` bodies stop the walk too: their own field initializers
//! re-enter this check when they parse, and MethodDefinition bodies
//! bind their own `arguments`.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// The symbol a Contains walk (§8.5.1 / §8.4.1) looks for. Both share
/// the recursion — pierce arrows, stop at anything that binds its own
/// function environment — and differ only in the leaf.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainsSym {
    /// An IdentifierReference spelled `arguments` (§8.4.1).
    Arguments,
    /// The `new.target` meta-property (§8.5.1 Contains NewTarget).
    NewTarget,
}

/// §16.1.1 / §16.2.1.1 — global code (a Script's StatementList, a
/// Module's ModuleItemList) may not contain `new.target`: the walk
/// stops at every function that binds its own [[NewTarget]] and
/// pierces arrows, which inherit the global code's (rotation 636;
/// test262 `global-code/new.target` and `new.target-arrow`).
pub fn global_code_contains_new_target(ast: &Ast) -> bool {
    ast.stmts
        .iter()
        .any(|s| stmt_contains(ast, s, ContainsSym::NewTarget))
}

/// ContainsArguments of a class-field initializer expression.
pub(super) fn init_contains_arguments(ast: &Ast, eid: ExprId) -> bool {
    expr_contains(ast, eid, ContainsSym::Arguments)
}

/// The Contains recursion with its leaf left open: `hit` is asked of
/// every expression slot the walk reaches, and a `true` answer
/// short-circuits it.
///
/// The boundary rule — pierce arrows, stop at anything that binds its
/// own function environment — IS the content of §8.4.1, so it gets
/// one implementation. §19.2.1.1 extends the same field-initializer
/// prohibition to code arriving through a direct eval, and
/// `ast::desugar_eval::class_field_eval_arguments` rides this walk to
/// find those call sites rather than spelling the boundary a second
/// time.
pub(crate) fn expr_walk(ast: &Ast, eid: ExprId, hit: &mut dyn FnMut(ExprId) -> bool) -> bool {
    expr_contains_inner(ast, eid, None, hit)
}

/// Statement-list twin of [`expr_walk`] — a class static block's
/// StatementList enters here.
pub(crate) fn stmt_walk(ast: &Ast, s: &Stmt, hit: &mut dyn FnMut(ExprId) -> bool) -> bool {
    stmt_contains_inner(ast, s, None, hit)
}

/// The leaf §8.4.1 / §8.5.1 look for, asked of one slot.
fn leaf_is(ast: &Ast, eid: ExprId, sym: ContainsSym) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => sym == ContainsSym::Arguments && n == "arguments",
        Expr::NewTarget => sym == ContainsSym::NewTarget,
        _ => false,
    }
}

/// ContainsArguments of a ClassStaticBlockStatementList. §15.7.1
/// states the same early error twice — once for a field initializer,
/// once for a static block — and one walk answers both: `arguments`
/// spelled as an IdentifierReference is true, arrows are pierced (they
/// inherit the outer binding), and anything that binds its own
/// `arguments` stops it. So `static { (function () { return arguments;
/// }) }` and a nested class's method body are both legal, while
/// `static { ((x = arguments) => 0) }` is not.
pub(super) fn static_block_contains_arguments(ast: &Ast, stmts: &[Stmt]) -> bool {
    stmts
        .iter()
        .any(|s| stmt_contains(ast, s, ContainsSym::Arguments))
}

fn expr_contains(ast: &Ast, eid: ExprId, sym: ContainsSym) -> bool {
    expr_contains_inner(ast, eid, Some(sym), &mut |_| false)
}

fn stmt_contains(ast: &Ast, s: &Stmt, sym: ContainsSym) -> bool {
    stmt_contains_inner(ast, s, Some(sym), &mut |_| false)
}

fn params_contain(
    ast: &Ast,
    params: &[Param],
    sym: Option<ContainsSym>,
    hit: &mut dyn FnMut(ExprId) -> bool,
) -> bool {
    // Arrow-param defaults evaluate in the outer `arguments` scope
    // (§8.4.1 recurses on the full ArrowFunction production).
    params.iter().any(|p| {
        p.default
            .is_some_and(|d| expr_contains_inner(ast, d, sym, hit))
    })
}

fn expr_contains_inner(
    ast: &Ast,
    eid: ExprId,
    sym: Option<ContainsSym>,
    hit: &mut dyn FnMut(ExprId) -> bool,
) -> bool {
    if sym.is_some_and(|s| leaf_is(ast, eid, s)) || hit(eid) {
        return true;
    }
    match ast.get_expr(eid) {
        Expr::Ident(_)
        | Expr::NewTarget
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Elision
        | Expr::This
        | Expr::Closure { .. } => false,
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        }
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
        } => {
            expr_contains_inner(ast, *left, sym, hit) || expr_contains_inner(ast, *right, sym, hit)
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. }
        | Expr::PostIncr { target: expr, .. }
        | Expr::Member { obj: expr, .. }
        | Expr::OptChain { obj: expr, .. } => expr_contains_inner(ast, *expr, sym, hit),
        Expr::InstanceOf { expr, rhs } => {
            expr_contains_inner(ast, *expr, sym, hit) || expr_contains_inner(ast, *rhs, sym, hit)
        }
        Expr::Call { callee, args }
        | Expr::OptCall { callee, args }
        | Expr::NewDynamic { callee, args } => {
            expr_contains_inner(ast, *callee, sym, hit)
                || args.iter().any(|a| expr_contains_inner(ast, *a, sym, hit))
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            args.iter().any(|a| expr_contains_inner(ast, *a, sym, hit))
        }
        Expr::Array(items) => items.iter().any(|a| expr_contains_inner(ast, *a, sym, hit)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .any(|(_, v)| expr_contains_inner(ast, *v, sym, hit)),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_contains_inner(ast, *cond, sym, hit)
                || expr_contains_inner(ast, *then_branch, sym, hit)
                || expr_contains_inner(ast, *else_branch, sym, hit)
        }
        Expr::ArrowFn { params, body, .. } => {
            // fn-expr / generator-expr share this node — those bind
            // their own `arguments`, so the walk stops there.
            // …and so does an object-literal method / accessor
            // (MethodDefinition is function code, §15.4): rotation
            // 636's new.target gate refused `{ get x() { return
            // new.target } }` until it stopped here too.
            if ast.fn_expr_exprs.contains(&eid)
                || ast.gen_fn_exprs.contains_key(&eid)
                || ast.objlit_method_exprs.contains(&eid)
            {
                return false;
            }
            params_contain(ast, params, sym, hit)
                || body.iter().any(|s| stmt_contains_inner(ast, s, sym, hit))
        }
    }
}

fn stmt_contains_inner(
    ast: &Ast,
    s: &Stmt,
    sym: Option<ContainsSym>,
    hit: &mut dyn FnMut(ExprId) -> bool,
) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => expr_contains_inner(ast, *e, sym, hit),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            expr_contains_inner(ast, *init, sym, hit)
        }
        Stmt::YieldInto { value, .. } => expr_contains_inner(ast, *value, sym, hit),
        Stmt::Return(e) => e.is_some_and(|e| expr_contains_inner(ast, e, sym, hit)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_contains_inner(ast, *cond, sym, hit)
                || stmt_contains_inner(ast, then_branch, sym, hit)
                || else_branch
                    .as_deref()
                    .is_some_and(|s| stmt_contains_inner(ast, s, sym, hit))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_contains_inner(ast, *cond, sym, hit) || stmt_contains_inner(ast, body, sym, hit)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            expr_contains_inner(ast, *scrutinee, sym, hit)
                || cases.iter().any(|c| {
                    expr_contains_inner(ast, c.value, sym, hit)
                        || c.body.iter().any(|s| stmt_contains_inner(ast, s, sym, hit))
                })
                || default
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_contains_inner(ast, s, sym, hit)))
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref()
                .is_some_and(|s| stmt_contains_inner(ast, s, sym, hit))
                || cond.is_some_and(|e| expr_contains_inner(ast, e, sym, hit))
                || step.is_some_and(|e| expr_contains_inner(ast, e, sym, hit))
                || stmt_contains_inner(ast, body, sym, hit)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_contains_inner(ast, *parent, sym, hit)
                || expr_contains_inner(ast, *sep, sym, hit)
                || stmt_contains_inner(ast, body, sym, hit)
        }
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            expr_contains_inner(ast, *elem_expr, sym, hit)
                || forin_obj.is_some_and(|e| expr_contains_inner(ast, e, sym, hit))
                || stmt_contains_inner(ast, body, sym, hit)
        }
        Stmt::Labeled { body, .. } => stmt_contains_inner(ast, body, sym, hit),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_contains_inner(ast, s, sym, hit))
                || catch_body
                    .iter()
                    .any(|s| stmt_contains_inner(ast, s, sym, hit))
                || finally_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_contains_inner(ast, s, sym, hit)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_contains_inner(ast, s, sym, hit))
        }
        Stmt::ExportDecl { inner, .. } => inner
            .as_deref()
            .is_some_and(|s| stmt_contains_inner(ast, s, sym, hit)),
        // A nested class binds its own `arguments` in its methods and
        // initializers (which re-check when they parse), but its
        // heritage and computed keys evaluate out here: Contains on a
        // ClassTail recurses through ClassHeritage and
        // ComputedPropertyContains (§8.5.1).
        Stmt::ClassDecl {
            parent, decl_id, ..
        } => {
            parent.is_some_and(|p| expr_contains_inner(ast, p, sym, hit))
                || class_computed_keys_of(ast, *decl_id)
                    .into_iter()
                    .any(|k| expr_contains_inner(ast, k, sym, hit))
        }
        // `function` decls bind their own `arguments` scope; the rest
        // carry no expressions.
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ImportDecl { .. }
        | Stmt::Break(_)
        | Stmt::Continue(_) => false,
    }
}

/// The computed-key expressions the class `decl_id` declares, in arena
/// order so a walk that records what it meets stays deterministic.
fn class_computed_keys_of(ast: &Ast, decl_id: u32) -> Vec<ExprId> {
    let mut keys: Vec<ExprId> = ast
        .class_computed_keys
        .iter()
        .filter(|((_, sentinel), _)| {
            ast.class_computed_owner
                .get(sentinel)
                .is_some_and(|ids| ids.contains(&decl_id))
        })
        .map(|(_, &k)| k)
        .collect();
    keys.sort_by_key(|k| k.0);
    keys
}

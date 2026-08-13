//! §14.11 `with` — the semantic half (RFC 20260814 刀 1).
//!
//! A textbook engine gives `with` an OBJECT ENVIRONMENT RECORD at the
//! front of the scope chain, and every identifier resolution walks it.
//! tr resolves identifiers at compile time, so that shape would change
//! not `with` but *every name lookup in the language*.
//!
//! What makes the desugar exact instead of a shortcut is that `with`'s
//! reach is syntactically bounded: only the FREE names of its body can
//! be captured by the object. So the dynamic half is materialised at
//! exactly those sites and nowhere else — the same move `desugar_eval`
//! makes for direct eval.
//!
//! ```text
//! with (E) S   ->   { let w = __torajs_with_obj(E); S' }
//!
//! read  n      ->   (__torajs_with_has(w, "n") ? w.n : n)
//! call  n(a…)  ->   (__torajs_with_has(w, "n") ? w.n(a…) : n(a…))
//! typeof n     ->   (has ? typeof w.n : typeof n)
//! n++          ->   (has ? w.n++ : n++)
//! ```
//!
//! The membership test is re-run at every reference on purpose: the
//! object can gain or lose a property between two mentions of the same
//! name, and §9.1.1.2.1 HasBinding is evaluated per resolution. It is
//! not a value that may be hoisted to the head of the block.
//!
//! # What shadows the object, and what does not
//!
//! A LEXICAL declaration inside the body (`let` / `const` / `class`,
//! a catch parameter) sits in an environment record IN FRONT of the
//! object record, so those names are not free and are left alone.
//!
//! `var` does NOT shadow: it hoists to the function scope, which is
//! BEHIND the object record. `with (o) { var v = 1 }` writes `o.v`
//! when `o` has `v` — a classic, and the reason var names stay in the
//! free set.
//!
//! # Loud, not wrong (刀 1 boundary)
//!
//! Reads, bare-name calls, `typeof` and `++`/`--` are rewritten here.
//! Every other way a free name can be reached — assignment, compound
//! assignment, `delete`, and any nested function body (whose names
//! resolve when it is CALLED, so the object has to be captured) — is
//! refused with a diagnostic naming the knife that will land it.
//! Leaving them unrewritten would silently resolve to the lexical
//! binding instead of the object, which is the one outcome this file
//! is not allowed to produce.

use super::{Ast, Expr, ExprId, Stmt};

/// The binding the parser mints per `with` site. Also the marker the
/// walk below keys on: a Block whose head is a `let` of this shape IS
/// a with statement, and nothing else can spell the name.
pub(crate) const WITH_OBJ_PREFIX: &str = "__with_";

/// §14.11.2 step 2 — ToObject on the head expression (a TypeError for
/// null / undefined, which a plain binding would not raise).
pub(crate) const WITH_OBJ_FN: &str = "__torajs_with_obj";

/// §9.1.1.2.1 HasBinding for an object environment record, including
/// the §9.1.1.2.1 step 3 `@@unscopables` filter.
const WITH_HAS_FN: &str = "__torajs_with_has";

/// Both helpers as ordinary TS, spliced in through the `parse_into`
/// convention (`desugar_using` / `inject_disposable_stack`先例) rather
/// than hand-built as arena nodes: the bodies are exactly the spec
/// steps and stay readable as such.
///
/// `__torajs_with_has` is §9.1.1.2.1 verbatim — HasProperty first (the
/// `in` operator IS that abstract operation), then the `@@unscopables`
/// filter, which is why `with ([]) { values }` still answers the outer
/// `values` rather than `Array.prototype.values`.
const HELPER_SRC: &str = r#"
function __torajs_with_obj(v: any): any {
  if (v === null || v === undefined) {
    throw new TypeError("Cannot convert undefined or null to object");
  }
  return Object(v);
}
function __torajs_with_has(w: any, k: any): boolean {
  if (!(k in w)) { return false; }
  const u: any = w[Symbol.unscopables];
  if (u === null || u === undefined) { return true; }
  if (typeof u !== "object" && typeof u !== "function") { return true; }
  return !u[k];
}
"#;

/// `Some(msg)` = a shape 刀 1 refuses; the caller reports it and stops.
pub fn desugar_with(ast: &mut Ast) -> Option<String> {
    if !ast.has_with_stmt {
        return None;
    }
    inject_helpers(ast);
    let mut stmts = std::mem::take(&mut ast.stmts);
    let mut err = None;
    for s in &mut stmts {
        rewrite_stmt(ast, s, &mut err);
    }
    ast.stmts = stmts;
    err
}

fn inject_helpers(ast: &mut Ast) {
    let src = HELPER_SRC.to_string();
    let tokens = crate::lexer::tokenize(&src).expect("with helper lex");
    let offset = crate::parser::parse_into(&src, &tokens, ast).expect("with helper parse");
    // The parsed spans index HELPER_SRC, not the user's file, so they
    // are meaningless once spliced in — and a `Function.prototype
    // .toString` style slice of `ast.source` with them panics ("end
    // byte index 178 is out of bounds for string of length 82"). The
    // (0,0) "no user source" sentinel is the stmt.rs contract for an
    // injected declaration; `inject_disposable_stack` /
    // `desugar_using::inject_helpers` stamp the same one.
    for s in ast.stmts[offset..].iter_mut() {
        if let Stmt::FnDecl { span, .. } = s {
            *span = crate::lexer::Span { start: 0, end: 0 };
        }
    }
    // Front-splice so the helpers are declared before any use, the
    // same ordering `inject_disposable_stack` needs.
    let injected: Vec<Stmt> = ast.stmts.split_off(offset);
    ast.stmts.splice(0..0, injected);
}

/// Depth-first so an inner `with` is already rewritten when the outer
/// one runs. That ordering is what makes nesting come out right: the
/// inner rewrite leaves its fall-through `Ident(n)` behind, and the
/// outer turns exactly that into its own guarded read — which is the
/// scope chain, spelled as nested conditionals.
fn rewrite_stmt(ast: &mut Ast, s: &mut Stmt, err: &mut Option<String>) {
    for child in stmt_children(s) {
        rewrite_stmt(ast, child, err);
    }
    let Stmt::Block(items) = s else { return };
    let Some(Stmt::LetDecl { name, .. }) = items.first() else {
        return;
    };
    if !name.starts_with(WITH_OBJ_PREFIX) {
        return;
    }
    let w = name.clone();
    let body: Vec<Stmt> = items.drain(1..).collect();
    let mut bound = std::collections::HashSet::new();
    let mut sites: Vec<(ExprId, Position)> = Vec::new();
    for st in &body {
        collect_stmt(ast, st, &mut bound, &mut sites, err);
    }
    for (eid, pos) in sites {
        let Expr::Ident(n) = ast.get_expr(eid) else {
            continue;
        };
        let n = n.clone();
        if bound.contains(&n) || n.starts_with("__") {
            continue;
        }
        match pos {
            Position::Read => rewrite_read(ast, eid, &w, &n),
            Position::Callee(call) => rewrite_call(ast, call, eid, &w, &n),
            Position::Wrapping(outer) => rewrite_wrapping(ast, outer, &w, &n),
            Position::Refused(what) => {
                err.get_or_insert(format!(
                    "`{n}` is reached by {what} inside a `with` body — not yet supported \
                     (RFC 20260814, ES §14.11)"
                ));
            }
        }
    }
    items.extend(body);
}

/// `n` -> `(__torajs_with_has(w, "n") ? w.n : n)`, written OVER the
/// original arena slot so no parent link has to be found or rebuilt.
/// The fall-through branch is a fresh `Ident` node carrying the same
/// name, which is what the outer `with` of a nested pair rewrites.
fn rewrite_read(ast: &mut Ast, eid: ExprId, w: &str, n: &str) {
    let cond = has_call(ast, w, n);
    let then_branch = with_member(ast, w, n);
    let else_branch = ast.add_expr(Expr::Ident(n.to_string()));
    ast.with_fallthrough_idents.insert(else_branch);
    ast.exprs[eid.0 as usize] = Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    };
}

/// `n(a…)` -> `(has ? w.n(a…) : n(a…))`. The whole CALL is replaced,
/// not just its callee: §9.1.1.2.3 WithBaseObject makes the object the
/// receiver when the name came from it, and a rewritten callee alone
/// would call it with `undefined`. The argument subtrees are cloned
/// rather than shared — only one branch runs, but the arena is a tree
/// everywhere else and a shared node is a shape no other pass expects.
fn rewrite_call(ast: &mut Ast, call: ExprId, callee: ExprId, w: &str, n: &str) {
    let Expr::Call { args, .. } = ast.get_expr(call) else {
        return;
    };
    let args = args.clone();
    let cond = has_call(ast, w, n);
    let recv = with_member(ast, w, n);
    let then_branch = ast.add_expr(Expr::Call {
        callee: recv,
        args: args.clone(),
    });
    let mut cloner = super::clone_body::BodyCloner::new(ast);
    let cloned: Vec<ExprId> = args.iter().map(|a| cloner.clone_expr(*a)).collect();
    let plain = ast.add_expr(Expr::Ident(n.to_string()));
    ast.with_fallthrough_idents.insert(plain);
    let else_branch = ast.add_expr(Expr::Call {
        callee: plain,
        args: cloned,
    });
    let _ = callee;
    ast.exprs[call.0 as usize] = Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    };
}

/// `typeof n` / `n++` / `n--`: the whole WRAPPING node is replaced with
/// a guard whose two arms are the same operator applied to `w.n` and to
/// `n`. Nothing has to be cloned — the operand is the only child, and
/// each arm mints its own — so unlike a compound assignment this shape
/// keeps §9.1.1.2.1 HasBinding evaluated exactly ONCE, which is what
/// the spec's single ResolveBinding does.
///
/// `typeof` is the shape that must not answer through the fall-through
/// alone: §13.5.3 answers `"undefined"` for an unresolvable name
/// instead of throwing, and the object arm has to be consulted before
/// that rule applies.
fn rewrite_wrapping(ast: &mut Ast, outer: ExprId, w: &str, n: &str) {
    let cond = has_call(ast, w, n);
    let obj_operand = with_member(ast, w, n);
    let plain_operand = ast.add_expr(Expr::Ident(n.to_string()));
    ast.with_fallthrough_idents.insert(plain_operand);
    let (then_branch, else_branch) = match ast.get_expr(outer) {
        Expr::TypeOf { .. } => (
            Expr::TypeOf { expr: obj_operand },
            Expr::TypeOf {
                expr: plain_operand,
            },
        ),
        Expr::PostIncr { is_inc, .. } => {
            let is_inc = *is_inc;
            (
                Expr::PostIncr {
                    target: obj_operand,
                    is_inc,
                },
                Expr::PostIncr {
                    target: plain_operand,
                    is_inc,
                },
            )
        }
        _ => return,
    };
    let then_branch = ast.add_expr(then_branch);
    let else_branch = ast.add_expr(else_branch);
    ast.exprs[outer.0 as usize] = Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    };
}

fn has_call(ast: &mut Ast, w: &str, n: &str) -> ExprId {
    let f = ast.add_expr(Expr::Ident(WITH_HAS_FN.to_string()));
    let obj = ast.add_expr(Expr::Ident(w.to_string()));
    let key = ast.add_expr(Expr::String(n.to_string()));
    ast.add_expr(Expr::Call {
        callee: f,
        args: vec![obj, key],
    })
}

fn with_member(ast: &mut Ast, w: &str, n: &str) -> ExprId {
    let obj = ast.add_expr(Expr::Ident(w.to_string()));
    ast.add_expr(Expr::Member {
        obj,
        name: n.to_string(),
    })
}

/// Where an `Ident` occurrence sits, which is what decides its
/// rewrite. Everything not yet handled carries the phrase the
/// diagnostic uses, so a refusal names the shape rather than the pass.
pub(crate) enum Position {
    Read,
    /// The callee of this `Call`.
    Callee(ExprId),
    /// The sole operand of this single-child node (`typeof` / `++` /
    /// `--`), which is replaced whole.
    Wrapping(ExprId),
    Refused(&'static str),
}

mod collect;
pub(crate) use collect::{collect_stmt, stmt_children};

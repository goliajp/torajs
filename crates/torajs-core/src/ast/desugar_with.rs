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
//! n = v        ->   (has ? (w.n = v) : (n = v))
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
//! # Nested functions
//!
//! A function written inside the body resolves its free names when it
//! is CALLED, which can be long after the block has been left. Nothing
//! special is needed for that: the binding is an ordinary block-scoped
//! `let`, so the guards grown inside the function body capture it the
//! way any closure captures any outer binding — and because the
//! membership test lives at the reference rather than at the head of
//! the block, a call made later sees the object as it is THEN.
//!
//! # Loud, not wrong
//!
//! Reads, bare-name calls, `typeof`, `++`/`--`, `delete`, assignment
//! (plain and compound), `var` initialisers and nested function bodies
//! are rewritten here. What is left is refused with a diagnostic
//! naming the shape: several declarators in one `for` initialiser, and
//! a class — declaration or expression — that reads any name the
//! object could supply. Leaving them unrewritten would silently
//! resolve to the lexical binding instead of the object, which is the
//! one outcome this file is not allowed to produce.
//!
//! # Classes
//!
//! A class is strict code throughout (§11.2.2), which forbids WRITING
//! a `with` inside one but does not take the object environment record
//! out of the chain around one: a class written in the body still
//! resolves its free names through the object. A CLOSED class needs no
//! guard and is accepted; anything else is refused, because a guard
//! names the `with` binding and `hoist_nested_classes` lifts a nested
//! class only when its bodies resolve at top level. See
//! [`collect::collect_class`].

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

/// Why this pass stopped. The two reasons are not the same kind of
/// thing and must not read as the same kind of thing: one is the
/// program being ill-formed, the other is tr not covering a shape yet.
///
/// Which matters beyond wording — every harness that reads tr's
/// stderr classifies by the FIRST LINE's prefix, so a refusal printed
/// as a bare `error:` is indistinguishable from an uncaught runtime
/// throw. That is how every refused `with` program spent this chapter
/// counted as "tr ran it and it failed" instead of "tr declined it".
pub enum WithReject {
    /// §14.11.1 — a `with` in strict code. The program is invalid, and
    /// tr recognising that is a correct answer, not a gap.
    SyntaxError(String),
    /// A shape this desugar does not cover yet.
    Unsupported(String),
}

impl WithReject {
    /// The line to print, prefix included. The prefixes are the ones
    /// every other early gate in the prelude uses.
    pub fn message(&self) -> String {
        match self {
            WithReject::SyntaxError(m) => format!("parse error: {m}"),
            WithReject::Unsupported(m) => format!("not yet supported: {m}"),
        }
    }
}

/// `Some(_)` = this pass stopped; the caller prints
/// [`WithReject::message`] and returns an error.
pub fn desugar_with(ast: &mut Ast) -> Option<WithReject> {
    if !ast.has_with_stmt {
        return None;
    }
    if !ast.sloppy_script_goal {
        // §14.11.1 — a WithStatement is a SyntaxError in strict code,
        // and a module always is. A parser-level one never reaches
        // here (`judge_with_statement` refuses it inside a strict
        // function, `triage_strict_reserved_idents` at the goal), so
        // what does is one the EVAL inline brought in.
        //
        // Whose strictness governs it is the eval's own, not the
        // module's — `Function("…")` makes a sloppy function even from
        // strict code (t262 12.10.1-5-s), while a direct `eval` in a
        // strict function inherits strict and must raise SyntaxError
        // (12.10.1-10-s). `desugar_eval` does not currently hand that
        // verdict down, so this refuses both rather than RUN a `with`
        // that strict code forbids. Loud costs 12.10.1-5-s its pass;
        // running it would be a semantic wrong, which costs more.
        return Some(WithReject::SyntaxError(
            "`with` is not allowed in strict code (ES §14.11.1) — reached here through an \
             eval / Function inline, whose own strictness this pass cannot yet see \
             (RFC 20260814)"
                .to_string(),
        ));
    }
    inject_helpers(ast);
    let mut err = None;
    // One statement at a time rather than taking the whole list. The
    // walk needs `&mut Ast` to mint its guard nodes, so the statement
    // being rewritten has to come out — but only that one: a class
    // EXPRESSION's declaration was spliced at top level by the parser,
    // and `collect` goes to find it there by name. Taking the whole
    // list left `ast.stmts` empty for the duration, so that lookup
    // answered "no such class" for every program and the refusal it
    // exists to raise never fired.
    for i in 0..ast.stmts.len() {
        let mut s = std::mem::replace(&mut ast.stmts[i], Stmt::Multi(Vec::new()));
        rewrite_stmt(ast, &mut s, &mut err);
        ast.stmts[i] = s;
    }
    err.map(WithReject::Unsupported)
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
    for arrow in arrow_nodes(ast, s) {
        rewrite_arrow_body(ast, arrow, err);
    }
    let Stmt::Block(items) = s else { return };
    let Some(Stmt::LetDecl { name, .. }) = items.first() else {
        return;
    };
    if !name.starts_with(WITH_OBJ_PREFIX) {
        return;
    }
    let w = name.clone();
    let mut body: Vec<Stmt> = items.drain(1..).collect();
    // Before the sites are collected: a `var` initialiser is an
    // assignment in the with scope even though its declaration belongs
    // to the enclosing function, and splitting the two turns it into
    // an ordinary write site.
    split_var_inits(ast, &mut body);
    let mut sites: Vec<(ExprId, Position)> = Vec::new();
    collect_body(ast, &body, &mut sites, err);
    // Writes last: the then-arm CLONES the value expression, and the
    // clone has to carry the guards the read rewrites put there. A
    // clone taken before those ran would resolve its own free names
    // lexically — the silent-wrong this whole pass exists to avoid.
    sites.sort_by_key(|(_, p)| u8::from(matches!(p, Position::Assign(..))));
    for (eid, pos) in sites {
        let Expr::Ident(n) = ast.get_expr(eid) else {
            continue;
        };
        let n = n.clone();
        // Compiler-minted names are never the user's free names — the
        // shadowing question (`__with_<n>` itself, the helpers, the
        // loop temporaries) is settled by their spelling.
        if n.starts_with("__") {
            continue;
        }
        match pos {
            Position::Read => rewrite_read(ast, eid, &w, &n),
            Position::Callee(call) => rewrite_call(ast, call, eid, &w, &n),
            Position::Wrapping(outer) => rewrite_wrapping(ast, outer, &w, &n),
            Position::Assign(node, lhs) => rewrite_assign(ast, node, lhs, &w, &n),
        }
    }
    items.extend(body);
}

/// Re-enter the statement rewrite on a function expression's body,
/// which hangs off an arena expression rather than off a statement and
/// so is invisible to `stmt_children` (see [`arrow_nodes`]).
///
/// The body is taken out of the arena for the duration: the rewrite
/// needs `&mut Ast` to mint its guard nodes, and the body it is
/// rewriting would otherwise be borrowed out of that same arena.
fn rewrite_arrow_body(ast: &mut Ast, arrow: ExprId, err: &mut Option<String>) {
    let Expr::ArrowFn { body, .. } = &mut ast.exprs[arrow.0 as usize] else {
        return;
    };
    let mut body = std::mem::take(body);
    for s in &mut body {
        rewrite_stmt(ast, s, err);
    }
    if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[arrow.0 as usize] {
        *slot = body;
    }
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

/// `typeof n` / `n++` / `n--` / `delete n`: the whole WRAPPING node is
/// replaced with a guard whose two arms are the same operator applied
/// to `w.n` and to `n`. Nothing has to be cloned — the operand is the
/// only child, and each arm mints its own — so unlike a compound
/// assignment this shape keeps §9.1.1.2.1 HasBinding evaluated exactly
/// ONCE, which is what the spec's single ResolveBinding does.
///
/// `typeof` is the shape that must not answer through the fall-through
/// alone: §13.5.3 answers `"undefined"` for an unresolvable name
/// instead of throwing, and the object arm has to be consulted before
/// that rule applies. `delete` is the same argument one step further:
/// §13.5.1.2 evaluates the reference first, and a reference the object
/// supplies is a PROPERTY reference, so the object arm must really
/// remove `w.n` where the fall-through only answers a boolean.
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
        Expr::Delete { .. } => (
            Expr::Delete { expr: obj_operand },
            Expr::Delete {
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

/// `n = v` -> `(has ? (w.n = v) : (n = v))`, and the compound form
/// `n op= v` (which reaches here as `n = n op v`) with the cloned left
/// operand filled in per branch: `w.n` on the object side, the plain
/// name on the fall-through side.
///
/// Filling it in rather than guarding it is what keeps §9.1.1.2.1
/// HasBinding evaluated ONCE for the whole compound. The spec resolves
/// the reference a single time and then does GetValue + PutValue on
/// it; two independent tests would disagree if evaluating `v` added or
/// removed the property between them.
///
/// The value expression is cloned for the object arm rather than
/// shared: only one arm ever runs, but the arena is a tree everywhere
/// else and a shared node is a shape no other pass expects.
fn rewrite_assign(ast: &mut Ast, node: ExprId, compound_lhs: Option<ExprId>, w: &str, n: &str) {
    let Expr::Assign { value, .. } = ast.get_expr(node) else {
        return;
    };
    let value = *value;
    let cond = has_call(ast, w, n);
    let mut cloner = super::clone_body::BodyCloner::new(ast);
    let cloned_value = cloner.clone_expr(value);
    let cloned_lhs = compound_lhs.map(|l| cloner.map[&l]);
    if let (Some(orig), Some(clone)) = (compound_lhs, cloned_lhs) {
        let obj_read = with_member(ast, w, n);
        ast.exprs[clone.0 as usize] = ast.exprs[obj_read.0 as usize].clone();
        // The fall-through arm keeps the plain name, which is already
        // what the node holds — it only needs the diagnostic
        // exemption every fall-through read gets.
        ast.with_fallthrough_idents.insert(orig);
    }
    let then_target = with_member(ast, w, n);
    let then_branch = ast.add_expr(Expr::Assign {
        target: then_target,
        value: cloned_value,
    });
    let else_target = ast.add_expr(Expr::Ident(n.to_string()));
    ast.with_fallthrough_idents.insert(else_target);
    let else_branch = ast.add_expr(Expr::Assign {
        target: else_target,
        value,
    });
    ast.exprs[node.0 as usize] = Expr::Ternary {
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
/// rewrite. A shape with no position of its own is refused by the
/// collect walk itself (statement-level: a multi-declarator `var` in a
/// `for` initialiser, a class that reads a name the object could
/// supply), so this enum carries only the forms that ARE handled.
pub(crate) enum Position {
    Read,
    /// The callee of this `Call`.
    Callee(ExprId),
    /// The sole operand of this single-child node (`typeof` / `++` /
    /// `--` / `delete`), which is replaced whole.
    Wrapping(ExprId),
    /// The target of this `Assign`. The second field is the compound
    /// desugar's cloned left operand when there is one (`n op= v`
    /// arrives as `n = n op v`), which the rewrite fills in per branch
    /// instead of guarding separately.
    Assign(ExprId, Option<ExprId>),
}

mod collect;
mod scope;
mod var_split;
mod walk;
use collect::collect_body;
use var_split::split_var_inits;
use walk::{arrow_nodes, stmt_children};

//! Which bare-name assignments in a fragment WOULD create a global
//! binding — §9.1.1.4.6 SetMutableBinding with `S` = false, asked
//! statically of one piece of a program.
//!
//! This file used to answer that for the WHOLE program and act on the
//! answer, by synthesizing a hoisted `var <name>;` per never-declared
//! assignment target. That retired in rotation 631 (RFC
//! 20260911-global-object-record): §9.1.1.4.6 creates a property on
//! the GLOBAL OBJECT, at run time, and the synthesis got two things
//! wrong about that. It was unconditional where the write is not —
//! `with (o) { foo = 42 }` creates `foo` only when the object does not
//! already carry it, which is a fact about the run — and a `var`
//! binding is non-configurable where an implicit global is
//! configurable. The lowering does it now
//! (`ssa_lower_assign_ident` → `__torajs_global_ident_set`), off the
//! checker's own `undeclared_reads` verdict.
//!
//! What remains is the walk, for its one remaining consumer:
//! `desugar_eval` needs the names a STRICT eval text assigns to, so
//! the lowering can leave them on the throw. §19.2.1.1 gives eval code
//! its own strictness and strict code creates no globals, so
//! `eval('"use strict"; unresolvable = null')` is a ReferenceError —
//! and after the splice no later pass can tell those statements from
//! the sloppy program they landed in. Answering with THIS walk rather
//! than a second one is what keeps the two in step.
//!
//! The walk is a TREE walk, not an arena scan, because strictness is
//! positional: a function whose directive prologue says `"use strict"`
//! gives its assignments the strict posture, so sites inside a strict
//! body are not evidence of anything. Inherited strictness needs no
//! propagation here — the parser materializes a directive into every
//! strict-by-inheritance body (`parser::strict_directive`).
//!
//! Names it will not name (each measured, none speculative): the
//! §19.1 readonly value properties (`undefined` / `NaN` / `Infinity`),
//! `eval` / `arguments`, the contextual keywords (`let` / `yield` /
//! `await` / `static`), and any known builtin global.

use super::sloppy_this_prologue::has_use_strict_directive;
use super::{Ast, Expr, ExprId, Stmt};

/// The bare names an assignment inside `stmts` would create if this
/// code were sloppy — the same question `synthesize_sloppy_implicit_
/// globals` asks of the whole program, asked of one fragment.
///
/// The eval splice needs it because a text that is STRICT must create
/// nothing: §19.2.1.1 gives eval code its own strictness, and after
/// the splice no later pass can tell those statements from the sloppy
/// program around them. Answering with this walk rather than a second
/// one keeps the two in step.
pub(super) fn assigned_bare_names(ast: &Ast, stmts: &[Stmt]) -> Vec<String> {
    let declared = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    let mut walk = Walk {
        declared: &declared,
        out: &mut out,
        seen: Vec::new(),
    };
    walk.walk_block(ast, stmts);
    out
}

fn admit(n: &str, declared: &std::collections::HashSet<String>, out: &mut Vec<String>) {
    if declared.contains(n)
        || out.iter().any(|seen| seen == n)
        || crate::check::is_known_builtin_global(n)
        || matches!(
            n,
            "undefined"
                | "NaN"
                | "Infinity"
                | "eval"
                | "arguments"
                | "let"
                | "yield"
                | "await"
                | "static"
        )
    {
        return;
    }
    out.push(n.to_string());
}

/// The walk. `seen` holds the names READ so far in the expression
/// currently being evaluated, in evaluation order — and that ordering
/// IS the rule. §13.15.2 calls GetValue on an assignment's value
/// before PutValue ever runs, and §13.15.1 and friends evaluate
/// operands left to right, so a read this walk reaches before an
/// assignment is a read that RUNS before that write. There is then
/// nothing left for the write to create: the read threw.
///
/// That single question covers what used to be three separate
/// spellings — `x += 1` (the parser desugars it, so the read sits in
/// the value), `x = y + x` (a plain assignment reading its own
/// target), and `x + (x = 1)` (an earlier sibling), the last of which
/// a value-only check cannot see.
struct Walk<'a> {
    declared: &'a std::collections::HashSet<String>,
    out: &'a mut Vec<String>,
    seen: Vec<String>,
}

impl Walk<'_> {
    fn note_read(&mut self, n: &str) {
        if !self.seen.iter().any(|s| s == n) {
            self.seen.push(n.to_string());
        }
    }

    fn already_read(&self, n: &str) -> bool {
        self.seen.iter().any(|s| s == n)
    }

    /// A body whose evaluation is DEFERRED — every function-like. Its
    /// assignments still create bindings (the body runs, if ever,
    /// after the expression we are inside), but nothing outside it has
    /// run yet when it does, so it starts with an empty record and
    /// leaves the caller's untouched: `x = function () { return x; }`
    /// really does create the binding.
    fn walk_deferred(&mut self, ast: &Ast, stmts: &[Stmt]) {
        let outer = std::mem::take(&mut self.seen);
        self.walk_block(ast, stmts);
        self.seen = outer;
    }

    /// A region that may not run at all — a branch, a loop body, a
    /// `catch`. Everything before it did run, so it INHERITS the
    /// record; but its own reads are not reads that anything after it
    /// can count on, so they do not escape.
    fn walk_branch(&mut self, f: impl FnOnce(&mut Self)) {
        let outer = self.seen.clone();
        f(self);
        self.seen = outer;
    }

    /// A block's statements are separate statements, and whether an
    /// earlier one ran is not a question this pass answers (it may
    /// have thrown), so each starts over.
    fn walk_block(&mut self, ast: &Ast, stmts: &[Stmt]) {
        for s in stmts {
            self.seen.clear();
            self.walk_stmt(ast, s);
        }
    }

    fn walk_stmt(&mut self, ast: &Ast, s: &Stmt) {
        match s {
            // A strict body's assignments keep the strict posture — skip
            // the whole subtree (strictness is contagious downward, and
            // strict-by-inheritance bodies carry a parser-written
            // directive of their own).
            Stmt::FnDecl { body, .. } => {
                if !has_use_strict_directive(body, &ast.exprs) {
                    self.walk_deferred(ast, body);
                }
            }
            Stmt::Expr(e) | Stmt::Yield(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
                self.walk_expr(ast, *e)
            }
            Stmt::YieldInto { value, .. } => self.walk_expr(ast, *value),
            Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => self.walk_expr(ast, *init),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(ast, *cond);
                self.walk_branch(|w| w.walk_stmt(ast, then_branch));
                if let Some(eb) = else_branch {
                    self.walk_branch(|w| w.walk_stmt(ast, eb));
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
                self.walk_expr(ast, *cond);
                self.walk_branch(|w| w.walk_stmt(ast, body));
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(i) = init {
                    self.walk_stmt(ast, i);
                }
                if let Some(c) = cond {
                    self.walk_expr(ast, *c);
                }
                // Body before step — that is the order they run in.
                self.walk_branch(|w| {
                    w.walk_stmt(ast, body);
                    if let Some(st) = step {
                        w.walk_expr(ast, *st);
                    }
                });
            }
            Stmt::ForOf {
                elem_expr, body, ..
            } => {
                self.walk_expr(ast, *elem_expr);
                self.walk_branch(|w| w.walk_stmt(ast, body));
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                self.walk_expr(ast, *parent);
                self.walk_expr(ast, *sep);
                self.walk_branch(|w| w.walk_stmt(ast, body));
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                self.walk_expr(ast, *scrutinee);
                for c in cases {
                    self.walk_expr(ast, c.value);
                    self.walk_branch(|w| w.walk_block(ast, &c.body));
                }
                if let Some(d) = default {
                    self.walk_branch(|w| w.walk_block(ast, d));
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                self.walk_branch(|w| w.walk_block(ast, body));
                self.walk_branch(|w| w.walk_block(ast, catch_body));
                if let Some(fb) = finally_body {
                    self.walk_block(ast, fb);
                }
            }
            Stmt::Block(inner) => self.walk_block(ast, inner),
            // Not a block: `Stmt::Multi` is how the parser spells ONE
            // comma expression (`a, b;` / a destructuring assignment),
            // whose segments run in order, unconditionally. Threading
            // the record through them is the same threading
            // `Expr::Sequence` gets in expression position.
            Stmt::Multi(inner) => {
                for s in inner {
                    self.walk_stmt(ast, s);
                }
            }
            Stmt::Labeled { body, .. } => self.walk_stmt(ast, body),
            Stmt::ExportDecl { inner, .. } => {
                if let Some(i) = inner {
                    self.walk_stmt(ast, i);
                }
            }
            _ => {}
        }
    }

    fn walk_expr(&mut self, ast: &Ast, eid: ExprId) {
        match ast.get_expr(eid) {
            Expr::Ident(n) => self.note_read(n),
            // `typeof x` and `delete x` do NOT read an unresolvable
            // name — §13.5.1.1 answers `"undefined"` and §13.5.1.2
            // answers `true` rather than throwing — so a bare name
            // under them is not a read that got there first. Anything
            // nested under them still is.
            Expr::TypeOf { expr } | Expr::Delete { expr } => {
                if !matches!(ast.get_expr(*expr), Expr::Ident(_)) {
                    self.walk_expr(ast, *expr);
                }
            }
            Expr::Assign { target, value } => {
                if let Expr::Ident(n) = ast.get_expr(*target) {
                    // The value runs before this write, so walk it
                    // first and then ask whether anything — the value
                    // or an earlier sibling — has already read the
                    // name.
                    self.walk_expr(ast, *value);
                    if !self.already_read(n) {
                        admit(n, self.declared, self.out);
                    }
                } else {
                    self.walk_expr(ast, *target);
                    self.walk_expr(ast, *value);
                }
            }
            // A strict arrow body keeps its assignments out, same as
            // FnDecl above (arrows inherit strictness, and inherited
            // strict bodies carry a parser-written directive).
            Expr::ArrowFn { body, .. } => {
                if !has_use_strict_directive(body, &ast.exprs) {
                    self.walk_deferred(ast, body);
                }
            }
            Expr::Call { callee, args } | Expr::OptCall { callee, args, .. } => {
                self.walk_expr(ast, *callee);
                for &a in args {
                    self.walk_expr(ast, a);
                }
            }
            Expr::New { args, .. } => {
                for &a in args {
                    self.walk_expr(ast, a);
                }
            }
            Expr::NewDynamic { callee, args, .. } => {
                self.walk_expr(ast, *callee);
                for &a in args {
                    self.walk_expr(ast, a);
                }
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => self.walk_expr(ast, *obj),
            Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
                self.walk_expr(ast, *obj);
                self.walk_expr(ast, *index);
            }
            Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
                self.walk_expr(ast, *left);
                self.walk_expr(ast, *right);
            }
            Expr::Nullish { lhs, rhs } => {
                self.walk_expr(ast, *lhs);
                self.walk_branch(|w| w.walk_expr(ast, *rhs));
            }
            Expr::Unary { expr, .. }
            | Expr::PostIncr { target: expr, .. }
            | Expr::As { expr, .. }
            | Expr::Spread { expr } => self.walk_expr(ast, *expr),
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(ast, *cond);
                self.walk_branch(|w| w.walk_expr(ast, *then_branch));
                self.walk_branch(|w| w.walk_expr(ast, *else_branch));
            }
            Expr::Array(items) => {
                for &el in items {
                    self.walk_expr(ast, el);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    self.walk_expr(ast, *e);
                }
            }
            _ => {}
        }
    }
}

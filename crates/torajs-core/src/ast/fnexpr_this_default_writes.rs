//! Which names the program writes, and how many times — the half of
//! receiver certainty a `const` gets for free.
//!
//! [`super::fnexpr_this_default`]'s census proves a receiver by
//! proving its initializer, which is only a proof because a `const`
//! cannot be reassigned. test262 writes `var map = new Map()` far
//! more often than it writes `const`, and by the time this pass looks,
//! `desugar_var_hoist` has split that into a mutable `let map: any =
//! <uninit>` at the top of the scope plus a statement-position `map =
//! new Map()`. Nothing about the receiver got less certain — the slot
//! still only ever holds the one value — but the initializer stopped
//! carrying it.
//!
//! So this answers the wider question: **the names the whole program
//! writes exactly once, paired with the value written.** A `const`
//! clears that bar trivially and is left to the other census; what
//! reaches certainty through here is the mutable binding whose one
//! write the program never follows with a second.
//!
//! **Completeness is the entire job.** A missed write is not a missed
//! optimisation — it hands a callback `undefined` for `this` where the
//! program overwrote the receiver with something that binds its own,
//! and it does that silently, which the design principles rank worst.
//! An over-count only costs the loud reject that is today's answer.
//! So every way a name can be written or re-bound counts, and the
//! doubtful ones count too:
//!
//! * `x = v` and `x++` / `x--` — the two expression forms that write
//!   a bare name. A `++` write cannot name its value, so it drops the
//!   name outright rather than proving anything.
//! * A declaration with a real initializer (`let m = new Map()`) is
//!   itself the one write. The `var` shape's hoisted declaration is
//!   not: its init is the `Uninit` sentinel and the value arrives as
//!   the assignment.
//! * Every re-binding of the name anywhere — a nested declaration, a
//!   `for`-`of` loop variable, a `catch` parameter, a `using`
//!   declaration, a function or class name, a parameter, an imported
//!   name. The census is name-keyed, not scope-keyed, exactly like
//!   the `const` one: a second binding of the name drops it whether
//!   or not the two could ever be confused.
//! * Destructuring needs no arm of its own — it reaches this pass
//!   already expanded into one assignment (or one declaration) per
//!   name by the parse-time destructuring desugars.
//!
//! The walk skips generic-twin bodies for the reason
//! `collect_const_decls` documents (399-04): a `__cmany_` body is
//! `desugar_classes_generic_twin`'s copy of a body already counted,
//! and counting the copy would drop the name from certainty — which
//! there is not a loud reject but a silent fall back to the enclosing
//! method's own receiver.

use std::collections::{HashMap, HashSet};

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::fnexpr_this_names::is_twin_body_name;
use super::{Ast, Expr, ExprId, Param, Stmt};

/// The names written exactly once and bound exactly once, paired with
/// the value written.
pub(super) struct SingleWrite {
    once: HashMap<String, ExprId>,
}

impl SingleWrite {
    pub(super) fn scan(ast: &Ast) -> Self {
        let mut census = Census::default();
        walk_stmts(ast, &ast.stmts, false, &mut census);
        let mut once = HashMap::new();
        for (name, sites) in &census.writes {
            if census.bound.get(name).copied() != Some(1) {
                continue;
            }
            if let [Some(value)] = sites[..] {
                once.insert(name.clone(), value);
            }
        }
        Self { once }
    }

    /// The subset whose one written value satisfies `pred` — the same
    /// shape predicate the `const` census applies to an initializer.
    pub(super) fn names(
        &self,
        exprs: &[Expr],
        pred: &dyn Fn(&[Expr], ExprId) -> bool,
    ) -> HashSet<String> {
        self.once
            .iter()
            .filter(|(_, value)| pred(exprs, **value))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

#[derive(Default)]
struct Census {
    /// Binding sites per name. Anything but exactly one drops it.
    bound: HashMap<String, usize>,
    /// Write sites per name. `None` is a write whose value this cannot
    /// name (`x++`), and drops the name as surely as a second write.
    writes: HashMap<String, Vec<Option<ExprId>>>,
}

impl Census {
    fn bind(&mut self, name: &str) {
        *self.bound.entry(name.to_string()).or_insert(0) += 1;
    }

    fn write(&mut self, name: &str, value: Option<ExprId>) {
        self.writes.entry(name.to_string()).or_default().push(value);
    }

    fn bind_params(&mut self, ast: &Ast, params: &[Param]) {
        for p in params {
            self.bind(&p.name);
            if let Some(default) = p.default {
                walk_expr(ast, default, self);
            }
        }
    }
}

fn walk_stmts(ast: &Ast, stmts: &[Stmt], in_twin: bool, census: &mut Census) {
    for s in stmts {
        walk_stmt(ast, s, in_twin, census);
    }
}

fn walk_stmt(ast: &Ast, s: &Stmt, in_twin: bool, census: &mut Census) {
    let twin = in_twin || matches!(s, Stmt::FnDecl { name, .. } if is_twin_body_name(name));
    if !twin {
        record_bindings(ast, s, census);
        for e in stmt_exprs(s) {
            walk_expr(ast, e, census);
        }
    }
    for child in stmt_children_ref(s) {
        walk_stmt(ast, child, twin, census);
    }
    match s {
        // `stmt_children_ref` stops at a function body on purpose —
        // its caller enters a nested scope deliberately. This census
        // wants the whole program, so it enters every one.
        Stmt::FnDecl { params, body, .. } => {
            if !twin {
                census.bind_params(ast, params);
            }
            walk_stmts(ast, body, twin, census);
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(i) = inner {
                walk_stmt(ast, i, twin, census);
            }
            if !twin && let Some(d) = default_expr {
                walk_expr(ast, *d, census);
            }
        }
        _ => {}
    }
}

/// The names this one statement binds, plus the write a declaration
/// with a real initializer performs.
fn record_bindings(ast: &Ast, s: &Stmt, census: &mut Census) {
    match s {
        Stmt::LetDecl {
            mutable,
            name,
            init,
            ..
        } => {
            census.bind(name);
            // A `const` is the other census's business; a mutable
            // declaration's initializer is this one's first write —
            // unless it is the hoisted `var`'s sentinel, whose value
            // arrives later as an assignment.
            if *mutable && !matches!(ast.get_expr(*init), Expr::Uninit) {
                census.write(name, Some(*init));
            }
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            census.bind(var_name)
        }
        Stmt::UsingDecl { name, .. } => census.bind(name),
        Stmt::Try {
            catch_param: Some(p),
            ..
        } => census.bind(p),
        // A method body is a top-level `FnDecl` by now (`desugar_
        // classes` pass 3 leaves a `TypeDecl` behind), so the name is
        // all a class contributes here.
        Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } => census.bind(name),
        Stmt::ImportDecl {
            default,
            namespace,
            named,
            ..
        } => {
            for n in default.iter().chain(namespace.iter()) {
                census.bind(n);
            }
            // Both halves of `a as b`: which one is the local binding
            // does not matter to a census that only ever refuses.
            for (n, alias) in named {
                census.bind(n);
                if let Some(a) = alias {
                    census.bind(a);
                }
            }
        }
        _ => {}
    }
}

fn walk_expr(ast: &Ast, root: ExprId, census: &mut Census) {
    let mut pending = vec![root];
    while let Some(eid) = pending.pop() {
        match ast.get_expr(eid) {
            Expr::Assign { target, value } => {
                if let Expr::Ident(n) = ast.get_expr(*target) {
                    census.write(n, Some(*value));
                }
            }
            Expr::PostIncr { target, .. } => {
                if let Expr::Ident(n) = ast.get_expr(*target) {
                    census.write(n, None);
                }
            }
            // `lift_arrow_fns` has turned every function expression
            // this pass can still reach into a `Closure` over a
            // top-level `FnDecl`. A body that outlived the lift would
            // hide its writes from a census whose whole value is
            // completeness, so enter it rather than assume.
            Expr::ArrowFn { params, body, .. } => {
                census.bind_params(ast, params);
                walk_stmts(ast, body, false, census);
            }
            _ => {}
        }
        pending.extend(expr_children(ast, eid));
    }
}

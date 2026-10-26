//! §8.1.1.1 step 4 — the temporal dead zone of a `let` / `const`, for
//! the one shape a compiler can settle without tracking
//! initialization at run time: a read written EARLIER in the same
//! statement list than the declaration itself.
//!
//! ```text
//! console.log(a);   // ReferenceError: Cannot access 'a' before …
//! const a = 5;
//! ```
//!
//! tr answered `5`. The read resolves because two independent lanes
//! give a literal-init top-level `const` a home that carries no
//! position: the checker's `pass_2` registers every literal-init
//! top-level binding into `globals` before it walks a single
//! statement (so a named fn declared BEFORE the `const` can read it —
//! which is legal, and is why the registration exists), and the
//! lowerer's inline-literal lane (`ssa_lower_ident::
//! try_inline_const_literal`) scans the whole statement list for the
//! declaration and inlines its value. Neither asks where the read is.
//! The result was the worst shape available: a wrong ANSWER, silently,
//! where every other unresolvable read is loud.
//!
//! What makes this decidable at all is the boundary the walk stops at.
//! A read sitting directly in top-level statement `i` executes before
//! statement `j > i` runs — unconditionally, for every control shape a
//! statement list can hold, because the list is linear and nothing
//! re-enters an earlier statement. The mark is therefore not a claim
//! that the read HAPPENS (it may sit in a branch nobody takes); it is
//! a claim that IF it happens, the declaration has not run. That is
//! exactly what the emitted raise says, at exactly the read's own
//! site.
//!
//! A function body is where that reasoning stops FOR THE TOP-LEVEL
//! LIST: `function g() { return a }` written before the `const` reads
//! it whenever `g` is CALLED, which is normally after. So no walk
//! carries an outer list's names into a fn body. Calling such a fn
//! early (`g(); const a = 5`) is still wrong, and still answers `5`;
//! settling it needs a call graph, and is registered rather than
//! guessed at here.
//!
//! But the reasoning itself is about a STATEMENT LIST, not about the
//! top level, and every list is linear the same way. A body's own
//! `const` is in dead zone for a read written earlier in that same
//! body, whenever the body runs:
//!
//! ```text
//! function f() { console.log(a); const a = 5 }   // f() throws
//! { console.log(a); const a = 5 }                // the block throws
//! ```
//!
//! So the rule runs once per list — the top level, every block, every
//! function body — each seeing only its own declarations. Nothing
//! crosses a list boundary, which is what keeps a false raise
//! impossible: an inner list's read of an outer name is the CALLED
//! shape above, and stays unmarked.
//!
//! A switch's CaseBlock is one declarative environment spread across
//! the case bodies (§14.12.4), and the bodies in source order are one
//! list for this rule: entering at any clause, control only moves
//! forward through them, so a read written in an earlier clause than
//! the declaration — or earlier in the same clause — is a dead-zone
//! read whenever it happens. Whether a read AFTER the declaration
//! finds it initialized depends on the clause control entered at;
//! `switch_tdz_flag` settles that at run time.
//!
//! Two more narrowings, both to keep a false raise impossible:
//!
//!   - **Synthesized names never participate.** Late pipeline passes
//!     APPEND declarations (`synthesize_forwarders` extends
//!     `ast.stmts` with `__forward_*` lets whose readers were written
//!     earlier), so relative position stops meaning source position
//!     for those. Every such name is `__`-prefixed and user code
//!     cannot spell one, so the prefix is dropped where the
//!     declarations are collected — which is the only place a name
//!     can enter the wanted set, so no synthesized READ can be marked
//!     either.
//!   - **A name declared anywhere inside the reading statement is
//!     skipped.** `{ let a = 1; console.log(a) }` reads the inner
//!     binding, not the top-level one. Rather than track block scopes,
//!     the subtree is asked whether it binds the name at all; a true
//!     TDZ read that happens to share a statement with an unrelated
//!     inner binding of the same name goes unmarked, which is the safe
//!     direction.
//!
//! Write targets are marked too, and land in the same set: §6.2.5.5
//! GetValue and §6.2.5.6 PutValue are the same Reference, dead zone
//! and all, and `Ast::undeclared_reads` already keeps both kinds of
//! occurrence together for exactly that reason. The assign lane reads
//! the mark off the TARGET eid, which the read lane never lowers, so
//! the two never collide.
//!
//! A `const` target is the exception, and it is not this rule's:
//! `a = 1` where `a` is `const` is an EARLY error (§13.15.1 —
//! AssignmentTargetType is not simple), which bun reports without
//! running the program at all. Whatever tr does with that shape, it
//! is not a dead-zone raise, so const targets stay off the mark.

use std::collections::{HashMap, HashSet};

mod descend;

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};
use descend::descend_lists;

/// Fill `ast.tdz_reads`. Runs last in the AST pipeline, on the tree
/// the lowerer will read, so no later rewrite can move a marked
/// occurrence out from under its mark.
pub fn mark_tdz_reads(ast: &mut Ast) {
    let mut marks: HashSet<ExprId> = HashSet::new();
    mark_list(ast, &super::toplevel_stmts_flat(ast), &mut marks);
    for s in &ast.stmts {
        descend_lists(ast, s, &mut marks);
    }
    // §16.2.1.6 — a read the module resolver judged unresolvable in
    // its own module has no binding to be in the dead zone of; the
    // flat position that put it there belongs to another module's
    // declaration, and the read raises "not defined" instead.
    marks.retain(|e| !ast.module_foreign_reads.contains(e));
    // Extend, not replace: the parser already marked the reads a
    // for-in / for-of head puts in its own dead zone
    // (`forof_binding::mark_forof_head_tdz`).
    ast.tdz_reads.extend(marks);
}

/// `Multi` shares the surrounding scope, so it is spliced rather than
/// treated as a list of its own — the same flattening the top level
/// gets. So does `desugar_using`'s wrapper: `try { <the rest of the
/// list> } catch (__usc_N) { … } finally { … }` holds the statements
/// written after a `using` declaration, in the scope they were
/// written in, and a read before the wrapper of a name declared
/// inside it (`console.log(x); await using x = null`) is the dead
/// zone exactly as it would be without the wrapper.
fn flatten<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a Stmt>) {
    for s in stmts {
        match s {
            Stmt::Multi(inner) => flatten(inner, out),
            Stmt::Try {
                body,
                catch_param: Some(p),
                ..
            } if p.starts_with("__usc_") => flatten(body, out),
            other => out.push(other),
        }
    }
}

/// Run the rule on one more list, then keep descending. A list's own
/// declarations are the only ones it sees.
///
/// `pub(super)` since RFC 20260915 item 2: a generator / async
/// machine body asks the same question BEFORE its lets lift into
/// fields (`desugar_generators_tdz`), where the list order this rule
/// reads still exists.
pub(super) fn run_nested(ast: &Ast, stmts: &[Stmt], out: &mut HashSet<ExprId>) {
    let mut flat: Vec<&Stmt> = Vec::new();
    flatten(stmts, &mut flat);
    mark_list(ast, &flat, out);
    for s in stmts {
        descend_lists(ast, s, out);
    }
}

fn mark_list(ast: &Ast, flat: &[&Stmt], marks: &mut HashSet<ExprId>) {
    let mut decl_at: HashMap<&str, usize> = HashMap::new();
    let mut mutable_decl: HashSet<&str> = HashSet::new();
    for (i, s) in flat.iter().enumerate() {
        if let Stmt::LetDecl {
            name,
            is_var: false,
            mutable,
            ..
        } = s
            && !name.starts_with("__")
            && decl_at.insert(name.as_str(), i).is_none()
            && *mutable
        {
            mutable_decl.insert(name.as_str());
        }
    }
    if decl_at.is_empty() {
        return;
    }
    for (i, s) in flat.iter().enumerate() {
        // A declaration's OWN initializer runs before the binding is
        // initialized (§14.3.1.2 step 1: the initializer is evaluated,
        // then InitializeReferencedBinding), so `const a = a + 1` /
        // `let x = { x }` read the dead zone every time. The name is
        // not "later" than its own statement, and `collect_bound_names`
        // would shadow it, so it is asked directly of the init. (The
        // checker used to call the read undeclared — `a is not
        // defined` — the right class for the wrong reason.)
        if let Stmt::LetDecl {
            name,
            init,
            is_var: false,
            ..
        } = s
            && decl_at.get(name.as_str()) == Some(&i)
        {
            let own: HashSet<&str> = std::iter::once(name.as_str()).collect();
            mark_expr(ast, *init, &own, &HashSet::new(), marks);
        }
        let later: Vec<&str> = decl_at
            .iter()
            .filter(|(_, j)| **j > i)
            .map(|(n, _)| *n)
            .collect();
        if later.is_empty() {
            continue;
        }
        let mut shadowed: HashSet<&str> = HashSet::new();
        collect_bound_names(s, &mut shadowed);
        let wanted: HashSet<&str> = later
            .into_iter()
            .filter(|n| !shadowed.contains(n))
            .collect();
        if wanted.is_empty() {
            continue;
        }
        let wanted_mut: HashSet<&str> = wanted
            .iter()
            .copied()
            .filter(|n| mutable_decl.contains(n))
            .collect();
        mark_stmt(ast, s, &wanted, &wanted_mut, marks);
    }
}

/// Every name the statement's own subtree binds — the walk that
/// [`mark_stmt`] takes, asked the opposite question. Fn and class
/// bodies are not entered here either: a binding inside one cannot
/// shadow anything the walk will mark.
pub(super) fn collect_bound_names<'a>(s: &'a Stmt, out: &mut HashSet<&'a str>) {
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => {
            out.insert(name.as_str());
        }
        Stmt::FnDecl { name, .. } | Stmt::ClassDecl { name, .. } => {
            out.insert(name.as_str());
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            out.insert(var_name.as_str());
        }
        Stmt::Try {
            catch_param: Some(p),
            ..
        } => {
            out.insert(p.as_str());
        }
        _ => {}
    }
    for c in stmt_children_ref(s) {
        collect_bound_names(c, out);
    }
}

pub(super) fn mark_stmt(
    ast: &Ast,
    s: &Stmt,
    wanted: &HashSet<&str>,
    wanted_mut: &HashSet<&str>,
    out: &mut HashSet<ExprId>,
) {
    for root in stmt_exprs(s) {
        mark_expr(ast, root, wanted, wanted_mut, out);
    }
    for c in stmt_children_ref(s) {
        mark_stmt(ast, c, wanted, wanted_mut, out);
    }
}

fn mark_expr(
    ast: &Ast,
    eid: ExprId,
    wanted: &HashSet<&str>,
    wanted_mut: &HashSet<&str>,
    out: &mut HashSet<ExprId>,
) {
    if let Expr::Ident(name) = ast.get_expr(eid) {
        if wanted.contains(name.as_str()) {
            out.insert(eid);
        }
        return;
    }
    // An assignment's TARGET is the one position where the const
    // carve-out above applies; its own subtree (`o.k = v`) is ordinary
    // reads again.
    if let Expr::Assign { target, value } = ast.get_expr(eid) {
        match ast.get_expr(*target) {
            Expr::Ident(name) if wanted_mut.contains(name.as_str()) => {
                out.insert(*target);
            }
            Expr::Ident(_) => {}
            _ => mark_expr(ast, *target, wanted, wanted_mut, out),
        }
        mark_expr(ast, *value, wanted, wanted_mut, out);
        return;
    }
    for c in expr_children(ast, eid) {
        mark_expr(ast, c, wanted, wanted_mut, out);
    }
}

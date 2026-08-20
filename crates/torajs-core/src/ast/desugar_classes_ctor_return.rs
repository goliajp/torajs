//! RFC 20260820-ctor-return-override 刀 1 — which classes sit on an
//! inheritance chain that touches a value-returning constructor.
//!
//! §10.2.2 [[Construct]] step 13: a constructor that returns an object
//! makes `new C(…)` answer THAT object, not the one the construction
//! minted. tr's desugar hands every ctor a pre-minted `__this` and
//! declares it `void`, so the return had no channel at all and was
//! dropped silently. Giving every class the answering ABI would cost
//! the typed tier everywhere; this pass names the narrow set that
//! needs it (see the RFC's narrow-surface section).

use super::desugar_classes_super::ClassIndexEntry;
use super::{Ast, Expr, ExprId, Stmt};

/// Seed on the classes whose own ctor body carries a value return,
/// then spread DOWN first and only afterwards UP.
///
/// Descendants are the reason the set exists at all: `class C extends
/// Base {}` writes no `return`, yet `new C(o)` must answer whatever
/// Base returned — that is the whole test262
/// `privatefieldset-evaluation-order-3` shape. One forward sweep over
/// `class_index` suffices because a parent always precedes its child
/// there: `desugar_classes_fields` rejects the other order loudly
/// (`M5.2`), which is itself right — a class binding is in TDZ until
/// its declaration, so `class B extends A {}` above `class A` is a
/// ReferenceError, not a shape we should be closing over.
///
/// Ancestors join so that one chain speaks one ctor ABI. A member's
/// ctor takes `__this: any` and answers `any`; an outsider's takes the
/// class type and answers `void`. Passing a typed receiver into an
/// `any` parameter is a box and always sound, so widening upward costs
/// only the ancestors' own ctor bodies — whereas the other direction
/// (an `any` receiver arriving at a typed parameter) would need a
/// checked narrowing at every super site.
///
/// The two spreads do NOT compose into one fixed point, and the order
/// is load-bearing: a widened ancestor's OTHER subtrees stay out. Such
/// a sibling boxes its own receiver into the widened parameter and
/// ignores the answer, which is already correct — an ancestor that
/// carries no value return of its own hands back the very object it
/// was given. Running the descendant sweep after the widening instead
/// pulled every cousin in (the `sibling_subtree_*` test below is that
/// bug, kept as a regression).
pub(super) fn collect(ast: &mut Ast, class_index: &[ClassIndexEntry]) {
    let mut set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut parent: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for (_, cname, _tp, p, _, _, ctor, _, _) in class_index {
        if let Some(pname) = p.as_deref() {
            parent.insert(cname.as_str(), pname);
        }
        let seeded = ctor
            .as_ref()
            .is_some_and(|c| c.body.iter().any(|s| stmt_returns_value(ast, s)));
        if seeded || p.as_deref().is_some_and(|pn| set.contains(pn)) {
            set.insert(cname.clone());
        }
    }
    if set.is_empty() {
        return;
    }
    // Hop-capped by the class count so a malformed cycle cannot spin;
    // the inheritance graph gets its own validation in `desugar_classes`.
    let max_hops = class_index.len();
    for start in set.iter().cloned().collect::<Vec<_>>() {
        let mut cur = start.as_str();
        for _ in 0..max_hops {
            let Some(p) = parent.get(cur).copied() else {
                break;
            };
            set.insert(p.to_string());
            cur = p;
        }
    }
    ast.ctor_return_override = set;
}

/// Whether a ctor statement can hand back a value. Mirrors the
/// statement-container shape of `super_collect::collect_super_in_stmt`
/// next door — the other traversal that reads a ctor body — minus the
/// descent into expressions: a `return` is always a statement, and the
/// only returns living inside an expression belong to an arrow or
/// function body of their own, which is not this constructor's.
/// `Stmt::FnDecl` / `Stmt::ClassDecl` are skipped for the same reason.
///
/// `return;` is not a value return — per §10.2.2 an empty return
/// leaves `this` in place, exactly what today's `void` shape already
/// does. A written-out `return undefined;` reads the same way (step
/// 13's undefined branch also falls back to `this`), so it stays out
/// of the seed set: naming it would widen the ABI for a program that
/// cannot observe the difference.
fn stmt_returns_value(ast: &Ast, s: &Stmt) -> bool {
    let any = |list: &[Stmt]| list.iter().any(|s| stmt_returns_value(ast, s));
    match s {
        Stmt::Return(maybe) => maybe.is_some_and(|e| !is_undefined_literal(ast, e)),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_returns_value(ast, then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| stmt_returns_value(ast, b))
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. } => stmt_returns_value(ast, body),
        Stmt::For { init, body, .. } => {
            init.as_ref().is_some_and(|i| stmt_returns_value(ast, i))
                || stmt_returns_value(ast, body)
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| any(&c.body)) || default.as_ref().is_some_and(|b| any(b))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => any(stmts),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => any(body) || any(catch_body) || finally_body.as_ref().is_some_and(|b| any(b)),
        _ => false,
    }
}

fn is_undefined_literal(ast: &Ast, e: ExprId) -> bool {
    matches!(ast.get_expr(e), Expr::Ident(n) if n == "undefined")
}

#[cfg(test)]
mod tests {
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Run the real front end up through the class desugar, then read
    /// the set. Going through `desugar_classes` rather than calling
    /// `collect` on a hand-built index is the point: the seeds are
    /// read off `class_index` AFTER the default-ctor synthesis, so a
    /// hand-built fixture would not exercise the ordering this pass
    /// depends on.
    fn override_set(src: &str) -> std::collections::HashSet<String> {
        let tokens = tokenize(src).expect("lex");
        let mut ast = parse(src, &tokens).expect("parse");
        crate::ast::desugar_classes(&mut ast);
        ast.ctor_return_override.clone()
    }

    #[test]
    fn plain_classes_stay_out() {
        let s = override_set("class A { constructor() { this.x = 1; } }\nclass B extends A {}\n");
        assert!(s.is_empty(), "{s:?}");
    }

    #[test]
    fn value_return_seeds_and_reaches_descendants() {
        let s = override_set("class A { constructor(o) { return o; } }\nclass B extends A {}\n");
        assert!(s.contains("A"), "{s:?}");
        assert!(s.contains("B"), "{s:?}");
    }

    #[test]
    fn the_whole_descendant_chain_joins_not_just_the_first_hop() {
        let s = override_set(
            "class A { constructor(o) { return o; } }\n\
             class B extends A {}\n\
             class C extends B {}\n",
        );
        assert!(s.contains("C"), "{s:?}");
    }

    #[test]
    fn ancestors_widen_so_one_chain_speaks_one_abi() {
        let s = override_set(
            "class P { constructor() {} }\n\
             class C extends P { constructor() { super(); return {}; } }\n",
        );
        assert!(s.contains("P"), "{s:?}");
        assert!(s.contains("C"), "{s:?}");
    }

    #[test]
    fn sibling_subtree_of_a_widened_ancestor_stays_out() {
        // S's `super(…)` boxes its receiver into P's widened parameter
        // and ignores the answer — no reason to reshape S itself.
        let s = override_set(
            "class P { constructor() {} }\n\
             class C extends P { constructor() { super(); return {}; } }\n\
             class S extends P { constructor() { super(); } }\n",
        );
        assert!(!s.contains("S"), "{s:?}");
    }

    #[test]
    fn bare_and_undefined_returns_are_not_value_returns() {
        let s =
            override_set("class A { constructor(f) { if (f) { return; } return undefined; } }\n");
        assert!(s.is_empty(), "{s:?}");
    }

    #[test]
    fn a_return_nested_in_control_flow_still_seeds() {
        let s = override_set(
            "class A { constructor(o) { try { if (o) { return o; } } catch (e) {} } }\n",
        );
        assert!(s.contains("A"), "{s:?}");
    }

    #[test]
    fn a_return_inside_a_nested_function_belongs_to_that_function() {
        let s =
            override_set("class A { constructor() { this.f = function () { return {}; }; } }\n");
        assert!(s.is_empty(), "{s:?}");
    }
}

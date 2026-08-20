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
use super::{Ast, Expr, ExprId, Param, Stmt};

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

/// Does `__new_<C>` have to hand back whatever the constructor
/// answered?
///
/// Membership alone is not the question. A class can be in the set
/// only because a DESCENDANT return-overrides — the upward widening —
/// and if it declares no constructor of its own, its factory makes no
/// ctor call and there is no answer to relay. Such a factory keeps
/// its precise return type, and `new P()` keeps its typed tier.
pub(super) fn factory_relays_answer(ast: &Ast, cname: &str, has_ctor: bool) -> bool {
    has_ctor && ast.ctor_return_override.contains(cname)
}

/// Give a member class's `__cm_<C>__ctor` the answering shape
/// (blade 3). Mutates the parameter list and body that
/// `emit_ctor_fn` just assembled and hands back the return
/// annotation to declare.
///
/// The receiver arrives as `__this_in` and is immediately copied into
/// an ordinary mutable local named `__this`. Two things fall out of
/// that. Pass 2 already rewrote every `this` in the body into
/// `Ident("__this")`, so the local is what the body reads and writes
/// with no further rewriting. And because it is a local rather than a
/// parameter, reassigning it at the `super(…)` site is an ordinary
/// assignment with ordinary ownership — it releases what it held and
/// retains what it is given, which is why the step-13 pick must stay
/// borrow-shaped (retaining there too leaked a cell per construction;
/// see that kernel's doc). `__this_in` goes on naming the object
/// the factory minted, which is what the field carry needs.
///
/// The body's own `return` statements are left ALONE. Appending
/// `return __this` at the tail is enough: falling off answers the
/// current `this`, a written `return;` answers undefined, and a
/// written `return <expr>` answers it raw — and the step-13 pick at
/// the factory maps all three the way the spec asks. Rewriting each
/// return in place would mean a second body walk that has to stay in
/// lockstep with the seeding one below, for no added answer.
pub(super) fn reshape_ctor(ast: &mut Ast, params: &mut [Param], body: &mut Vec<Stmt>) -> String {
    params[0].name = "__this_in".into();
    params[0].type_ann = Some("any".into());
    let init = ast.add_expr(Expr::Ident("__this_in".into()));
    body.insert(
        0,
        Stmt::LetDecl {
            mutable: true,
            name: "__this".into(),
            type_ann: Some("any".into()),
            init,
            is_var: false,
        },
    );
    // A slot for what `super(…)` answered. Binding the parent
    // constructor's result to a LOCAL before the pick reads it is not
    // cosmetic: the SSA emits a release for a local holding a call's
    // result and none for a bare call result handed straight to
    // another call, so feeding the pick inline leaked the parent's
    // answer once per construction. Declared here rather than at the
    // super site because that rewrite replaces an EXPRESSION and has
    // nowhere to put a statement; one slot serves every super site in
    // the body, each assignment releasing what the last one left.
    let undef = ast.add_expr(Expr::Ident("undefined".into()));
    body.insert(
        1,
        Stmt::LetDecl {
            mutable: true,
            name: "__sup".into(),
            type_ann: Some("any".into()),
            init: undef,
            is_var: false,
        },
    );
    let tail = ast.add_expr(Expr::Ident("__this".into()));
    body.push(Stmt::Return(Some(tail)));
    "any".to_string()
}

/// `__torajs_ctor_ret_value(incumbent, candidate)` — the §10.2.2 step
/// 13 pick, minted for the factory and the super site alike.
pub(super) fn pick_call(ast: &mut Ast, incumbent: ExprId, candidate: ExprId) -> ExprId {
    let callee = ast.add_expr(Expr::Ident("__torajs_ctor_ret_value".into()));
    ast.add_expr(Expr::Call {
        callee,
        args: vec![incumbent, candidate],
    })
}

/// `__torajs_ctor_ret_carry(minted, target, "<name>")` — one own
/// element moved onto an adopted object.
pub(super) fn carry_call(ast: &mut Ast, target: ExprId, field: &str) -> ExprId {
    let callee = ast.add_expr(Expr::Ident("__torajs_ctor_ret_carry".into()));
    let minted = ast.add_expr(Expr::Ident("__this_in".into()));
    let name = ast.add_expr(Expr::String(field.to_string()));
    ast.add_expr(Expr::Call {
        callee,
        args: vec![minted, target, name],
    })
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

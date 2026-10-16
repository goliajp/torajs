//! `a?.m(args)` — the optional chain that ends in a call.
//!
//! §13.3.9: when the base of an optional chain is nullish the WHOLE
//! chain short-circuits to `undefined`, the call included, and the
//! arguments never evaluate. When it is not nullish, what remains is
//! an ordinary member call — a missing method there is an ordinary
//! TypeError, not another `undefined`.
//!
//! tr parsed such a site as `Call { callee: OptChain }` and had no
//! lane for it: the chain lowered to a VALUE which was then called.
//! That loses the receiver (a method reading `this` saw nothing), skips
//! the short-circuit (a nullish base reached a call on `undefined` and
//! threw), and for shapes with no fn-value lowering hit the
//! callee-form panic outright.
//!
//! This pass says the second half of the rule: the callee becomes a
//! plain `Member`, so the checker's class ladder and every dispatch
//! lane answer it as written. The first half — the guard — is what the
//! `optchain_calls` record is for; see its field doc.
//!
//! Only a `Call` callee is rewritten. `a?.m` as a VALUE stays an
//! `OptChain`: nothing is being invoked, so there is no receiver to
//! keep and no arguments to withhold.

use super::{Ast, Expr, ExprId};

mod spelled_callee;
pub use spelled_callee::distribute_spelled_callees;

/// One call to rewrite: the call's index, its optional link, the link's
/// base, the call's arguments and the `.name` steps past the link.
type Rewrite = (usize, ExprId, ExprId, Vec<ExprId>, Vec<String>);

pub fn desugar_optchain_calls(ast: &mut Ast) {
    let mut rewrites: Vec<Rewrite> = Vec::new();
    for i in 0..ast.exprs.len() {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        // the chain may go on past the optional link — `o?.m.call(t)`
        // short-circuits as a whole too (§13.3.9.1: an OptionalChain
        // continues through `.name`)
        let mut tail: Vec<String> = Vec::new();
        let mut link = *callee;
        // `(o?.m).call(t)`: the parentheses end the chain, so the call
        // past them is an ordinary one on its value
        let mut cut = false;
        while let Expr::Member { obj, name } = ast.get_expr(link) {
            cut |= ast.paren_optchains.contains(&link);
            tail.push(name.clone());
            link = *obj;
        }
        let Expr::OptChain { obj, name: _ } = ast.get_expr(link) else {
            continue;
        };
        if cut || (!tail.is_empty() && ast.paren_optchains.contains(&link)) {
            continue;
        }
        rewrites.push((i, link, *obj, args.clone(), tail));
    }
    for (i, callee, obj, args, tail) in rewrites {
        // `(o?.m)(args)`: the parenthesized chain is a reference to
        // `o.m` (§13.3.9.1 ChainEvaluation), so a present `o` is still
        // the receiver; a nullish one makes the chain `undefined`, and
        // calling that is a TypeError after the arguments evaluate
        // (§13.3.6.1 steps 3-4)
        if tail.is_empty() && ast.paren_optchains.contains(&callee) {
            ast.paren_optchain_calls.insert(ExprId(i as u32));
        }
        let Expr::OptChain { name, .. } = ast.get_expr(callee) else {
            unreachable!("collected as an OptChain callee");
        };
        let name = name.clone();
        // In the hit branch the base is known not to be nullish —
        // that is what the guard ahead of it decided — so the member
        // read says so with the non-null assertion the language
        // already has. Without it a `A | null` receiver fails the
        // ordinary member call it is now spelled as, which is a
        // question about the branch it cannot be in.
        let base = ast.add_expr(Expr::As {
            expr: obj,
            ty_ann: "__nonnull__".into(),
        });
        // A FRESH node rather than a mutation of the chain in place:
        // the same `OptChain` expr may also be read as a value
        // elsewhere in the arena, and that reading is still a chain.
        let mut member = ast.add_expr(Expr::Member { obj: base, name });
        for name in tail.into_iter().rev() {
            member = ast.add_expr(Expr::Member { obj: member, name });
        }
        ast.exprs[i] = Expr::Call {
            callee: member,
            args,
        };
        ast.optchain_calls.insert(ExprId(i as u32), obj);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `a?.m(1)` — one call whose callee is a chain.
    fn optchain_call() -> (Ast, ExprId) {
        let mut ast = Ast::default();
        let obj = ast.add_expr(Expr::Ident("a".into()));
        let callee = ast.add_expr(Expr::OptChain {
            obj,
            name: "m".into(),
        });
        let one = ast.add_expr(Expr::Number(1.0));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![one],
        });
        (ast, call)
    }

    #[test]
    fn the_callee_becomes_an_ordinary_member_read() {
        let (mut ast, call) = optchain_call();
        desugar_optchain_calls(&mut ast);
        let Expr::Call { callee, args } = ast.get_expr(call) else {
            panic!("the call stays a call")
        };
        assert_eq!(args.len(), 1, "the argument list is untouched");
        match ast.get_expr(*callee) {
            Expr::Member { name, .. } => assert_eq!(name, "m"),
            other => panic!("expected a Member callee, got {other:?}"),
        }
    }

    #[test]
    fn the_call_is_recorded_so_the_guard_can_be_put_back() {
        let (mut ast, call) = optchain_call();
        desugar_optchain_calls(&mut ast);
        assert!(ast.optchain_calls.contains_key(&call));
    }

    #[test]
    fn the_hit_branch_reads_the_receiver_as_non_nullish() {
        let (mut ast, call) = optchain_call();
        desugar_optchain_calls(&mut ast);
        let Expr::Call { callee, .. } = ast.get_expr(call) else {
            unreachable!()
        };
        let Expr::Member { obj, .. } = ast.get_expr(*callee) else {
            unreachable!()
        };
        let Expr::As { expr, ty_ann } = ast.get_expr(*obj) else {
            panic!("the hit branch asserts the base is there")
        };
        assert_eq!(ty_ann, "__nonnull__");
        assert!(matches!(ast.get_expr(*expr), Expr::Ident(n) if n == "a"));
    }

    #[test]
    fn the_base_the_guard_must_evaluate_is_the_one_written() {
        let (mut ast, call) = optchain_call();
        desugar_optchain_calls(&mut ast);
        let base = ast.optchain_calls[&call];
        assert!(matches!(ast.get_expr(base), Expr::Ident(n) if n == "a"));
    }

    #[test]
    fn a_chain_read_as_a_value_is_left_alone() {
        // `a?.m` with nothing invoked — still a chain
        let mut ast = Ast::default();
        let obj = ast.add_expr(Expr::Ident("a".into()));
        let chain = ast.add_expr(Expr::OptChain {
            obj,
            name: "m".into(),
        });
        desugar_optchain_calls(&mut ast);
        assert!(matches!(ast.get_expr(chain), Expr::OptChain { .. }));
        assert!(ast.optchain_calls.is_empty());
    }

    #[test]
    fn a_plain_member_call_is_not_recorded() {
        let mut ast = Ast::default();
        let obj = ast.add_expr(Expr::Ident("a".into()));
        let callee = ast.add_expr(Expr::Member {
            obj,
            name: "m".into(),
        });
        let call = ast.add_expr(Expr::Call {
            callee,
            args: Vec::new(),
        });
        desugar_optchain_calls(&mut ast);
        assert!(ast.optchain_calls.is_empty());
        assert!(matches!(ast.get_expr(call), Expr::Call { .. }));
    }

    #[test]
    fn a_chain_going_on_past_the_link_is_guarded_on_its_base() {
        // `a?.m.call(1)`
        let mut ast = Ast::default();
        let obj = ast.add_expr(Expr::Ident("a".into()));
        let chain = ast.add_expr(Expr::OptChain {
            obj,
            name: "m".into(),
        });
        let callee = ast.add_expr(Expr::Member {
            obj: chain,
            name: "call".into(),
        });
        let one = ast.add_expr(Expr::Number(1.0));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![one],
        });
        desugar_optchain_calls(&mut ast);
        assert_eq!(ast.optchain_calls.get(&call), Some(&obj));
        let Expr::Call { callee, .. } = ast.get_expr(call) else {
            panic!("the call stays a call")
        };
        let Expr::Member { obj: inner, name } = ast.get_expr(*callee) else {
            panic!("expected `.call`")
        };
        assert_eq!(name, "call");
        assert!(matches!(ast.get_expr(*inner), Expr::Member { name, .. } if name == "m"));
    }

    #[test]
    fn a_parenthesized_chain_ends_before_the_call() {
        // `(a?.m).call()`
        let mut ast = Ast::default();
        let obj = ast.add_expr(Expr::Ident("a".into()));
        let chain = ast.add_expr(Expr::OptChain {
            obj,
            name: "m".into(),
        });
        ast.paren_optchains.insert(chain);
        let callee = ast.add_expr(Expr::Member {
            obj: chain,
            name: "call".into(),
        });
        let call = ast.add_expr(Expr::Call {
            callee,
            args: Vec::new(),
        });
        desugar_optchain_calls(&mut ast);
        assert!(ast.optchain_calls.is_empty());
        assert!(matches!(ast.get_expr(call), Expr::Call { callee: c, .. } if *c == callee));
    }

    #[test]
    fn a_parenthesized_chain_called_directly_throws_when_nullish() {
        // `(a?.m)()` keeps `a` as the receiver and is marked to throw
        let mut ast = Ast::default();
        let obj = ast.add_expr(Expr::Ident("a".into()));
        let chain = ast.add_expr(Expr::OptChain {
            obj,
            name: "m".into(),
        });
        ast.paren_optchains.insert(chain);
        let call = ast.add_expr(Expr::Call {
            callee: chain,
            args: Vec::new(),
        });
        desugar_optchain_calls(&mut ast);
        assert_eq!(ast.optchain_calls.get(&call), Some(&obj));
        assert!(ast.paren_optchain_calls.contains(&call));
    }

    #[test]
    fn an_unparenthesized_chain_call_short_circuits() {
        let (mut ast, call) = optchain_call();
        desugar_optchain_calls(&mut ast);
        assert!(!ast.paren_optchain_calls.contains(&call));
    }
}

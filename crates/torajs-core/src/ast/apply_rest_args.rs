//! Rest-arg call-site desugar pass — split from `apply_args.rs`, whose
//! two passes had grown past the file-size line together.
//!
//! `apply_rest_args` packs trailing call-site args into an Array
//! literal at the rest-param position, and emits per-type
//! `__empty_arr__<sanitized>` helper FnDecls so the empty-rest shape
//! lowers through a typed local. Its sibling `apply_default_args`
//! stays next door; the two are independent, sharing only
//! `peel_hidden_params`.
//!
//! `pub` because torajs-cli main / cmd_build / lsp / repl call it at
//! `torajs_core::ast::apply_rest_args`; the `pub use` in ast.rs
//! preserves that path across this move.

use super::apply_args::peel_hidden_params;
use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

/// What a call site has to supply for one variadic callee.
#[derive(Clone)]
struct RestShape {
    /// How many arguments come before the tail.
    fixed: usize,
    /// The rest parameter's annotation, which picks the empty-array
    /// helper a call with no tail arguments reaches for.
    ann: String,
    /// Each fixed parameter's own default, for a call that stops
    /// short of it.
    defaults: Vec<Option<ExprId>>,
}

/// Pack trailing call-site args into an array literal when the
/// callee declares its last param with `...rest`. This pass mirrors
/// `apply_default_args` but for the rest-param shape.
///
/// The transformation: `f(a0, a1, …, ak)` where f's params are
/// `[p0, p1, ..., pn-1, ...rest]` becomes `f(a0, ..., an-1, [an, ..., ak])`
/// — the trailing args (positions n through k) get bundled into a
/// single Array literal at the rest-param position.
pub fn apply_rest_args(ast: &mut Ast) {
    let mut fn_rest: HashMap<String, RestShape> = HashMap::new();
    // The same reading, minus the receiver — see `Ast::rest_arg_prefix`.
    let mut arg_prefix: HashMap<String, usize> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            let user_params: &[Param] = peel_hidden_params(params);
            if let Some(last) = user_params.last() {
                if last.is_rest {
                    let fixed = user_params.len() - 1;
                    fn_rest.insert(
                        name.clone(),
                        RestShape {
                            fixed,
                            ann: last.type_ann.clone().unwrap_or_else(|| "any[]".into()),
                            defaults: user_params[..fixed].iter().map(|p| p.default).collect(),
                        },
                    );
                    let this_led = user_params.first().is_some_and(|p| p.name == "__this");
                    arg_prefix.insert(name.clone(), fixed.saturating_sub(usize::from(this_led)));
                }
            }
        }
    }
    // Published for the lanes that have no packing site of their own.
    // A Member-shape call survives desugar when several unrelated
    // classes declare the name — sibling-class dispatch resolves the
    // receiver at SSA level, not here — and that lane handed a
    // rest-declaring body its trailing arguments one per register, so
    // the parameter read a scalar where it expects an array. It reads
    // this table rather than its own walk because a second reading of
    // "where does the tail begin" is a second thing to drift.
    ast.rest_arg_prefix = arg_prefix;
    if fn_rest.is_empty() {
        return;
    }
    // Pre-synthesize empty-array helper FnDecls per rest type ann. Each
    // helper has shape `function __empty_arr_<sanitized>(): T[] {
    //   let _e: T[] = []; return _e; }`. The let-binding's annotation
    // gives ssa-lower the typed-empty-array path.
    let mut empty_helpers: HashMap<String, String> = HashMap::new();
    for shape in fn_rest.values() {
        let rest_ann = &shape.ann;
        if !empty_helpers.contains_key(rest_ann) {
            let sanitized: String = rest_ann
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let helper_name = format!("__empty_arr__{sanitized}");
            empty_helpers.insert(rest_ann.clone(), helper_name);
        }
    }
    // Emit the helpers as new FnDecls.
    for (rest_ann, helper_name) in &empty_helpers {
        // Skip if already present.
        let exists = ast
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == helper_name));
        if exists {
            continue;
        }
        let arr_lit = ast.add_expr(Expr::Array(Vec::new()));
        let body = vec![
            Stmt::LetDecl {
                mutable: false,
                name: "_e".into(),
                type_ann: Some(rest_ann.clone()),
                init: arr_lit,
                is_var: false,
            },
            Stmt::Return(Some(ast.add_expr(Expr::Ident("_e".into())))),
        ];
        ast.stmts.push(Stmt::FnDecl {
            name: helper_name.clone(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Some(rest_ann.clone()),
            body,
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        });
    }
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Call { callee, args } = &ast.exprs[i] {
            let callee = *callee;
            let name = match ast.get_expr(callee) {
                Expr::Ident(n) => n.clone(),
                _ => continue,
            };
            let Some(RestShape {
                fixed,
                ann: rest_ann,
                defaults,
            }) = fn_rest.get(&name).cloned()
            else {
                continue;
            };
            let args_clone = args.clone();
            let given = args_clone.len().min(fixed);
            let mut new_args: Vec<ExprId> = args_clone[..given].to_vec();
            let rest_elems: Vec<ExprId> = args_clone[given..].to_vec();
            // §10.2.11 — a call that stops short of the fixed prefix
            // binds the missing parameters to their defaults, or to
            // undefined, and the tail to the empty array. It is settled
            // HERE because this is the pass that says what a variadic
            // call site looks like: the by-name default padding one
            // pass earlier has nothing to put in the tail, so it backs
            // out of the whole call — which left `g()` on
            // `function g(x, ...r)` refused for want of an argument the
            // language does not ask for.
            for d in &defaults[given..] {
                let pad = match d {
                    Some(dflt) => *dflt,
                    None => ast.add_expr(Expr::Ident("undefined".into())),
                };
                new_args.push(pad);
            }
            // Single-spread shape: `f(req…, ...src)`. Common in
            // delegating wrappers, and it used to hand the source
            // straight through as the rest param — which is not what a
            // rest param is. ES §10.4.2 builds it with
            // CreateArrayFromList, i.e. a FRESH array, so
            // `function g(...xs) { xs.push(9) }` called as `g(...arr)`
            // must not touch `arr`; it did. The same shortcut is why
            // `f(..."xyz")` was a type error rather than three
            // arguments: a String is not an `Array(String)`, but it IS
            // iterable, and only the pass-through cared about the
            // difference.
            //
            // `Array.from` is exactly CreateArrayFromList over an
            // iterable, and is already code-point-correct for strings
            // (the same intrinsic rotation 307 pointed the array-literal
            // spread at), so both fall out of spelling the copy.
            let single_spread_only =
                rest_elems.len() == 1 && matches!(ast.get_expr(rest_elems[0]), Expr::Spread { .. });
            let rest_arr = if rest_elems.is_empty() {
                let helper_name = empty_helpers.get(&rest_ann).cloned().unwrap();
                let callee_id = ast.add_expr(Expr::Ident(helper_name));
                ast.add_expr(Expr::Call {
                    callee: callee_id,
                    args: Vec::new(),
                })
            } else if single_spread_only {
                let Expr::Spread { expr } = ast.get_expr(rest_elems[0]) else {
                    unreachable!()
                };
                let src = *expr;
                let array_id = ast.add_expr(Expr::Ident("Array".into()));
                let from_id = ast.add_expr(Expr::Member {
                    obj: array_id,
                    name: "from".into(),
                });
                ast.add_expr(Expr::Call {
                    callee: from_id,
                    args: vec![src],
                })
            } else {
                ast.add_expr(Expr::Array(rest_elems))
            };
            new_args.push(rest_arr);
            ast.exprs[i] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnd(name: &str, params: Vec<Param>) -> Stmt {
        Stmt::FnDecl {
            name: name.into(),
            type_params: Vec::new(),
            params,
            return_type: None,
            body: Vec::new(),
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        }
    }

    fn p(name: &str, is_rest: bool) -> Param {
        Param {
            name: name.into(),
            type_ann: None,
            default: None,
            is_rest,
        }
    }

    fn prefixes(stmts: Vec<Stmt>) -> HashMap<String, usize> {
        let mut ast = Ast {
            stmts,
            ..Ast::default()
        };
        apply_rest_args(&mut ast);
        ast.rest_arg_prefix
    }

    #[test]
    fn the_receiver_is_not_an_argument() {
        // `new A().f(1, 2, 3)` supplies one argument before the tail
        let m = prefixes(vec![fnd(
            "__cm_A__f",
            vec![p("__this", false), p("x", false), p("r", true)],
        )]);
        assert_eq!(m.get("__cm_A__f"), Some(&1));
    }

    #[test]
    fn a_plain_function_counts_every_fixed_parameter() {
        let m = prefixes(vec![fnd(
            "g",
            vec![p("x", false), p("y", false), p("r", true)],
        )]);
        assert_eq!(m.get("g"), Some(&2));
    }

    #[test]
    fn a_fixed_declaration_has_no_tail_to_begin() {
        let m = prefixes(vec![fnd("g", vec![p("x", false)])]);
        assert!(m.is_empty());
    }

    fn call_args(mut ast: Ast, decl: Stmt, given: Vec<ExprId>) -> (Ast, Vec<ExprId>) {
        ast.stmts.push(decl);
        let callee = ast.add_expr(Expr::Ident("g".into()));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: given,
        });
        apply_rest_args(&mut ast);
        let args = match ast.get_expr(call) {
            Expr::Call { args, .. } => args.clone(),
            _ => unreachable!("the call stays a call"),
        };
        (ast, args)
    }

    #[test]
    fn a_call_short_of_the_prefix_binds_undefined_and_an_empty_tail() {
        // `function g(x, ...r) {}` called as `g()`
        let (ast, args) = call_args(
            Ast::default(),
            fnd("g", vec![p("x", false), p("r", true)]),
            Vec::new(),
        );
        assert_eq!(args.len(), 2, "one fixed pad and the tail");
        assert!(matches!(ast.get_expr(args[0]), Expr::Ident(n) if n == "undefined"));
        assert!(
            matches!(ast.get_expr(args[1]), Expr::Call { .. }),
            "the empty-array helper"
        );
    }

    #[test]
    fn a_missing_parameter_takes_its_own_default() {
        // `function g(x = 5, ...r) {}` called as `g()` — the default
        // is the callee's own, not an unrelated declaration's
        let mut ast = Ast::default();
        let five = ast.add_expr(Expr::Number(5.0));
        let mut x = p("x", false);
        x.default = Some(five);
        let (_, args) = call_args(ast, fnd("g", vec![x, p("r", true)]), Vec::new());
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], five);
    }

    #[test]
    fn arguments_past_the_prefix_are_the_tail() {
        // `g(1, 2, 3)` on `function g(x, ...r) {}` — one fixed
        // argument and a two-element tail
        let mut ast = Ast::default();
        let given: Vec<ExprId> = (1..=3)
            .map(|n| ast.add_expr(Expr::Number(n.into())))
            .collect();
        let (ast, args) = call_args(ast, fnd("g", vec![p("x", false), p("r", true)]), given);
        assert_eq!(args.len(), 2);
        match ast.get_expr(args[1]) {
            Expr::Array(elems) => assert_eq!(elems.len(), 2),
            other => panic!("the tail is an array literal, got {other:?}"),
        }
    }

    #[test]
    fn a_hidden_head_is_peeled_before_the_count() {
        // an `__env`-first body: neither the environment nor the
        // receiver behind it is written at a call site
        let m = prefixes(vec![fnd(
            "__closure_0",
            vec![
                p("__env", false),
                p("__this", false),
                p("x", false),
                p("r", true),
            ],
        )]);
        assert_eq!(m.get("__closure_0"), Some(&1));
    }
}

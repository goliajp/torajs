//! Dynamic-import (§13.3.10) dispatcher synthesis — the runtime half
//! of [`super::dyn_import`]: the `__torajs_dyn_import` FnDecl that
//! matches a specifier against the candidate table
//! ([`synth_dispatcher`]); the poison verdict it consults lives in
//! [`super::dyn_import_poison`]. Split from `dyn_import.rs` when it crossed
//! the file-size line (rotation 645); candidate collection stays
//! there.

use crate::ast::{Ast, BinOp, Expr, Param, Stmt};
use std::collections::{HashMap, HashSet};

use super::dyn_import::{DISPATCHER_NAME, DynTarget};

/// `if (<cond>) throw new SyntaxError(<reason>)` — the dispatcher's
/// catch turns it into the §16.2.1.5 promise reject.
fn push_syntax_error_arm(
    ast: &mut Ast,
    body: &mut Vec<Stmt>,
    cond: crate::ast::ExprId,
    reason: String,
) {
    let msg = ast.add_expr(Expr::String(reason.into()));
    let err = ast.add_expr(Expr::New {
        class_name: "SyntaxError".to_string(),
        args: vec![msg],
        type_args: Vec::new(),
    });
    body.push(Stmt::If {
        cond,
        then_branch: Box::new(Stmt::Throw(err)),
        else_branch: None,
    });
}

/// Synthesize the dispatcher FnDecl:
///
/// ```ts
/// function __torajs_dyn_import(__spec: any): any {
///   try {
///     const __s: any = String(__spec);
///     if (__s === "<lit_0>") return Promise.resolve(__dyn_ns_c0);
///     ...
///     throw new TypeError("Failed to resolve module specifier '" + __s + "'");
///   } catch (__e) {
///     return Promise.reject(__e);
///   }
/// }
/// ```
///
/// The whole §13.3.10 abrupt surface funnels through ONE catch: a
/// throwing `toString` (step 7) and the miss TypeError both land in
/// `Promise.reject`, which is exactly IfAbruptRejectPromise. The
/// namespace each eval fn answers is bound inside it, after the module
/// body has run (`synth_eval_fn`).
///
/// The dispatcher plus, per loadable candidate, its evaluation
/// function and the two state bindings that memoize it
/// ([`synth_eval_fn`]); the caller splices all of them.
pub(super) fn synth_dispatcher(
    ast: &mut Ast,
    table: &[(String, DynTarget)],
    poisoned: &HashSet<String>,
    eval_bodies: &HashMap<String, Vec<Stmt>>,
    ns_fields: &HashMap<String, Vec<(String, String)>>,
) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::new();
    let mut body: Vec<Stmt> = Vec::new();
    // const __s: any = String(__spec);
    let string_ident = ast.add_expr(Expr::Ident("String".to_string()));
    // §13.3.10 runs the intrinsic ToString — like the Promise idents
    // below, no `with` object may substitute these globals.
    ast.synth_global_refs.insert(string_ident);
    let spec_ident = ast.add_expr(Expr::Ident("__spec".to_string()));
    let s_init = ast.add_expr(Expr::Call {
        callee: string_ident,
        args: vec![spec_ident],
    });
    body.push(Stmt::LetDecl {
        mutable: false,
        name: "__s".to_string(),
        type_ann: Some("any".to_string()),
        init: s_init,
        is_var: false,
    });
    // Per candidate: if (__s === "<lit>") return Promise.resolve(<ns>);
    for (lit, target) in table {
        let s_ref = ast.add_expr(Expr::Ident("__s".to_string()));
        let lit_expr = ast.add_expr(Expr::String(lit.clone().into()));
        let cond = ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: s_ref,
            right: lit_expr,
        });
        let ns_name = match target {
            DynTarget::SyntaxError(reason) => {
                push_syntax_error_arm(ast, &mut body, cond, reason.clone());
                continue;
            }
            DynTarget::Namespace(ns) if poisoned.contains(ns.as_str()) => {
                push_syntax_error_arm(
                    ast,
                    &mut body,
                    cond,
                    format!(
                        "indirect exports of module '{lit}' do not resolve (circular, missing, or ambiguous)"
                    ),
                );
                continue;
            }
            DynTarget::Namespace(ns) => ns,
        };
        // if (__s === "<lit>") return Promise.resolve(undefined).then(() => __dyn_eval_c<k>());
        // — the module body evaluates in a job (§16.2.1.5.3 runs it
        // from HostLoadImportedModule's continuation, never inside
        // the `import()` expression), and a throw there rejects the
        // returned promise through the `.then`.
        let stmts = eval_bodies
            .get(ns_name.as_str())
            .cloned()
            .unwrap_or_default();
        let fields = ns_fields.get(ns_name.as_str()).cloned().unwrap_or_default();
        let eval_name = synth_eval_fn(ast, &mut out, ns_name, stmts, fields);
        let promise_ident = ast.add_expr(Expr::Ident("Promise".to_string()));
        ast.synth_global_refs.insert(promise_ident);
        let resolve = ast.add_expr(Expr::Member {
            obj: promise_ident,
            name: "resolve".to_string(),
        });
        let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
        let resolved = ast.add_expr(Expr::Call {
            callee: resolve,
            args: vec![undef],
        });
        let then = ast.add_expr(Expr::Member {
            obj: resolved,
            name: "then".to_string(),
        });
        let eval_ident = ast.add_expr(Expr::Ident(eval_name));
        let eval_call = ast.add_expr(Expr::Call {
            callee: eval_ident,
            args: Vec::new(),
        });
        let thunk = ast.add_expr(Expr::ArrowFn {
            params: Vec::new(),
            return_type: Some("any".to_string()),
            body: vec![Stmt::Return(Some(eval_call))],
        });
        let chained = ast.add_expr(Expr::Call {
            callee: then,
            args: vec![thunk],
        });
        body.push(Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Return(Some(chained))),
            else_branch: None,
        });
    }
    // throw new TypeError("Failed to resolve module specifier '" + __s + "'");
    let msg_head = ast.add_expr(Expr::String(
        "Failed to resolve module specifier '".to_string().into(),
    ));
    let s_ref = ast.add_expr(Expr::Ident("__s".to_string()));
    let head_concat = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: msg_head,
        right: s_ref,
    });
    let msg_tail = ast.add_expr(Expr::String("'".to_string().into()));
    let msg = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: head_concat,
        right: msg_tail,
    });
    let type_error = ast.add_expr(Expr::New {
        class_name: "TypeError".to_string(),
        args: vec![msg],
        type_args: Vec::new(),
    });
    body.push(Stmt::Throw(type_error));
    // catch (__e) { return Promise.reject(__e); }
    let promise_ident = ast.add_expr(Expr::Ident("Promise".to_string()));
    ast.synth_global_refs.insert(promise_ident);
    let reject = ast.add_expr(Expr::Member {
        obj: promise_ident,
        name: "reject".to_string(),
    });
    let e_ident = ast.add_expr(Expr::Ident("__e".to_string()));
    let rejected = ast.add_expr(Expr::Call {
        callee: reject,
        args: vec![e_ident],
    });
    let catch_body = vec![Stmt::Return(Some(rejected))];
    out.push(Stmt::FnDecl {
        name: DISPATCHER_NAME.to_string(),
        type_params: Vec::new(),
        params: vec![Param {
            name: "__spec".to_string(),
            type_ann: Some("any".to_string()),
            default: None,
            is_rest: false,
        }],
        return_type: Some("any".to_string()),
        body: vec![Stmt::Try {
            body,
            had_catch: true,
            catch_param: Some("__e".to_string()),
            catch_type: None,
            catch_body,
            finally_body: None,
        }],
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
    out
}

/// One candidate's module evaluation, memoized (§16.2.1.5.3 — a module
/// evaluates once; a later `import()` of one that threw rejects with
/// the same value, `[[EvaluationError]]`):
///
/// ```ts
/// const __dyn_memo_c<k>: any = { state: 0, err: undefined, ns: undefined };  // 0 unevaluated / 1 done / 2 threw
/// function __dyn_eval_c<k>(): any {
///   if (__dyn_memo_c<k>.state === 0) {
///     __dyn_memo_c<k>.state = 1;
///     try { <the candidate's top-level statements> }
///     catch (__e) { __dyn_memo_c<k>.state = 2; __dyn_memo_c<k>.err = __e; }
///   }
///   if (__dyn_memo_c<k>.state === 2) throw __dyn_memo_c<k>.err;
///   if (__dyn_memo_c<k>.ns === undefined) {
///     let __dyn_ns_c<k>: any = { name1: local1, … };   // a `namespace_bindings` row: dynobj lane + module_ns_finalize
///     __dyn_memo_c<k>.ns = __dyn_ns_c<k>;
///   }
///   return __dyn_memo_c<k>.ns;
/// }
/// ```
///
/// The namespace is built AFTER the body ran because its fields are
/// value snapshots of the exports, and once per module (memoized on
/// the memo object) because every `import()` of a module answers the
/// same namespace object (§16.2.1.5.3 / §10.4.6). It is a `let` in
/// `namespace_bindings` like a static `import * as ns`, which is what
/// gives it the §10.4.6 exotic attributes.
///
/// The memo is one object rather than two top-level `let`s: a
/// `__`-spelled top-level binding is kept out of the global-write lane
/// by design, so a function assigning it is "assign to unknown ident";
/// a field write on a top-level `any` object needs no lane.
///
/// The candidate's DECLARATIONS still inject at the top level, eagerly
/// (the namespace literal names them; `dyn_import.rs`'s "candidates
/// are EAGER" boundary), so an initializer runs at program start and
/// only the declaration-free statements wait for the import — the
/// order between the two is the one deviation left. A body holding
/// a top-level `await` is not hosted by a plain function and keeps
/// the drop. Answers the function's name.
fn synth_eval_fn(
    ast: &mut Ast,
    out: &mut Vec<Stmt>,
    ns_name: &str,
    stmts: Vec<Stmt>,
    fields: Vec<(String, String)>,
) -> String {
    let suffix = ns_name.trim_start_matches("__dyn_ns_");
    let memo = format!("__dyn_memo_{suffix}");
    let eval = format!("__dyn_eval_{suffix}");
    let stmts = if stmts.iter().any(|s| crate::ast::stmt_has_await(ast, s)) {
        Vec::new()
    } else {
        stmts
    };
    let zero = ast.add_expr(Expr::Number(0.0));
    let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
    let undef_ns = ast.add_expr(Expr::Ident("undefined".to_string()));
    let memo_init = ast.add_expr(Expr::ObjectLit {
        fields: vec![
            ("state".into(), zero),
            ("err".into(), undef),
            ("ns".into(), undef_ns),
        ],
    });
    out.push(Stmt::LetDecl {
        mutable: false,
        name: memo.clone(),
        type_ann: Some("any".to_string()),
        init: memo_init,
        is_var: false,
    });
    let field = |ast: &mut Ast, name: &str| {
        let obj = ast.add_expr(Expr::Ident(memo.clone()));
        ast.add_expr(Expr::Member {
            obj,
            name: name.to_string(),
        })
    };
    let set_state = |ast: &mut Ast, n: f64| {
        let target = field(ast, "state");
        let value = ast.add_expr(Expr::Number(n));
        Stmt::Expr(ast.add_expr(Expr::Assign { target, value }))
    };
    let state_is = |ast: &mut Ast, n: f64| {
        let left = field(ast, "state");
        let right = ast.add_expr(Expr::Number(n));
        ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left,
            right,
        })
    };
    let mark_running = set_state(ast, 1.0);
    let mark_threw = set_state(ast, 2.0);
    let err_target = field(ast, "err");
    let e_read = ast.add_expr(Expr::Ident("__e".to_string()));
    let store_err = Stmt::Expr(ast.add_expr(Expr::Assign {
        target: err_target,
        value: e_read,
    }));
    let run = Stmt::Try {
        body: stmts,
        had_catch: true,
        catch_param: Some("__e".to_string()),
        catch_type: None,
        catch_body: vec![mark_threw, store_err],
        finally_body: None,
    };
    let unevaluated = state_is(ast, 0.0);
    let threw = state_is(ast, 2.0);
    let err_read = field(ast, "err");
    // if (memo.ns === undefined) { let <ns>: any = {…}; memo.ns = <ns>; }
    let ns_slot = field(ast, "ns");
    let undef_cmp = ast.add_expr(Expr::Ident("undefined".to_string()));
    let unbound = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: ns_slot,
        right: undef_cmp,
    });
    let ns_lit = super::namespace_objlit::namespace_objlit_expr(ast, &fields);
    ast.namespace_bindings.insert(ns_name.to_string(), fields);
    let bind_ns = Stmt::LetDecl {
        mutable: false,
        name: ns_name.to_string(),
        type_ann: Some("any".to_string()),
        init: ns_lit,
        is_var: false,
    };
    let ns_store_target = field(ast, "ns");
    let ns_value = ast.add_expr(Expr::Ident(ns_name.to_string()));
    let store_ns = Stmt::Expr(ast.add_expr(Expr::Assign {
        target: ns_store_target,
        value: ns_value,
    }));
    let ns_read = field(ast, "ns");
    out.push(Stmt::FnDecl {
        name: eval.clone(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Some("any".to_string()),
        body: vec![
            Stmt::If {
                cond: unevaluated,
                then_branch: Box::new(Stmt::Block(vec![mark_running, run])),
                else_branch: None,
            },
            Stmt::If {
                cond: threw,
                then_branch: Box::new(Stmt::Throw(err_read)),
                else_branch: None,
            },
            Stmt::If {
                cond: unbound,
                then_branch: Box::new(Stmt::Block(vec![bind_ns, store_ns])),
                else_branch: None,
            },
            Stmt::Return(Some(ns_read)),
        ],
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
    eval
}

//! `using` declaration desugar — Explicit Resource Management (RFC
//! 20260809 B2). Runs in the AST prelude AFTER `desugar_eval` (so
//! eval-inlined statements are in place) and BEFORE
//! `hoist_gen_fn_exprs` / `desugar_generators` — a generator body's
//! `using` becomes plain try/finally here, and the state-machine
//! rewrite then inherits the dispose-on-return()/throw() paths from
//! its ordinary finally handling.
//!
//! The rewrite is the textbook transpiler shape (TypeScript 5.2
//! emit; V8's own implementation is likewise an explicit dispose
//! stack). For a statement list whose first resource sits at
//! position i:
//!
//! ```text
//! pre…  using a = ea;  mid…  using b = eb;  post…
//! ──►
//! pre…
//! const __usenv_N: any = { stack: [], error: undefined, hasError: false };
//! try {
//!   const a = __torajs_using_add(__usenv_N, ea);
//!   mid…
//!   const b = __torajs_using_add(__usenv_N, eb);
//!   post…
//! } catch (__usc_N) { __torajs_using_caught(__usenv_N, __usc_N); }
//!   finally { __torajs_using_dispose(__usenv_N); }
//! ```
//!
//! `__torajs_using_add` skips null/undefined, reads
//! `[Symbol.dispose]` ONCE at bind time (§AddDisposableResource —
//! gets-initializer-Symbol.dispose-property-once), throws TypeError
//! on a non-callable method, and stacks `(value, method)` pairs.
//! `__torajs_using_dispose` walks the stack in REVERSE, calling each
//! method with its value as `this`; a dispose throw joins an
//! in-flight thrown completion as `SuppressedError(e, error)` and
//! replaces a normal/break/return completion outright — which the
//! try/catch/finally shape gives for free (the finally's throw wins).
//! The helpers are parsed from source (`parse_into`, the modules.rs
//! append convention) and injected only when the program actually
//! used `using` — programs without it pay nothing.
//!
//! `Stmt::Multi` carrying a `using` multi-binding is spliced into
//! the surrounding list first: Multi shares the enclosing scope by
//! definition, so its resources belong to the enclosing list's env.
//! A `for (using x = …;;)` init registers in the env wrapped around
//! the whole For statement — the loop-exit dispose timing the spec
//! gives that head form. After this pass no `Stmt::UsingDecl`
//! survives anywhere in the tree.

use super::{Ast, Expr, ExprId, Stmt};

pub fn desugar_using(ast: &mut Ast) {
    let mut n: u32 = 0;
    let mut stmts = std::mem::take(&mut ast.stmts);
    rewrite_list(&mut stmts, ast, &mut n);
    ast.stmts = stmts;
    // Arrow-fn bodies live in the expr arena (raw-parse period: the
    // closure lift runs much later). One flat sweep covers arrows at
    // any nesting depth. The rewrite appends only Ident/Call exprs,
    // never a new ArrowFn, so the pre-sweep len bound is complete.
    for i in 0..ast.exprs.len() {
        if matches!(ast.exprs[i], Expr::ArrowFn { .. }) {
            let Expr::ArrowFn { ref mut body, .. } = ast.exprs[i] else {
                unreachable!()
            };
            let mut b = std::mem::take(body);
            rewrite_list(&mut b, ast, &mut n);
            let Expr::ArrowFn { ref mut body, .. } = ast.exprs[i] else {
                unreachable!()
            };
            *body = b;
        }
    }
    if n > 0 {
        inject_helpers(ast);
    }
}

/// Does this statement anchor a block-lifetime resource at the
/// current list level? Only a bare `using` — a `for (using …;;)`
/// head disposes when the FOR completes, not at block exit, so it
/// gets its own single-statement wrap (the For arm in
/// `rewrite_list`'s scan loop) instead of joining the tail.
fn is_using_head(s: &Stmt) -> bool {
    matches!(s, Stmt::UsingDecl { .. })
}

fn init_holds_using(init: &Stmt) -> bool {
    match init {
        Stmt::UsingDecl { .. } => true,
        Stmt::Multi(inner) => inner.iter().any(|s| matches!(s, Stmt::UsingDecl { .. })),
        _ => false,
    }
}

fn rewrite_list(stmts: &mut Vec<Stmt>, ast: &mut Ast, n: &mut u32) {
    let mut i = 0;
    while i < stmts.len() {
        // A Multi carrying using-decls shares THIS scope — splice its
        // members in and re-examine from the same index (the
        // uninit_let convention; a nested Multi re-enters this arm).
        if matches!(&stmts[i], Stmt::Multi(inner) if inner.iter().any(|s| matches!(s, Stmt::UsingDecl { .. })))
        {
            let Stmt::Multi(inner) = std::mem::replace(&mut stmts[i], Stmt::Break(None)) else {
                unreachable!()
            };
            stmts.splice(i..=i, inner);
            continue;
        }
        rewrite_in_stmt(&mut stmts[i], ast, n);
        // `for (using r = …;;)` disposes when the FOR completes —
        // wrap just this one statement in its own env, in place
        // (the wrap product is a share-scope Multi, so the list
        // keeps its shape; the For body was already recursed).
        if matches!(&stmts[i], Stmt::For { init: Some(init), .. } if init_holds_using(init)) {
            let taken = std::mem::replace(&mut stmts[i], Stmt::Break(None));
            stmts[i] = wrap_with_env(vec![taken], ast, n);
        }
        i += 1;
    }
    let Some(first) = stmts.iter().position(is_using_head) else {
        return;
    };
    let tail: Vec<Stmt> = stmts.split_off(first);
    let wrapped = wrap_with_env(tail, ast, n);
    match wrapped {
        Stmt::Multi(mut parts) => stmts.append(&mut parts),
        other => stmts.push(other),
    }
}

/// Build `const __usenv_N: any = {…}; try { tail′ } catch (__usc_N)
/// { caught } finally { dispose }` around `tail`, rewriting the
/// tail's top-level using-decls (and for-init using-decls) onto the
/// fresh env. Answers a share-scope `Stmt::Multi` of the two
/// statements.
fn wrap_with_env(mut tail: Vec<Stmt>, ast: &mut Ast, n: &mut u32) -> Stmt {
    let env_name = format!("__usenv_{n}");
    let catch_name = format!("__usc_{n}");
    *n += 1;
    for s in tail.iter_mut() {
        replace_using_shallow(s, &env_name, ast);
    }
    // The env is built INLINE with an `: any` annotation — that is
    // what routes the literal down the dynobj lane, whose properties
    // the helpers (any params) can write. A helper-returned literal
    // would type as Struct, and a struct cell boxed to any rejects
    // property assignment ("cannot assign to a property of this any
    // value") — measured, not theoretical.
    let stack_eid = ast.add_expr(Expr::Array(Vec::new()));
    let undef_eid = ast.add_expr(Expr::Ident("undefined".into()));
    let false_eid = ast.add_expr(Expr::Bool(false));
    let env_init = ast.add_expr(Expr::ObjectLit {
        fields: vec![
            ("stack".into(), stack_eid),
            ("error".into(), undef_eid),
            ("hasError".into(), false_eid),
        ],
    });
    let env_decl = Stmt::LetDecl {
        mutable: false,
        name: env_name.clone(),
        type_ann: Some("any".into()),
        init: env_init,
        is_var: false,
    };
    let env_eid = ast.add_expr(Expr::Ident(env_name.clone()));
    let catch_eid = ast.add_expr(Expr::Ident(catch_name.clone()));
    let caught = call_expr(ast, "__torajs_using_caught", vec![env_eid, catch_eid]);
    let env_eid2 = ast.add_expr(Expr::Ident(env_name));
    let dispose = call_expr(ast, "__torajs_using_dispose", vec![env_eid2]);
    let try_stmt = Stmt::Try {
        body: tail,
        had_catch: true,
        catch_param: Some(catch_name),
        catch_type: None,
        catch_body: vec![Stmt::Expr(caught)],
        finally_body: Some(vec![Stmt::Expr(dispose)]),
    };
    Stmt::Multi(vec![env_decl, try_stmt])
}

/// Turn a top-of-tail `using x = e` into
/// `const x = __torajs_using_add(env, e)`. Only the CURRENT list
/// level — nested lists were already rewritten (each with its own
/// env) by the recursion in `rewrite_list`.
fn replace_using_shallow(s: &mut Stmt, env_name: &str, ast: &mut Ast) {
    match s {
        Stmt::UsingDecl { .. } => {
            let Stmt::UsingDecl {
                name,
                type_ann,
                init,
            } = std::mem::replace(s, Stmt::Break(None))
            else {
                unreachable!()
            };
            let env_eid = ast.add_expr(Expr::Ident(env_name.to_string()));
            let add = call_expr(ast, "__torajs_using_add", vec![env_eid, init]);
            *s = Stmt::LetDecl {
                mutable: false,
                name,
                type_ann,
                init: add,
                is_var: false,
            };
        }
        Stmt::For {
            init: Some(init), ..
        } if init_holds_using(init) => {
            match init.as_mut() {
                Stmt::Multi(inner) => {
                    for m in inner.iter_mut() {
                        if matches!(m, Stmt::UsingDecl { .. }) {
                            replace_using_shallow(m, env_name, ast);
                        }
                    }
                }
                one => replace_using_shallow(one, env_name, ast),
            };
        }
        _ => {}
    }
}

/// Recurse into every nested statement list / single-statement body.
/// A bare `using` in a single-statement position (`if (c) using x =
/// y;`) is grammatically a declaration where only statements are
/// allowed; it gets a Block wrap so the rewrite gives it an
/// immediately-disposed scope of its own — the binding is
/// unreferencable there anyway.
fn rewrite_in_stmt(s: &mut Stmt, ast: &mut Ast, n: &mut u32) {
    match s {
        Stmt::FnDecl { body, .. } => rewrite_list(body, ast, n),
        Stmt::Block(inner) => rewrite_list(inner, ast, n),
        Stmt::Multi(inner) => {
            // using-bearing Multis were spliced by the caller; any
            // other Multi just recurses.
            for m in inner.iter_mut() {
                rewrite_in_stmt(m, ast, n);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_body_box(then_branch, ast, n);
            if let Some(e) = else_branch {
                rewrite_body_box(e, ast, n);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => rewrite_body_box(body, ast, n),
        Stmt::For { init, body, .. } => {
            // The init's own UsingDecl is handled by the ENCLOSING
            // list (is_using_head / replace_using_shallow) — the
            // spec's loop-exit dispose timing. Recurse only into a
            // non-using init and the body.
            if let Some(init) = init
                && !init_holds_using(init)
            {
                rewrite_in_stmt(init, ast, n);
            }
            rewrite_body_box(body, ast, n);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                rewrite_list(&mut c.body, ast, n);
            }
            if let Some(d) = default {
                rewrite_list(d, ast, n);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_list(body, ast, n);
            rewrite_list(catch_body, ast, n);
            if let Some(f) = finally_body {
                rewrite_list(f, ast, n);
            }
        }
        Stmt::ClassDecl {
            static_init,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            for si in static_init.iter_mut() {
                if let super::StaticInit::Block(b) = si {
                    rewrite_list(b, ast, n);
                }
            }
            if let Some(c) = ctor {
                rewrite_list(&mut c.body, ast, n);
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                rewrite_list(&mut m.body, ast, n);
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => rewrite_in_stmt(inner, ast, n),
        _ => {}
    }
}

fn rewrite_body_box(b: &mut Box<Stmt>, ast: &mut Ast, n: &mut u32) {
    if is_using_head(b)
        || matches!(b.as_ref(), Stmt::Multi(inner) if inner.iter().any(|s| matches!(s, Stmt::UsingDecl { .. })))
        || matches!(b.as_ref(), Stmt::For { init: Some(init), .. } if init_holds_using(init))
    {
        let taken = std::mem::replace(b.as_mut(), Stmt::Break(None));
        *b.as_mut() = Stmt::Block(vec![taken]);
    }
    rewrite_in_stmt(b.as_mut(), ast, n);
}

fn call_expr(ast: &mut Ast, f: &str, args: Vec<ExprId>) -> ExprId {
    let callee = ast.add_expr(Expr::Ident(f.to_string()));
    ast.add_expr(Expr::Call { callee, args })
}

/// The runtime shape of the rewrite, injected as ordinary top-level
/// fns. Note: spec passes NO message to the SuppressedError here
/// (§DisposeResources 3.b.i), leaving no own `message` property; the
/// injected ctor's checker face wants a String, so we pass "" — the
/// observable `.message` read is "" either way (Error.prototype
/// fallback), only Object.hasOwnProperty("message") differs. B6
/// (SuppressedError checker face: optional params) closes that.
/// Injected via the `parse_into` append convention (modules.rs);
/// the fns ride the full downstream pipeline like user code, and
/// `new SuppressedError(…)` here is what makes
/// `inject_builtin_classes` (which runs later) materialize that
/// class.
const HELPER_SRC: &str = r#"
function __torajs_using_add(env: any, value: any): any {
  if (value === null || value === undefined) {
    return value;
  }
  const m = value[Symbol.dispose];
  if (typeof m !== "function") {
    throw new TypeError("value is not disposable: [Symbol.dispose] is not a function");
  }
  env.stack.push({ v: value, d: m });
  return value;
}
function __torajs_using_caught(env: any, e: any): void {
  env.error = e;
  env.hasError = true;
}
function __torajs_using_dispose(env: any): void {
  const st = env.stack;
  let error = env.error;
  let hasError = env.hasError;
  let i = st.length - 1;
  while (i >= 0) {
    const r = st[i];
    try {
      r.d.call(r.v);
    } catch (e) {
      if (hasError) {
        error = new SuppressedError(e, error, "");
      } else {
        error = e;
      }
      hasError = true;
    }
    i = i - 1;
  }
  if (hasError) {
    throw error;
  }
}
"#;

fn inject_helpers(ast: &mut Ast) {
    let tokens = crate::lexer::tokenize(HELPER_SRC).expect("using helpers lex");
    let offset = crate::parser::parse_into(HELPER_SRC, &tokens, ast).expect("using helpers parse");
    // The parsed spans index HELPER_SRC, not the program's source —
    // consumers slicing `ast.source` with them would read out of
    // bounds. Synthesized FnDecls carry the (0,0) "no user source"
    // sentinel (the stmt.rs span contract); stamp it here.
    for s in ast.stmts[offset..].iter_mut() {
        if let Stmt::FnDecl { span, .. } = s {
            *span = crate::lexer::Span { start: 0, end: 0 };
        }
    }
}

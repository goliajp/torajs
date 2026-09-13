//! RFC 20260805 blade 1 — compile an async function to a state machine.
//!
//! An async function's `await` must SUSPEND: control returns to the
//! caller at the first one, and the body resumes from a microtask.
//! tr's `await` lowering instead drains the whole queue and reads the
//! settled slot inline (`ssa_lower_member_promise_value`), so the body
//! runs to completion inside the caller's frame — two async functions
//! never interleave, and the statement after the call runs last.
//!
//! The textbook compilation is a generator state machine plus a
//! promise driver, and tr already owns both halves: `desugar_generators`
//! builds machines with try regions and finally frames, and the
//! microtask queue orders `.then` correctly. This pass connects them:
//!
//! ```text
//! async function f(a: A): Promise<T> { BODY }
//!   ⇒  function* __async_gen__f(a: A): any { BODY′ }   // await → yield
//!      function f(a: A): Promise<any> { return __async_drive(__async_gen__f(a)); }
//! ```
//!
//! Runs BEFORE `desugar_generators`, which then builds `__async_gen__f`'s
//! machine like any other `function*`, and before `desugar_async` — a
//! rewritten function leaves `ast.async_fns`, so its `return e` keeps
//! the bare value the driver's `resolve` settles with.
//!
//! **Which functions move.** `Stmt::Yield` is statement-level, so an
//! `await` in expression position (`f(await a, await b)`, `if (await x)`)
//! needs an A-normal lift that blade 2 owns. Until then a function moves
//! only when every one of its awaits already sits where a yield can:
//! `await e;` and `let v = await e;`. A function holding any other await
//! keeps today's lowering verbatim — mixed programs are no worse than
//! today's, because each moved function is CORRECT, and the check is
//! conservative in the safe direction (an await this pass cannot place
//! blocks the move rather than being silently dropped).

use super::{Ast, Expr, ExprId, Param, Stmt};

/// Prefix of the generator holding a moved async function's body. The
/// wrapper keeps the user's name so every call site is untouched.
const GEN_PREFIX: &str = "__async_gen__";

/// The driver, injected once when at least one function moves.
///
/// Every name the driver binds is `__`-spelled. The receiver-rule
/// census in `fnexpr_this_default` is name-keyed across the whole
/// program (deliberately coarse): a user's `var r = f()` stopped being
/// a certain promise binding the moment the driver also declared an
/// `r`, and every fn-expression handler on it became an unclaimed
/// `this` (`fnexpr-this-var-promise-recv-001`). Injected text must
/// not share a namespace with the program it is injected into.
///
/// `__torajs_await_resolve` is §27.7.5.3 Await step 2 —
/// PromiseResolve(%Promise%, v) through the intrinsic. `Promise.resolve`
/// spelled in the driver honoured a user patch or accessor on the
/// static and was counted by programs that instrument it.
///
/// `__async_step` is deliberately a TOP-LEVEL function taking the
/// generator as a parameter rather than a self-recursive nested one
/// closing over it: a self-recursive nested function that merely
/// MENTIONS a captured generator object segfaults at exit today
/// (probe: recursion never has to happen — the recursive call site
/// alone is enough; a plain class instance, array, string or promise
/// in the same position is fine). That defect is filed separately;
/// this shape does not depend on it being fixed.
const DRIVER_SRC: &str = r#"
function __async_step(__g: any, __resolve: any, __reject: any, __v: any, __is_err: boolean): void {
  let __r: any;
  try {
    __r = __is_err ? __g.throw(__v) : __g.next(__v);
  } catch (__e: any) {
    __reject(__e);
    return;
  }
  if (__r.done) {
    __resolve(__r.value);
    return;
  }
  const __w: any = __torajs_await_resolve(__r.value);
  __w.then(
    (__x: any) => { __async_step(__g, __resolve, __reject, __x, false); return 0; },
    (__e: any) => { __async_step(__g, __resolve, __reject, __e, true); return 0; },
  );
}

function __async_drive(__mk: any): Promise<any> {
  return new Promise((__resolve: any, __reject: any) => {
    let __g: any;
    try {
      __g = __mk();
    } catch (__e: any) {
      __reject(__e);
      return;
    }
    __async_step(__g, __resolve, __reject, undefined, false);
  });
}
"#;

pub fn desugar_async_state_machine(ast: &mut Ast) {
    if ast.async_fns.is_empty() {
        return;
    }
    // Snapshot by index so `ast.stmts` can be rebuilt in place. Async
    // GENERATORS are excluded: their bodies carry real `yield`s beside
    // the await reads, and telling the two apart in one machine is
    // blade 4.
    let targets: Vec<usize> = ast
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl {
                name,
                is_generator: false,
                ..
            } if ast.async_fns.contains(name) && !ast.async_generator_fns.contains(name) => Some(i),
            _ => None,
        })
        .collect();

    let mut generators: Vec<Stmt> = Vec::new();
    for idx in targets {
        let Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type,
            body,
            span,
            ..
        } = ast.stmts[idx].clone()
        else {
            continue;
        };
        if super::desugar_async_sm_gate::blocks_move(ast, &params, &body) {
            continue;
        }
        let mut moved = body.clone();
        if !rewrite_body(ast, &mut moved) {
            continue;
        }
        let gen_name = format!("{GEN_PREFIX}{name}");
        generators.push(Stmt::FnDecl {
            name: gen_name.clone(),
            type_params: type_params.clone(),
            params: generator_params(&params),
            // P10.7 default-Any generator: yields and the return value
            // both ride the Any tier, which is what the driver moves.
            return_type: Some("any".into()),
            body: moved,
            is_generator: true,
            span,
        });
        let wrapper_params = wrapper_params(ast, &params);
        ast.stmts[idx] = Stmt::FnDecl {
            name: name.clone(),
            type_params,
            params: wrapper_params,
            return_type: Some(promise_ann(return_type.as_deref())),
            body: vec![Stmt::Return(Some(build_drive_call(
                ast, &gen_name, &params,
            )))],
            is_generator: false,
            // The user wrote this declaration; `toString` answers their
            // source text, not the wrapper's.
            span,
        };
        // The name STAYS in `async_fns` — a dozen readers answer "is
        // this an async function" off that set (the fnexpr-this
        // receiver rule, any-widening, the fn value's async flavour
        // bit, the §15.8.4 param-throw posture), and every one of
        // those answers is still true of the wrapper. Only
        // `desugar_async` must skip it, or the wrapper's `return
        // __async_drive(...)` would get a second `Promise.resolve`
        // wrap; that is what this set is for. (The first landing
        // removed the name instead and lost all of those readers at
        // once — three of rotation 640's 19 blockers were that.)
        ast.async_sm_fns.insert(name.clone());
    }

    // Blade 3 — async methods, before `desugar_classes` turns them into
    // `__cm_` FnDecls.
    let moved_methods = super::desugar_async_sm_methods::rewrite_async_methods(ast);
    if generators.is_empty() && !moved_methods {
        return;
    }
    ast.stmts.extend(generators);
    inject_driver(ast);
}

/// The wrapper's return annotation. An async function's declared type
/// is already `Promise<T>` (or bare `T`, which `desugar_async` used to
/// wrap); the driver hands back `Promise<any>` either way, so the
/// wrapper says so and the Any tier carries T.
fn promise_ann(_declared: Option<&str>) -> String {
    "Promise<any>".into()
}

/// `__async_drive(() => __async_gen__f(a, b, rest))`.
///
/// The generator is constructed INSIDE the driver's `try`: calling it
/// runs FunctionDeclarationInstantiation, which is where a parameter
/// default's initializer throws, and §15.8.4 step 3 wants that throw
/// as a rejection of the returned promise, not a synchronous one from
/// the wrapper. The thunk carries the wrapper's parameters in; the
/// wrapper's rest parameter arrives already packed and the generator
/// takes it as a plain array (`generator_params`).
fn build_drive_call(ast: &mut Ast, gen_name: &str, params: &[Param]) -> ExprId {
    let gen_ident = ast.add_expr(Expr::Ident(gen_name.into()));
    build_drive_call_to(ast, gen_ident, params)
}

/// The same call with the generator given as an expression — the
/// method rewrite hands its receiver-first parameter list and the
/// generator's name.
pub(super) fn build_drive_call_to(ast: &mut Ast, gen_ident: ExprId, params: &[Param]) -> ExprId {
    let args: Vec<ExprId> = params
        .iter()
        .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
        .collect();
    let gen_call = ast.add_expr(Expr::Call {
        callee: gen_ident,
        args,
    });
    let thunk = ast.add_expr(Expr::ArrowFn {
        params: Vec::new(),
        return_type: Some("any".into()),
        body: vec![Stmt::Return(Some(gen_call))],
    });
    let drive_ident = ast.add_expr(Expr::Ident("__async_drive".into()));
    ast.add_expr(Expr::Call {
        callee: drive_ident,
        args: vec![thunk],
    })
}

/// The wrapper's parameter list: the user's, with every defaulted
/// parameter turned into an `any` OPTIONAL — `x = init` becomes
/// `x?: any` — so the initializer runs once, in the generator, when the
/// wrapper forwards the absent argument as `undefined` (§10.2.11: an
/// `undefined` argument takes the default; the lowering's or_default
/// lane binds it for an `any` actual). Left on the wrapper it would run
/// at the wrapper's entry too — twice per call, and a throwing one
/// would escape synchronously. Only `any`-spelled defaults get here
/// (`typed_default_param` gates the rest); rest parameters are kept as
/// written — the wrapper packs, the generator takes the array.
pub(super) fn wrapper_params(ast: &mut Ast, params: &[Param]) -> Vec<Param> {
    params
        .iter()
        .map(|p| {
            let Some(d) = p.default else {
                return p.clone();
            };
            // An optional parameter already IS the shape the wrapper
            // wants; only its implicit default is minted afresh (two
            // declarations must not share one arena node).
            if matches!(ast.get_expr(d), Expr::Ident(n)
                if n == "undefined" || n.starts_with(crate::ast::UNDEF_SLOT_MARKER))
            {
                let type_ann = p.type_ann.clone();
                let default = crate::parser::param_optional_default::implicit_optional_default(
                    ast,
                    type_ann.as_deref(),
                );
                return Param {
                    name: p.name.clone(),
                    type_ann,
                    default: Some(default),
                    is_rest: p.is_rest,
                };
            }
            // A boxed scalar keeps its spelling as `n?: number` — the
            // slot is a box either way, and the annotation still gates
            // the caller. Anything else the gate let through is `any`.
            let type_ann = match p.type_ann.as_deref() {
                Some(a @ ("number" | "boolean")) => format!("__nullable({a})"),
                Some(a @ ("__nullable(number)" | "__nullable(boolean)")) => a.to_string(),
                _ => "any".to_string(),
            };
            let default = crate::parser::param_optional_default::implicit_optional_default(
                ast,
                Some(&type_ann),
            );
            Param {
                name: p.name.clone(),
                type_ann: Some(type_ann),
                default: Some(default),
                is_rest: p.is_rest,
            }
        })
        .collect()
}

/// The generator's parameter list: the user's, with a rest parameter
/// demoted to a plain array parameter. The wrapper's own rest already
/// packed the caller's arguments; the thunk hands that array over as
/// one argument, and a rest on the generator would pack it again.
pub(super) fn generator_params(params: &[Param]) -> Vec<Param> {
    params
        .iter()
        .map(|p| Param {
            name: p.name.clone(),
            type_ann: p.type_ann.clone(),
            default: p.default,
            is_rest: false,
        })
        .collect()
}

/// Parse the driver into the shared arena once. `parse_into` keeps
/// ExprId numbering continuous, so nothing needs remapping.
fn inject_driver(ast: &mut Ast) {
    if ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == "__async_drive"))
    {
        return;
    }
    let Ok(tokens) = crate::lexer::tokenize(DRIVER_SRC) else {
        return;
    };
    let expr_base = ast.exprs.len();
    let type_ann_base = ast.type_ann_spans.len();
    let Ok(offset) = crate::parser::parse_into(DRIVER_SRC, &tokens, ast) else {
        return;
    };
    // The driver's type annotations record spans too, and those index
    // DRIVER_SRC as well: an `: any` ~620 bytes into the driver sliced
    // the user's file at that offset — inside an em-dash in a comment
    // of `array-from-async-001` ("end byte index 621 is not a char
    // boundary"). Same sentinel as the expression spans below.
    for sp in ast.type_ann_spans.iter_mut().skip(type_ann_base) {
        *sp = crate::lexer::Span { start: 0, end: 0 };
    }
    // Same hazard on the per-expression tables: an arrow's span rides
    // `expr_spans` until `lift_arrow_fns` turns it into a `__closure_N`
    // declaration, and the driver's three arrows would carry
    // DRIVER_SRC offsets into a slice of the user's file.
    let sentinel = crate::lexer::Span { start: 0, end: 0 };
    for sp in ast.expr_spans.iter_mut().skip(expr_base) {
        *sp = sentinel;
    }
    // The driver's spans index DRIVER_SRC, but every span consumer
    // downstream slices the user's `ast.source` — out of bounds when
    // that file is shorter, silently wrong `toString` text otherwise.
    // Same hazard, same sentinel as an injected module's decls.
    for s in &mut ast.stmts[offset..] {
        crate::modules::splice::clear_injected_spans(s);
    }
}

/// Rewrite every statement-level await into a yield. Answers false —
/// leaving `body` untouched for the caller to discard — when any await
/// sits somewhere `Stmt::Yield` cannot go.
pub(super) fn rewrite_body(ast: &mut Ast, body: &mut [Stmt]) -> bool {
    body.iter_mut().all(|s| rewrite_stmt(ast, s))
}

fn rewrite_stmt(ast: &mut Ast, s: &mut Stmt) -> bool {
    match s {
        // `await e;`
        Stmt::Expr(eid) => {
            if let Some(inner) = await_inner(ast, *eid) {
                *s = Stmt::Yield(inner);
                return !expr_has_await(ast, inner);
            }
            !expr_has_await(ast, *eid)
        }
        // `let v = await e;`
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            if let Some(inner) = await_inner(ast, *init) {
                *s = Stmt::YieldInto {
                    var: name.clone(),
                    type_ann: type_ann.clone(),
                    value: inner,
                };
                return !expr_has_await(ast, inner);
            }
            !expr_has_await(ast, *init)
        }
        Stmt::Return(v) => v.is_none_or(|e| !expr_has_await(ast, e)),
        Stmt::Throw(e) => !expr_has_await(ast, *e),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            !expr_has_await(ast, *cond)
                && rewrite_stmt(ast, then_branch)
                && else_branch
                    .as_deref_mut()
                    .is_none_or(|b| rewrite_stmt(ast, b))
        }
        Stmt::While { cond, body } => !expr_has_await(ast, *cond) && rewrite_stmt(ast, body),
        Stmt::DoWhile { cond, body } => !expr_has_await(ast, *cond) && rewrite_stmt(ast, body),
        Stmt::Labeled { body, .. } => rewrite_stmt(ast, body),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => rewrite_body(ast, stmts),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_body(ast, body)
                && rewrite_body(ast, catch_body)
                && finally_body
                    .as_deref_mut()
                    .is_none_or(|f| rewrite_body(ast, f))
        }
        // Everything else blocks the move if it holds an await; the
        // conservative direction is refusing to move, never dropping a
        // suspend point. `for` heads, `switch` discriminants and the
        // for-of/for-in iterables all land here.
        other => !stmt_has_await(ast, other),
    }
}

/// `Some(inner)` when `eid` IS a parser-minted `await inner` read.
fn await_inner(ast: &Ast, eid: ExprId) -> Option<ExprId> {
    if !ast.await_value_reads.contains(&eid) {
        return None;
    }
    match ast.get_expr(eid) {
        Expr::Member { obj, name } if name == "value" => Some(*obj),
        _ => None,
    }
}

fn stmt_has_await(ast: &Ast, s: &Stmt) -> bool {
    let mut found = false;
    super::desugar_async_sm_walk::visit_stmt_exprs(s, &mut |e| {
        found = found || expr_has_await(ast, e);
    });
    found
}

/// True when `eid`'s subtree holds any await read.
pub(super) fn expr_has_await(ast: &Ast, eid: ExprId) -> bool {
    if ast.await_value_reads.contains(&eid) {
        return true;
    }
    let mut found = false;
    super::desugar_async_sm_walk::visit_expr_children(ast, eid, &mut |c| {
        found = found || expr_has_await(ast, c);
    });
    found
}

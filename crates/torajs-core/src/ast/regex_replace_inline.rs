//! Which `s.replace(re, cb)` / `s.replaceAll(re, cb)` calls can run
//! their walk in the program's own code, with the match and every
//! capture handed to the callback as a view of the subject instead of
//! a fresh string — and the twin of each such callback that reads its
//! parameters as views.
//!
//! The kernel builds a string per capture per match, calls the
//! callback through a pointer and copies the answer into its buffer.
//! None of that is observable when the pattern is known, the callback
//! is a lifted arrow and its body reads each parameter only in ways a
//! view answers without escaping: as a `+` operand, as `.length`, as
//! the receiver of `.charCodeAt`, in an equality test, as an argument
//! of `console.log`, or as the value returned. For such a call the
//! lowering (`ssa_lower_call_str_regex_emit::replace_inline`) drives
//! `__torajs_regex_replace_iter_*` itself, cuts stack views at the
//! spans and calls the twin directly, which the inliner then folds
//! into the loop. Anything else keeps the kernel: the walk is the same
//! and the callback is the original, so a miss costs nothing.
//!
//! The twin is a clone of the lifted body under `<name>$$rview`. The
//! checker checks it beside the original at the construction site
//! (`check_type_of_fn::check_closure`); the lowering gives its string
//! parameters the `Substr` type (`ssa_lower_pass_1` / `ssa_lower_fn`)
//! and reads the original's env layout for it (`widen_clone_origin`),
//! since the call enters it with the original cell's env. Only that
//! call names it: the boxed-entry table skips it and no fn value ever
//! carries its address.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::stmt_nested_lists::for_each_nested_list;
use super::{Ast, BinOp, Expr, ExprId, Param, Stmt, toplevel_regex_literal};

/// The suffix the view twin's name carries.
const TWIN_SUFFIX: &str = "$$rview";

/// Record the admitted `.replace` / `.replaceAll` member ExprIds and
/// mint the view twin of each admitted callback. Runs at the end of
/// the desugar pipeline, after every pass that could add a use.
pub fn analyze_regex_replace_inline(ast: &mut Ast) {
    let console_rebound = binds_name(ast, "console");
    let sites: Vec<(ExprId, String)> = (0..ast.exprs.len())
        .filter_map(|i| admitted_site(ast, ExprId(i as u32), console_rebound))
        .collect();
    for (callee, fn_name) in sites {
        let twin = match ast.regex_replace_view_fns.get(&fn_name) {
            Some(t) => t.clone(),
            None => synthesize_twin(ast, &fn_name),
        };
        ast.regex_replace_inline_callees.insert(callee, twin);
    }
}

impl Ast {
    /// Whether `name` is the view twin of a replace callback.
    pub fn is_replace_view_fn(&self, name: &str) -> bool {
        self.widen_clone_origin
            .get(name)
            .and_then(|orig| self.regex_replace_view_fns.get(orig))
            .is_some_and(|twin| twin == name)
    }
}

/// `(callee member, lifted callback name)` when the call at `e` is
/// `recv.replace(pattern, closure)` on the lane.
fn admitted_site(ast: &Ast, e: ExprId, console_rebound: bool) -> Option<(ExprId, String)> {
    let Expr::Call { callee, args } = ast.get_expr(e) else {
        return None;
    };
    let Expr::Member { name: method, .. } = ast.get_expr(*callee) else {
        return None;
    };
    if method != "replace" && method != "replaceAll" || args.len() < 2 {
        return None;
    }
    if args
        .iter()
        .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
    {
        return None;
    }
    let (pattern, flags) = match ast.get_expr(args[0]) {
        Expr::Regex { pattern, flags } => (pattern.as_str(), flags.as_str()),
        Expr::Ident(n) => toplevel_regex_literal(ast, n)?,
        _ => return None,
    };
    // §22.1.3.20 step 2.b: replaceAll on a non-global RegExp is a
    // TypeError, which the kernel raises
    if method == "replaceAll" && !flags.contains('g') {
        return None;
    }
    let Expr::Closure { fn_name, .. } = ast.get_expr(args[1]) else {
        return None;
    };
    // a callback reading `arguments` needs the whole list, which only
    // its boxed entry carries
    if super::argv_face_callback::replace_callback_needs_boxed(ast, args[1]) {
        return None;
    }
    if ast.regex_replace_view_fns.contains_key(fn_name) {
        return Some((*callee, fn_name.clone()));
    }
    let n_caps = crate::ssa_lower_free_helpers::count_capture_groups(pattern);
    let (params, body) = lifted_decl(ast, fn_name)?;
    if ast.fnexpr_recv_fns.contains(fn_name)
        || ast.closure_argv_fns.contains(fn_name)
        || ast.async_fns.contains(fn_name)
        || ast.fn_async_value_fns.contains(fn_name)
        || ast.gen_env_methods.contains_key(fn_name)
    {
        return None;
    }
    let user: Vec<&Param> = params.iter().skip(1).collect();
    if !callback_shape_ok(&user, n_caps) {
        return None;
    }
    let views_ok = user
        .iter()
        .filter(|p| p.type_ann.as_deref() == Some("string"))
        .all(|p| {
            body.iter()
                .all(|s| stmt_ok(ast, s, &p.name, console_rebound))
        });
    views_ok.then(|| (*callee, fn_name.clone()))
}

/// The lifted callback's user params and body.
fn lifted_decl<'a>(ast: &'a Ast, fn_name: &str) -> Option<(&'a [Param], &'a [Stmt])> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            body,
            is_generator: false,
            ..
        } if name == fn_name && params.first().is_some_and(|p| p.name == "__env") => {
            Some((params.as_slice(), body.as_slice()))
        }
        _ => None,
    })
}

/// The typed callback ABI's two shapes, spelled on the annotations:
/// «matched, captures…» as `string`s, at most `n_caps + 1` of them,
/// or that list for every capture plus `position: number, string:
/// string`. A default or rest parameter takes the callback off the
/// lane; `ssa_lower_call_str_regex_emit::typed_callback_shape` asks
/// the same question of the signature the lowering built.
fn callback_shape_ok(user: &[&Param], n_caps: usize) -> bool {
    if user.iter().any(|p| p.default.is_some() || p.is_rest) {
        return false;
    }
    fn ann(p: &Param) -> Option<&str> {
        p.type_ann.as_deref()
    }
    let n = user.len();
    if n >= 3 && ann(user[n - 2]) == Some("number") {
        return n == n_caps + 3
            && ann(user[n - 1]) == Some("string")
            && user[..n - 2].iter().all(|p| ann(p) == Some("string"));
    }
    n <= n_caps + 1 && user.iter().all(|p| ann(p) == Some("string"))
}

fn is_x(ast: &Ast, e: ExprId, x: &str) -> bool {
    matches!(ast.get_expr(e), Expr::Ident(n) if n == x)
}

/// Every mention of `x` in `s` is a view read.
fn stmt_ok(ast: &Ast, s: &Stmt, x: &str, console_rebound: bool) -> bool {
    let exprs_ok = match s {
        // the return lowering copies a view out into an owned string
        Stmt::Return(Some(e)) if is_x(ast, *e, x) => true,
        _ => stmt_exprs(s)
            .into_iter()
            .all(|e| expr_ok(ast, e, x, console_rebound)),
    };
    exprs_ok
        && stmt_children_ref(s)
            .into_iter()
            .all(|c| stmt_ok(ast, c, x, console_rebound))
        && !matches!(s, Stmt::FnDecl { body, .. } if body.iter().any(|b| mentions_stmt(ast, b, x)))
}

/// `e` is a view read of `x` (the listed shapes), or mentions `x`
/// only inside such reads. A bare `x` anywhere else — an alias, an
/// argument to user code, a container element, a capture — lets the
/// view escape the iteration it was cut in.
fn expr_ok(ast: &Ast, e: ExprId, x: &str, console_rebound: bool) -> bool {
    let operand_ok = |o: ExprId| is_x(ast, o, x) || expr_ok(ast, o, x, console_rebound);
    match ast.get_expr(e) {
        Expr::Ident(n) => n != x,
        Expr::BinOp {
            op: BinOp::Add | BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq,
            left,
            right,
        } => operand_ok(*left) && operand_ok(*right),
        Expr::Member { obj, name } if is_x(ast, *obj, x) => name == "length",
        Expr::Call { callee, args } => {
            let view_call = match ast.get_expr(*callee) {
                Expr::Member { obj, name } if is_x(ast, *obj, x) => {
                    name == "charCodeAt" && args.len() <= 1
                }
                Expr::Member { obj, name } if !console_rebound => {
                    name == "log" && matches!(ast.get_expr(*obj), Expr::Ident(c) if c == "console")
                }
                _ => false,
            };
            (view_call || expr_ok(ast, *callee, x, console_rebound))
                && args.iter().all(|a| {
                    if view_call {
                        operand_ok(*a)
                    } else {
                        expr_ok(ast, *a, x, console_rebound)
                    }
                })
        }
        Expr::Closure { captures, .. } => !captures.iter().any(|c| c == x),
        Expr::ArrowFn { body, .. } => !body.iter().any(|s| mentions_stmt(ast, s, x)),
        _ => expr_children(ast, e)
            .into_iter()
            .all(|c| expr_ok(ast, c, x, console_rebound)),
    }
}

fn mentions_stmt(ast: &Ast, s: &Stmt, x: &str) -> bool {
    stmt_exprs(s).into_iter().any(|e| mentions(ast, e, x))
        || stmt_children_ref(s)
            .into_iter()
            .any(|c| mentions_stmt(ast, c, x))
        || matches!(s, Stmt::FnDecl { body, .. } if body.iter().any(|b| mentions_stmt(ast, b, x)))
}

fn mentions(ast: &Ast, e: ExprId, x: &str) -> bool {
    is_x(ast, e, x)
        || matches!(ast.get_expr(e), Expr::Closure { captures, .. } if captures.iter().any(|c| c == x))
        || matches!(ast.get_expr(e), Expr::ArrowFn { body, .. } if body.iter().any(|s| mentions_stmt(ast, s, x)))
        || expr_children(ast, e)
            .into_iter()
            .any(|c| mentions(ast, c, x))
}

/// Some declaration anywhere in the program binds `name`, so a read
/// of it may not be the builtin.
fn binds_name(ast: &Ast, name: &str) -> bool {
    fn in_list(stmts: &[Stmt], name: &str) -> bool {
        stmts.iter().any(|s| {
            let here = match s {
                Stmt::LetDecl { name: n, .. }
                | Stmt::UsingDecl { name: n, .. }
                | Stmt::ClassDecl { name: n, .. } => n == name,
                Stmt::FnDecl {
                    name: n, params, ..
                } => n == name || params.iter().any(|p| p.name == name),
                Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                    var_name == name
                }
                Stmt::Try {
                    catch_param: Some(p),
                    ..
                } => p == name,
                _ => false,
            };
            let mut nested = false;
            for_each_nested_list(s, &mut |inner| nested = nested || in_list(inner, name));
            here || nested
        })
    }
    in_list(&ast.stmts, name)
}

/// Clone the lifted body under the twin name and mirror the tables
/// keyed by the original's name onto it.
fn synthesize_twin(ast: &mut Ast, orig: &str) -> String {
    let twin = format!("{orig}{TWIN_SUFFIX}");
    let (params, return_type, body, span) = ast
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::FnDecl {
                name,
                params,
                return_type,
                body,
                span,
                ..
            } if name == orig => Some((params.clone(), return_type.clone(), body.clone(), *span)),
            _ => None,
        })
        .expect("an admitted callback has a lifted declaration");
    let (params, return_type, body, _) =
        crate::check_monomorph::clone_spec_body(ast, &params, &return_type, &body, &[]);
    ast.stmts.push(Stmt::FnDecl {
        name: twin.clone(),
        type_params: Vec::new(),
        params,
        return_type,
        body,
        is_generator: false,
        span,
    });
    crate::check_monomorph_closures::mirror_name_keyed_tables(ast, orig, &twin);
    if ast.contextual_ret_fns.contains(orig) {
        ast.contextual_ret_fns.insert(twin.clone());
    }
    ast.widen_clone_origin
        .insert(twin.clone(), orig.to_string());
    ast.regex_replace_view_fns
        .insert(orig.to_string(), twin.clone());
    twin
}
